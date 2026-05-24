//! Background cron scheduler with 1-second tick and circuit breaker.
//!
//! The scheduler polls all active jobs every second, fires those that match
//! the current time (with jitter), and tracks consecutive failures to
//! auto-disable broken jobs.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::Local;
use chrono::Timelike;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::config::CronConfig;
use crate::jitter::compute_one_shot_early_fire;
use crate::jitter::compute_recurring_jitter;
use crate::matcher::matches_cron;
use crate::persistence::save_durable_jobs;
use crate::store::CronJobStore;
use crate::types::CronJobStatus;

/// Event emitted when a cron job fires.
#[derive(Debug, Clone)]
pub struct CronFireEvent {
    /// Job identifier.
    pub job_id: String,
    /// Prompt to execute.
    pub prompt: String,
    /// Whether this is a one-shot (non-recurring) job.
    pub is_one_shot: bool,
}

/// Background cron scheduler.
pub struct CronScheduler {
    store: CronJobStore,
    config: CronConfig,
    on_fire: Arc<dyn Fn(CronFireEvent) + Send + Sync>,
    on_disabled: Option<Arc<dyn Fn(String, i32) + Send + Sync>>,
    cancel: CancellationToken,
    cocode_home: Option<PathBuf>,
    /// Set of job IDs currently being fired (re-entrancy protection).
    firing_jobs: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl CronScheduler {
    /// Create a new scheduler.
    ///
    /// `on_fire` is invoked with a `CronFireEvent` when a job fires.
    pub fn new(
        store: CronJobStore,
        config: CronConfig,
        on_fire: Arc<dyn Fn(CronFireEvent) + Send + Sync>,
    ) -> Self {
        Self {
            store,
            config,
            on_fire,
            on_disabled: None,
            cancel: CancellationToken::new(),
            cocode_home: None,
            firing_jobs: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    /// Set the cocode home directory for durable persistence.
    pub fn with_cocode_home(mut self, path: PathBuf) -> Self {
        self.cocode_home = Some(path);
        self
    }

    /// Set a callback invoked when a job is disabled by the circuit breaker.
    /// Receives (job_id, consecutive_failures).
    pub fn with_on_disabled(mut self, f: Arc<dyn Fn(String, i32) + Send + Sync>) -> Self {
        self.on_disabled = Some(f);
        self
    }

    /// Spawn the background tick loop. Returns immediately.
    pub fn start(&self) {
        let store = Arc::clone(&self.store);
        let on_fire = Arc::clone(&self.on_fire);
        let on_disabled = self.on_disabled.clone();
        let cancel = self.cancel.clone();
        let cocode_home = self.cocode_home.clone();
        let config = self.config.clone();
        let firing_jobs = Arc::clone(&self.firing_jobs);

        tokio::spawn(async move {
            let tick = std::time::Duration::from_secs(config.tick_interval_secs.max(1) as u64);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(tick) => {}
                }

                let now = Local::now();
                let now_ts = now.timestamp();

                let had_changes = check_and_fire(
                    &store,
                    &on_fire,
                    on_disabled.as_deref(),
                    &firing_jobs,
                    &config,
                    now,
                    now_ts,
                )
                .await;

                // Persist durable jobs if any changes occurred
                if had_changes
                    && let Some(ref home) = cocode_home
                    && let Err(e) = save_durable_jobs(&store, home).await
                {
                    tracing::warn!(error = %e, "Failed to save durable cron jobs after tick");
                }
            }
        });
    }

    /// Cancel the background task.
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    /// Report the result of executing a cron-fired prompt.
    ///
    /// Called by the agent loop after processing a cron-triggered prompt.
    /// Updates the circuit breaker state.
    pub async fn report_execution_result(&self, job_id: &str, success: bool) {
        let mut guard = self.store.lock().await;
        if let Some(job) = guard.get_mut(job_id) {
            if success {
                job.consecutive_failures = 0;
            } else {
                job.consecutive_failures += 1;
                if job.consecutive_failures >= self.config.circuit_breaker_threshold {
                    job.status = CronJobStatus::Disabled;
                    tracing::warn!(
                        job_id,
                        failures = job.consecutive_failures,
                        "Cron job disabled by circuit breaker"
                    );
                    if let Some(ref on_disabled) = self.on_disabled {
                        on_disabled(job_id.to_string(), job.consecutive_failures);
                    }
                }
            }
        }
    }

    /// Get the cancellation token for external lifecycle management.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

/// Inspect every job, fire due ones, handle expiry and one-shot removal.
///
/// Returns `true` if any jobs were modified (removed, expired, or fired).
async fn check_and_fire(
    store: &CronJobStore,
    on_fire: &Arc<dyn Fn(CronFireEvent) + Send + Sync>,
    on_disabled: Option<&(dyn Fn(String, i32) + Send + Sync)>,
    firing_jobs: &Arc<Mutex<std::collections::HashSet<String>>>,
    config: &CronConfig,
    now: chrono::DateTime<Local>,
    now_ts: i64,
) -> bool {
    let mut guard = store.lock().await;

    let mut remove_ids: Vec<String> = Vec::new();
    let mut fire_events: Vec<CronFireEvent> = Vec::new();
    let mut had_changes = false;

    for (id, job) in guard.iter_mut() {
        if job.status != CronJobStatus::Active {
            continue;
        }

        // Auto-expiry check
        if let Some(expires_at) = job.expires_at {
            if now_ts >= expires_at {
                job.status = CronJobStatus::Expired;
                remove_ids.push(id.clone());
                tracing::info!(job_id = %id, "Cron job expired");
                continue;
            }
        } else if job.recurring && !job.durable {
            let age = now_ts.saturating_sub(job.created_at);
            if age > config.recurring_expiry_secs {
                job.status = CronJobStatus::Expired;
                remove_ids.push(id.clone());
                tracing::info!(job_id = %id, "Cron job expired (age-based)");
                continue;
            }
        }

        // Check if this job should fire
        let should_fire = if let Some(next_fire_at) = job.next_fire_at {
            // Use cached fire time for 1-second precision
            now_ts >= next_fire_at
        } else {
            // No cached time — check cron match
            matches_cron(&job.cron, &now)
        };

        if !should_fire {
            continue;
        }

        // Re-entrancy check
        {
            let firing = firing_jobs.lock().await;
            if firing.contains(id.as_str()) {
                continue;
            }
        }

        // Fire the job
        job.execution_count += 1;
        job.last_executed_at = Some(now_ts);
        had_changes = true;

        fire_events.push(CronFireEvent {
            job_id: id.clone(),
            prompt: job.prompt.clone(),
            is_one_shot: !job.recurring,
        });

        if !job.recurring {
            // One-shot: mark completed and remove
            job.status = CronJobStatus::Completed;
            remove_ids.push(id.clone());
        } else {
            // Recurring: compute next fire time with jitter
            let jitter = compute_recurring_jitter(&job.cron, id, &config.jitter);
            job.next_fire_at = compute_next_fire_time(&job.cron, now_ts, jitter);
        }
    }

    // Handle one-shot early-fire: for jobs not yet due, check if early fire applies
    for (id, job) in guard.iter_mut() {
        if job.status != CronJobStatus::Active || job.recurring {
            continue;
        }
        // Skip already-fired jobs
        if fire_events.iter().any(|e| e.job_id == *id) {
            continue;
        }
        if job.next_fire_at.is_none() {
            // Compute next fire time for one-shot with early-fire jitter
            if let Some(next) = compute_next_fire_time(&job.cron, now_ts, 0) {
                let scheduled_minute = chrono::DateTime::from_timestamp(next, 0)
                    .map(|dt| dt.minute() as i32)
                    .unwrap_or(0);
                let early = compute_one_shot_early_fire(scheduled_minute, id, &config.jitter);
                job.next_fire_at = Some(next - early);
            }
        }
    }

    // Remove completed/expired jobs
    if !remove_ids.is_empty() {
        had_changes = true;
    }
    for id in &remove_ids {
        guard.remove(id);
    }

    // Auto-disable check for jobs with too many failures
    for (id, job) in guard.iter_mut() {
        if job.status == CronJobStatus::Active
            && job.consecutive_failures >= config.circuit_breaker_threshold
        {
            job.status = CronJobStatus::Disabled;
            had_changes = true;
            tracing::warn!(job_id = %id, failures = job.consecutive_failures, "Circuit breaker triggered");
            if let Some(on_disabled) = on_disabled {
                on_disabled(id.clone(), job.consecutive_failures);
            }
        }
    }

    // Release the lock before invoking callbacks
    drop(guard);

    // Mark jobs as firing and invoke callbacks
    for event in &fire_events {
        let mut firing = firing_jobs.lock().await;
        firing.insert(event.job_id.clone());
    }

    for event in fire_events {
        let job_id = event.job_id.clone();
        on_fire(event);
        let mut firing = firing_jobs.lock().await;
        firing.remove(&job_id);
    }

    had_changes
}

/// Compute the next fire timestamp for a cron schedule, starting from `after_ts`.
///
/// Scans forward in 60-second increments (cron's minimum granularity) up to 48 hours.
fn compute_next_fire_time(schedule: &str, after_ts: i64, jitter_secs: i64) -> Option<i64> {
    for offset_mins in 1..=2880 {
        let candidate_ts = after_ts + offset_mins * 60;
        let candidate = chrono::DateTime::from_timestamp(candidate_ts, 0)?;
        if matches_cron(schedule, &candidate) {
            return Some(candidate_ts + jitter_secs);
        }
    }
    None
}

#[cfg(test)]
#[path = "scheduler.test.rs"]
mod tests;
