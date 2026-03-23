use super::*;

#[test]
fn test_default_config() {
    let config = CronConfig::default();
    assert_eq!(config.max_jobs, 50);
    assert_eq!(config.tick_interval_secs, 1);
    assert_eq!(config.recurring_expiry_secs, 259_200);
    assert_eq!(config.circuit_breaker_threshold, 3);
}

#[test]
fn test_default_jitter_config() {
    let jitter = JitterConfig::default();
    assert!((jitter.recurring_frac - 0.1).abs() < f64::EPSILON);
    assert_eq!(jitter.recurring_cap_secs, 900);
    assert_eq!(jitter.one_shot_max_secs, 90);
    assert_eq!(jitter.one_shot_floor_secs, 0);
    assert_eq!(jitter.one_shot_minute_mod, 30);
}

#[test]
fn test_config_serde_with_defaults() {
    let json = r#"{"max_jobs": 100}"#;
    let config: CronConfig = serde_json::from_str(json).expect("deserialize");
    assert_eq!(config.max_jobs, 100);
    assert_eq!(config.tick_interval_secs, 1); // default
    assert_eq!(config.circuit_breaker_threshold, 3); // default
}
