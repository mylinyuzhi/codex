use super::*;
use crate::config::JitterConfig;

#[test]
fn test_hash_job_id_range() {
    for i in 0..100 {
        let id = format!("cron_{i:08x}");
        let h = hash_job_id(&id);
        assert!((0.0..1.0).contains(&h), "Hash {h} out of [0, 1) for {id}");
    }
}

#[test]
fn test_hash_job_id_deterministic() {
    let h1 = hash_job_id("cron_abc");
    let h2 = hash_job_id("cron_abc");
    assert!((h1 - h2).abs() < f64::EPSILON);
}

#[test]
fn test_hash_different_ids_differ() {
    let h1 = hash_job_id("cron_aaa");
    let h2 = hash_job_id("cron_bbb");
    assert!((h1 - h2).abs() > f64::EPSILON);
}

#[test]
fn test_recurring_jitter_no_jitter_for_fast_jobs() {
    let config = JitterConfig::default();
    // Every minute job
    let jitter = compute_recurring_jitter("*/1 * * * *", "cron_test", &config);
    assert_eq!(jitter, 0, "No jitter for <=60s period");
}

#[test]
fn test_recurring_jitter_positive_for_slow_jobs() {
    let config = JitterConfig::default();
    // Every hour job (period ~3600s)
    let jitter = compute_recurring_jitter("0 * * * *", "cron_test1234", &config);
    assert!(jitter >= 0, "Jitter should be non-negative");
    assert!(
        jitter <= config.recurring_cap_secs,
        "Jitter should be capped"
    );
}

#[test]
fn test_recurring_jitter_capped() {
    let config = JitterConfig {
        recurring_cap_secs: 10,
        ..JitterConfig::default()
    };
    // Daily job
    let jitter = compute_recurring_jitter("0 0 * * *", "cron_test", &config);
    assert!(
        jitter <= 10,
        "Jitter should be capped at {}",
        config.recurring_cap_secs
    );
}

#[test]
fn test_one_shot_early_fire_at_zero_mark() {
    let config = JitterConfig::default();
    let early = compute_one_shot_early_fire(0, "cron_test", &config);
    assert!(early >= 0);
    assert!(early <= config.one_shot_max_secs);
}

#[test]
fn test_one_shot_early_fire_at_30_mark() {
    let config = JitterConfig::default();
    let early = compute_one_shot_early_fire(30, "cron_test", &config);
    assert!(early >= 0);
    assert!(early <= config.one_shot_max_secs);
}

#[test]
fn test_one_shot_no_early_fire_at_15() {
    let config = JitterConfig::default(); // one_shot_minute_mod = 30
    let early = compute_one_shot_early_fire(15, "cron_test", &config);
    assert_eq!(early, 0, "No early fire for :15 mark");
}

#[test]
fn test_estimate_period_every_5_min() {
    let period = estimate_period_secs("*/5 * * * *");
    assert_eq!(period, 300, "Every 5 minutes = 300 seconds");
}

#[test]
fn test_estimate_period_every_hour() {
    let period = estimate_period_secs("0 * * * *");
    assert_eq!(period, 3600, "Every hour = 3600 seconds");
}

#[test]
fn test_estimate_period_heuristic_fallback() {
    // Invalid cron: falls back to heuristic
    let period = estimate_period_heuristic("*/10 * * * *");
    assert_eq!(period, 600, "Heuristic: */10 = 10 * 60 = 600s");
}

#[test]
fn test_estimate_period_heuristic_daily() {
    let period = estimate_period_heuristic("30 9 * * *");
    assert_eq!(period, 86400, "Heuristic: specific hour+minute = daily");
}
