//! Configuration for the cron scheduling system.

use serde::Deserialize;
use serde::Serialize;

/// Maximum number of active cron jobs allowed (default: 50).
pub const DEFAULT_MAX_JOBS: i32 = 50;

/// Default scheduler tick interval in seconds.
pub const DEFAULT_TICK_INTERVAL_SECS: i32 = 1;

/// Auto-expiry duration for recurring jobs (3 days in seconds).
pub const DEFAULT_RECURRING_EXPIRY_SECS: i64 = 3 * 24 * 60 * 60;

/// Default circuit breaker threshold (consecutive failures before auto-disable).
pub const DEFAULT_CIRCUIT_BREAKER_THRESHOLD: i32 = 3;

/// Configuration for the cron scheduling system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronConfig {
    /// Maximum number of active cron jobs.
    #[serde(default = "default_max_jobs")]
    pub max_jobs: i32,
    /// Scheduler tick interval in seconds.
    #[serde(default = "default_tick_interval_secs")]
    pub tick_interval_secs: i32,
    /// Recurring job auto-expiry in seconds.
    #[serde(default = "default_recurring_expiry_secs")]
    pub recurring_expiry_secs: i64,
    /// Consecutive failures before auto-disabling a job.
    #[serde(default = "default_circuit_breaker_threshold")]
    pub circuit_breaker_threshold: i32,
    /// Jitter configuration.
    #[serde(default)]
    pub jitter: JitterConfig,
}

impl Default for CronConfig {
    fn default() -> Self {
        Self {
            max_jobs: DEFAULT_MAX_JOBS,
            tick_interval_secs: DEFAULT_TICK_INTERVAL_SECS,
            recurring_expiry_secs: DEFAULT_RECURRING_EXPIRY_SECS,
            circuit_breaker_threshold: DEFAULT_CIRCUIT_BREAKER_THRESHOLD,
            jitter: JitterConfig::default(),
        }
    }
}

fn default_max_jobs() -> i32 {
    DEFAULT_MAX_JOBS
}
fn default_tick_interval_secs() -> i32 {
    DEFAULT_TICK_INTERVAL_SECS
}
fn default_recurring_expiry_secs() -> i64 {
    DEFAULT_RECURRING_EXPIRY_SECS
}
fn default_circuit_breaker_threshold() -> i32 {
    DEFAULT_CIRCUIT_BREAKER_THRESHOLD
}

/// Jitter configuration for spreading scheduled task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JitterConfig {
    /// Fraction of the job's period used as jitter for recurring jobs (default: 0.1 = 10%).
    #[serde(default = "default_recurring_frac")]
    pub recurring_frac: f64,
    /// Maximum jitter cap for recurring jobs in seconds (default: 900 = 15 minutes).
    #[serde(default = "default_recurring_cap_secs")]
    pub recurring_cap_secs: i64,
    /// Maximum early-fire seconds for one-shot jobs at :00/:30 marks (default: 90).
    #[serde(default = "default_one_shot_max_secs")]
    pub one_shot_max_secs: i64,
    /// Minimum early-fire seconds for one-shot jobs (default: 0).
    #[serde(default)]
    pub one_shot_floor_secs: i64,
    /// Minute modulus for one-shot jitter (default: 30, applies to :00 and :30).
    #[serde(default = "default_one_shot_minute_mod")]
    pub one_shot_minute_mod: i32,
}

impl Default for JitterConfig {
    fn default() -> Self {
        Self {
            recurring_frac: 0.1,
            recurring_cap_secs: 900,
            one_shot_max_secs: 90,
            one_shot_floor_secs: 0,
            one_shot_minute_mod: 30,
        }
    }
}

fn default_recurring_frac() -> f64 {
    0.1
}
fn default_recurring_cap_secs() -> i64 {
    900
}
fn default_one_shot_max_secs() -> i64 {
    90
}
fn default_one_shot_minute_mod() -> i32 {
    30
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
