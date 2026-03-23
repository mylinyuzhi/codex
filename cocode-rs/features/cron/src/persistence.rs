//! Durable cron job persistence and missed one-shot detection.

use std::collections::BTreeMap;
use std::path::Path;

use crate::config::DEFAULT_RECURRING_EXPIRY_SECS;
use crate::store::CronJobStore;
use crate::types::CronJob;
use crate::types::CronJobStatus;

/// File name for durable cron persistence.
const SCHEDULED_TASKS_FILE: &str = "scheduled_tasks.json";

/// A one-shot task that was missed during downtime.
#[derive(Debug, Clone)]
pub struct MissedTask {
    /// Job ID.
    pub id: String,
    /// The prompt that was scheduled.
    pub prompt: String,
    /// Cron expression.
    pub cron: String,
    /// Creation timestamp (Unix seconds).
    pub created_at: i64,
}

/// Save durable cron jobs to `{cocode_home}/scheduled_tasks.json`.
///
/// Filters the store to `durable == true && status == Active`, serializes to JSON,
/// and writes atomically (write to `.tmp` then rename).
pub async fn save_durable_jobs(store: &CronJobStore, cocode_home: &Path) -> std::io::Result<()> {
    let guard = store.lock().await;
    let durable: BTreeMap<String, CronJob> = guard
        .iter()
        .filter(|(_, j)| j.durable && j.status == CronJobStatus::Active)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    drop(guard);

    let json = serde_json::to_string_pretty(&durable)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let target = cocode_home.join(SCHEDULED_TASKS_FILE);
    let tmp = cocode_home.join(format!("{SCHEDULED_TASKS_FILE}.tmp"));

    tokio::fs::write(&tmp, json.as_bytes()).await?;
    tokio::fs::rename(&tmp, &target).await?;
    Ok(())
}

/// Load durable cron jobs from `{cocode_home}/scheduled_tasks.json`.
///
/// Recalculates `expires_at` for jobs whose expiry passed during downtime.
/// Truly expired jobs (past `RECURRING_EXPIRY_SECS` from creation) are skipped.
pub async fn load_durable_jobs(cocode_home: &Path) -> std::io::Result<BTreeMap<String, CronJob>> {
    let path = cocode_home.join(SCHEDULED_TASKS_FILE);
    let data = tokio::fs::read_to_string(&path).await?;
    let mut jobs: BTreeMap<String, CronJob> = serde_json::from_str(&data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let now = now_unix_secs();

    // Recalculate expiry for jobs that expired during downtime
    jobs.retain(|_, job| {
        if job.status != CronJobStatus::Active {
            return false;
        }
        // Check if the job is truly expired (past RECURRING_EXPIRY_SECS from creation)
        let age = now.saturating_sub(job.created_at);
        if age > DEFAULT_RECURRING_EXPIRY_SECS {
            return false;
        }
        // Recalculate expires_at from remaining lifetime
        if job.recurring {
            let remaining = DEFAULT_RECURRING_EXPIRY_SECS.saturating_sub(age);
            job.expires_at = Some(now + remaining);
        }
        true
    });

    Ok(jobs)
}

/// Detect one-shot tasks whose scheduled time has passed during downtime.
///
/// Returns missed tasks for notification. These should NOT be auto-executed;
/// the user should be asked first.
pub fn detect_missed_oneshots(jobs: &BTreeMap<String, CronJob>, now: i64) -> Vec<MissedTask> {
    jobs.values()
        .filter(|job| !job.recurring && job.status == CronJobStatus::Active && job.created_at < now)
        .map(|job| MissedTask {
            id: job.id.clone(),
            prompt: job.prompt.clone(),
            cron: job.cron.clone(),
            created_at: job.created_at,
        })
        .collect()
}

/// Format missed tasks as a notification message for system reminder injection.
pub fn format_missed_tasks_message(tasks: &[MissedTask]) -> String {
    if tasks.is_empty() {
        return String::new();
    }

    let mut msg = String::from(
        "The following one-shot scheduled tasks were missed while the session was not running.\n\
         They have been removed from scheduled_tasks.json.\n\
         Do NOT execute these prompts. First use AskUserQuestion to ask the user.\n\n",
    );

    for task in tasks {
        msg.push_str(&format!(
            "- ID: {}, Schedule: [{}], Created: {}\n  Prompt: {}\n",
            task.id, task.cron, task.created_at, task.prompt,
        ));
    }

    msg
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "persistence.test.rs"]
mod tests;
