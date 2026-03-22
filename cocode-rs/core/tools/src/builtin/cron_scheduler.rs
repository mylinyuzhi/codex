//! Background cron scheduler that periodically checks and fires due jobs.

use super::cron_state::CronJob;
use super::cron_state::CronJobStatus;
use super::cron_state::CronJobStore;
use super::cron_state::RECURRING_EXPIRY_SECS;
use chrono::Datelike;
use chrono::Local;
use chrono::Timelike;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Maximum jitter cap for recurring jobs: 15 minutes in seconds.
const MAX_JITTER_SECS: u64 = 900;

/// Background cron scheduler that checks jobs every 60 seconds.
pub struct CronScheduler {
    store: CronJobStore,
    on_fire: Arc<dyn Fn(String) + Send + Sync>,
    cancel: CancellationToken,
    cocode_home: Option<PathBuf>,
}

impl CronScheduler {
    /// Create a new scheduler.
    ///
    /// `store` is the shared cron job store.
    /// `on_fire` is invoked with the job's prompt when a job fires.
    pub fn new(store: CronJobStore, on_fire: Arc<dyn Fn(String) + Send + Sync>) -> Self {
        Self {
            store,
            on_fire,
            cancel: CancellationToken::new(),
            cocode_home: None,
        }
    }

    /// Set the cocode home directory for durable persistence.
    pub fn with_cocode_home(mut self, path: PathBuf) -> Self {
        self.cocode_home = Some(path);
        self
    }

    /// Spawn the background tick loop. Returns immediately.
    pub fn start(&self) {
        let store = Arc::clone(&self.store);
        let on_fire = Arc::clone(&self.on_fire);
        let cancel = self.cancel.clone();
        let cocode_home = self.cocode_home.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {}
                }
                let now = Local::now();
                let had_removals = Self::check_and_fire(&store, &on_fire, now).await;
                // Persist durable jobs if any one-shot jobs were removed
                if had_removals
                    && let Some(ref home) = cocode_home
                    && let Err(e) = super::cron_state::save_durable_jobs(&store, home).await
                {
                    tracing::warn!(error = %e, "Failed to save durable cron jobs after scheduler tick");
                }
            }
        });
    }

    /// Cancel the background task.
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    /// Inspect every job in the store, fire those whose schedule matches
    /// the current time, handle one-shot removal and auto-expiry.
    ///
    /// Returns `true` if any jobs were removed (one-shot completed or expired).
    async fn check_and_fire(
        store: &Arc<Mutex<BTreeMap<String, CronJob>>>,
        on_fire: &Arc<dyn Fn(String) + Send + Sync>,
        now: chrono::DateTime<Local>,
    ) -> bool {
        let now_ts = now.timestamp();

        let mut guard = store.lock().await;

        // Collect IDs to remove after iteration.
        let mut remove_ids: Vec<String> = Vec::new();
        // Collect prompts to fire after releasing the lock.
        let mut fire_prompts: Vec<String> = Vec::new();

        for (id, job) in guard.iter_mut() {
            // Skip jobs that are already completed or expired.
            if job.status != CronJobStatus::Active {
                continue;
            }

            // Auto-expiry: check `expires_at` or fall back to age-based
            // expiry for non-durable recurring jobs (3 days).
            if let Some(expires_at) = job.expires_at {
                if now_ts >= expires_at {
                    job.status = CronJobStatus::Expired;
                    remove_ids.push(id.clone());
                    continue;
                }
            } else if job.recurring && !job.durable {
                let age = now_ts.saturating_sub(job.created_at);
                if age > RECURRING_EXPIRY_SECS {
                    job.status = CronJobStatus::Expired;
                    remove_ids.push(id.clone());
                    continue;
                }
            }

            if !matches_cron(&job.cron, &now) {
                continue;
            }

            // Jitter: for recurring jobs, apply random delay check.
            if job.recurring && should_skip_for_jitter(&job.cron, &job.id) {
                continue;
            }

            // Fire the job.
            job.execution_count += 1;
            job.last_executed_at = Some(now_ts);
            fire_prompts.push(job.prompt.clone());

            // One-shot jobs (non-recurring) are removed after execution.
            if !job.recurring {
                job.status = CronJobStatus::Completed;
                remove_ids.push(id.clone());
            }
        }

        let had_removals = !remove_ids.is_empty();
        for id in &remove_ids {
            guard.remove(id);
        }

        // Release the lock before invoking callbacks.
        drop(guard);

        for prompt in fire_prompts {
            on_fire(prompt);
        }

        had_removals
    }
}

/// Check whether a 5-field cron expression matches the given time.
///
/// Fields: minute hour day-of-month month day-of-week
/// Supports: `*`, exact numbers, comma-separated lists, ranges (`1-5`),
/// and step values (`*/10`, `1-30/5`).
fn matches_cron<Tz: chrono::TimeZone>(schedule: &str, now: &chrono::DateTime<Tz>) -> bool {
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }

    let minute = now.minute();
    let hour = now.hour();
    let day = now.day();
    let month = now.month();
    // chrono: Monday=1 .. Sunday=7; cron: Sunday=0, Monday=1 .. Saturday=6
    let weekday = now.weekday().num_days_from_sunday();

    field_matches(fields[0], minute, 0, 59)
        && field_matches(fields[1], hour, 0, 23)
        && field_matches(fields[2], day, 1, 31)
        && field_matches(fields[3], month, 1, 12)
        && field_matches(fields[4], weekday, 0, 6)
}

/// Check whether a single cron field matches the given value.
///
/// Handles: `*`, `*/step`, `num`, `min-max`, `min-max/step`, and
/// comma-separated combinations of the above.
fn field_matches(field: &str, value: u32, min: u32, max: u32) -> bool {
    for part in field.split(',') {
        if part_matches(part.trim(), value, min, max) {
            return true;
        }
    }
    false
}

/// Match a single comma-element of a cron field.
fn part_matches(part: &str, value: u32, min: u32, max: u32) -> bool {
    // Split on '/' for step values.
    let (range_part, step) = if let Some((r, s)) = part.split_once('/') {
        let step_val: u32 = match s.parse() {
            Ok(v) if v > 0 => v,
            _ => return false,
        };
        (r, Some(step_val))
    } else {
        (part, None)
    };

    // Determine the range of values this part covers.
    let (range_min, range_max) = if range_part == "*" || range_part == "?" {
        (min, max)
    } else if let Some((lo, hi)) = range_part.split_once('-') {
        let lo_val: u32 = match lo.parse() {
            Ok(v) => v,
            _ => return false,
        };
        let hi_val: u32 = match hi.parse() {
            Ok(v) => v,
            _ => return false,
        };
        (lo_val, hi_val)
    } else {
        // Exact number.
        let exact: u32 = match range_part.parse() {
            Ok(v) => v,
            _ => return false,
        };
        if let Some(s) = step {
            // e.g. "5/10" means starting at 5, every 10
            return value >= exact && (value - exact).is_multiple_of(s);
        }
        return value == exact;
    };

    // Check value is within range.
    if value < range_min || value > range_max {
        return false;
    }

    // Apply step if present.
    match step {
        Some(s) => (value - range_min).is_multiple_of(s),
        None => true,
    }
}

/// Determine whether this tick should be skipped due to jitter.
///
/// For recurring jobs we add random delay up to 10% of the implied period
/// (capped at [`MAX_JITTER_SECS`] = 15 minutes).  Since the scheduler
/// ticks every 60 seconds, we model the jitter probabilistically: compute
/// the jitter window in ticks, then randomly skip with appropriate
/// probability so the average delay equals half the jitter window.
fn should_skip_for_jitter(schedule: &str, job_id: &str) -> bool {
    let period = estimate_period_secs(schedule);
    if period <= 60 {
        // No jitter for very frequent jobs.
        return false;
    }

    let jitter_secs = std::cmp::min(period / 10, MAX_JITTER_SECS);
    if jitter_secs < 60 {
        return false;
    }

    // Use a cheap pseudo-random source (no external crate required).
    // Mix job_id into the seed so jobs on the same tick don't all
    // skip/fire together (thundering herd).
    let seed = {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let job_hash = job_id
            .bytes()
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        ((t ^ (t >> 17) ^ (t >> 31)) as u64) ^ job_hash
    };
    let jitter_ticks = jitter_secs / 60;
    // Skip with probability (jitter_ticks - 1) / jitter_ticks.
    // On the first matching tick we always fire (seed % jitter_ticks == 0
    // with probability 1/jitter_ticks), giving an average delay of
    // ~half the jitter window.
    seed % jitter_ticks != 0
}

/// Estimate the rough period in seconds implied by a cron schedule.
///
/// Uses the cron matcher to compute the actual difference between the
/// next two matching times, falling back to a heuristic for edge cases.
fn estimate_period_secs(schedule: &str) -> u64 {
    // Try to find two consecutive matching times
    let now = Local::now();
    let mut first_match = None;
    let mut second_match = None;

    // Scan forward up to 48 hours in 1-minute increments
    for offset_mins in 1..=2880 {
        let candidate = now + chrono::Duration::minutes(offset_mins);
        if matches_cron(schedule, &candidate) {
            if first_match.is_none() {
                first_match = Some(candidate);
            } else {
                second_match = Some(candidate);
                break;
            }
        }
    }

    match (first_match, second_match) {
        (Some(first), Some(second)) => (second - first).num_seconds().unsigned_abs(),
        (Some(first), None) => {
            // Only found one match in 48h — likely daily or less frequent
            // Use the offset from now as a rough estimate
            (first - now).num_seconds().unsigned_abs().max(86400)
        }
        _ => {
            // Fallback heuristic
            estimate_period_heuristic(schedule)
        }
    }
}

/// Fallback heuristic for period estimation when cron scanning fails.
fn estimate_period_heuristic(schedule: &str) -> u64 {
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    if fields.len() != 5 {
        return 60;
    }

    // Check minute field for step patterns like */N
    if let Some(step) = extract_step(fields[0]) {
        return u64::from(step) * 60;
    }

    // If minute is exact but hour is *, period is ~60 minutes
    if fields[0] != "*" && fields[1] == "*" {
        return 3600;
    }

    // If both minute and hour are exact, roughly daily
    if fields[0] != "*" && fields[1] != "*" {
        return 86400;
    }

    // Default: assume every minute
    60
}

/// Extract the step value from a cron field like `*/N` or `0-59/N`.
fn extract_step(field: &str) -> Option<u32> {
    let (_base, step_str) = field.split_once('/')?;
    step_str.parse().ok()
}
