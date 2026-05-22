//! Jitter calculations for spreading scheduled task execution.
//!
//! Prevents thundering herd by adding deterministic-random delay based on
//! the job ID hash. Supports both recurring jitter (delay after match) and
//! one-shot early-fire jitter (fire before :00/:30 marks).

use chrono::Local;

use crate::config::JitterConfig;
use crate::matcher::matches_cron;

/// Compute the jitter offset in seconds for a recurring job.
///
/// The jitter is a fraction of the job's estimated period, capped at
/// `config.recurring_cap_secs`. The hash of the job ID provides a
/// deterministic-random offset so different jobs don't cluster.
pub fn compute_recurring_jitter(schedule: &str, job_id: &str, config: &JitterConfig) -> i64 {
    let period = estimate_period_secs(schedule);
    if period <= 60 {
        return 0; // No jitter for very frequent jobs
    }

    let hash = hash_job_id(job_id);
    let jitter_secs =
        ((hash * config.recurring_frac * period as f64) as i64).min(config.recurring_cap_secs);

    if jitter_secs < 1 {
        return 0;
    }

    jitter_secs
}

/// Compute the early-fire offset in seconds for a one-shot job.
///
/// For one-shot jobs scheduled at `:00` or `:30` minute marks, fires
/// up to `config.one_shot_max_secs` early to spread load.
pub fn compute_one_shot_early_fire(
    scheduled_minute: i32,
    job_id: &str,
    config: &JitterConfig,
) -> i64 {
    if config.one_shot_minute_mod <= 0 {
        return 0;
    }

    // Only apply to :00, :30 (or whatever modulus is configured)
    if scheduled_minute % config.one_shot_minute_mod != 0 {
        return 0;
    }

    let hash = hash_job_id(job_id);
    let range = config.one_shot_max_secs - config.one_shot_floor_secs;
    if range <= 0 {
        return 0;
    }

    config.one_shot_floor_secs + (hash * range as f64) as i64
}

/// Hash a job ID to a float in [0, 1) for deterministic jitter.
fn hash_job_id(job_id: &str) -> f64 {
    let hash = job_id
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    // Mix the bits
    let mixed = hash ^ (hash >> 17) ^ (hash >> 31);
    (mixed & 0xFFFF_FFFF) as f64 / 4_294_967_296.0
}

/// Estimate the rough period in seconds implied by a cron schedule.
///
/// Uses the cron matcher to find two consecutive matching times.
pub fn estimate_period_secs(schedule: &str) -> u64 {
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
            (first - now).num_seconds().unsigned_abs().max(86400)
        }
        _ => estimate_period_heuristic(schedule),
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

#[cfg(test)]
#[path = "jitter.test.rs"]
mod tests;
