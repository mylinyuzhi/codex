//! Core types for the cron scheduling system.

use serde::Deserialize;
use serde::Serialize;

/// A scheduled cron job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Unique job ID (e.g., "cron_a1b2c3d4").
    pub id: String,
    /// Standard 5-field cron expression (minute hour day-of-month month day-of-week).
    pub cron: String,
    /// The prompt or command to execute on each trigger.
    pub prompt: String,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether this job recurs (true) or is one-shot (false).
    #[serde(default = "default_recurring")]
    pub recurring: bool,
    /// Whether this job persists across sessions.
    #[serde(default)]
    pub durable: bool,
    /// Creation timestamp (Unix seconds).
    pub created_at: i64,
    /// Number of times this job has executed.
    #[serde(default)]
    pub execution_count: i32,
    /// Last execution timestamp (Unix seconds), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_executed_at: Option<i64>,
    /// Expiry timestamp (Unix seconds). Recurring jobs auto-expire after 3 days.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// Job status for completed/expired/disabled tracking.
    #[serde(default)]
    pub status: CronJobStatus,
    /// Consecutive execution failures (for circuit breaker).
    #[serde(default)]
    pub consecutive_failures: i32,
    /// Cached next fire time (Unix seconds, with jitter baked in).
    /// Used by the scheduler for 1-second tick comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at: Option<i64>,
}

fn default_recurring() -> bool {
    true
}

/// Status of a cron job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CronJobStatus {
    #[default]
    Active,
    Completed,
    Expired,
    /// Auto-disabled by the circuit breaker after consecutive failures.
    Disabled,
}

/// Generate a short unique cron job ID.
pub fn generate_cron_id() -> String {
    let uuid = uuid::Uuid::new_v4();
    format!("cron_{}", &uuid.to_string()[..8])
}

#[cfg(test)]
#[path = "types.test.rs"]
mod tests;
