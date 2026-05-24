use super::*;

#[test]
fn test_default_fallback_config() {
    let config = FallbackConfig::default();
    assert!(!config.enabled);
    assert!(config.fallback_models.is_empty());
    assert_eq!(config.max_retries, 3);
}

#[test]
fn test_should_fallback_disabled() {
    let config = FallbackConfig::default();
    let state = FallbackState::new("model-a".to_string());
    assert!(!state.should_fallback(&config));
}

#[test]
fn test_should_fallback_enabled_with_models() {
    let config = FallbackConfig {
        enabled: true,
        fallback_models: vec!["model-b".to_string()],
        max_retries: 3,
    };
    let state = FallbackState::new("model-a".to_string());
    assert!(state.should_fallback(&config));
}

#[test]
fn test_should_fallback_enabled_no_models() {
    let config = FallbackConfig {
        enabled: true,
        fallback_models: vec![],
        max_retries: 3,
    };
    let state = FallbackState::new("model-a".to_string());
    assert!(!state.should_fallback(&config));
}

#[test]
fn test_should_fallback_max_retries_reached() {
    let config = FallbackConfig {
        enabled: true,
        fallback_models: vec!["model-b".to_string()],
        max_retries: 1,
    };
    let mut state = FallbackState::new("model-a".to_string());
    state.record_fallback("model-b".to_string(), "error".to_string());
    assert!(!state.should_fallback(&config));
}

#[test]
fn test_next_model_sequence() {
    let config = FallbackConfig {
        enabled: true,
        fallback_models: vec!["model-b".to_string(), "model-c".to_string()],
        max_retries: 3,
    };
    let mut state = FallbackState::new("model-a".to_string());

    assert_eq!(state.next_model(&config), Some("model-b".to_string()));

    state.record_fallback("model-b".to_string(), "error 1".to_string());
    assert_eq!(state.next_model(&config), Some("model-c".to_string()));

    state.record_fallback("model-c".to_string(), "error 2".to_string());
    assert_eq!(state.next_model(&config), None);
}

#[test]
fn test_next_model_disabled() {
    let config = FallbackConfig::default();
    let state = FallbackState::new("model-a".to_string());
    assert_eq!(state.next_model(&config), None);
}

#[test]
fn test_record_fallback() {
    let mut state = FallbackState::new("model-a".to_string());
    assert_eq!(state.attempts, 0);
    assert!(state.history.is_empty());

    state.record_fallback("model-b".to_string(), "rate limited".to_string());

    assert_eq!(state.current_model, "model-b");
    assert_eq!(state.attempts, 1);
    assert_eq!(state.history.len(), 1);
    assert_eq!(state.history[0].from_model, "model-a");
    assert_eq!(state.history[0].to_model, "model-b");
    assert_eq!(state.history[0].reason, "rate limited");
}
