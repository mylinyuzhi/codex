use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_session_state_default_has_valid_uuid() {
    let state = SessionState::default();
    // UUID v4 format: 8-4-4-4-12 hex chars
    assert_eq!(state.session_id.len(), 36);
    let parts: Vec<&str> = state.session_id.split('-').collect();
    assert_eq!(parts.len(), 5);
    assert_eq!(parts[0].len(), 8);
    assert_eq!(parts[1].len(), 4);
    assert_eq!(parts[2].len(), 4);
    assert_eq!(parts[3].len(), 4);
    assert_eq!(parts[4].len(), 12);
    // Version nibble should be '4'
    assert!(parts[2].starts_with('4'));
}

#[test]
fn test_session_state_regenerate_session_id() {
    let mut state = SessionState::default();
    let original_id = state.session_id.clone();

    let new_id = state.regenerate_session_id(/*set_current_as_parent*/ true);
    assert_ne!(new_id, original_id);
    assert_eq!(
        state.parent_session_id.as_deref(),
        Some(original_id.as_str())
    );
}

#[test]
fn test_session_state_regenerate_session_id_without_parent() {
    let mut state = SessionState::default();
    let original_id = state.session_id.clone();

    let new_id = state.regenerate_session_id(/*set_current_as_parent*/ false);
    assert_ne!(new_id, original_id);
    assert!(state.parent_session_id.is_none());
}

#[test]
fn test_session_state_cost_tracking() {
    let mut state = SessionState::default();
    assert_eq!(state.total_cost_usd, 0.0);
    assert_eq!(state.total_input_tokens(), 0);

    state.add_cost(
        0.05,
        "claude-sonnet-4",
        ModelUsageEntry {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_input_tokens: 200,
            cache_creation_input_tokens: 100,
            cost_usd: 0.05,
        },
    );

    assert_eq!(state.total_cost_usd, 0.05);
    assert_eq!(state.total_input_tokens(), 1000);
    assert_eq!(state.total_output_tokens(), 500);
}

#[test]
fn test_session_state_reset_cost_state() {
    let mut state = SessionState::default();
    state.add_cost(
        1.0,
        "test-model",
        ModelUsageEntry {
            input_tokens: 5000,
            output_tokens: 2000,
            ..Default::default()
        },
    );
    state.add_api_duration(1500.0);
    state.add_tool_duration(300.0);
    state.add_lines_changed(100, 50);

    state.reset_cost_state();

    assert_eq!(state.total_cost_usd, 0.0);
    assert_eq!(state.total_api_duration_ms, 0.0);
    assert_eq!(state.total_tool_duration_ms, 0.0);
    assert_eq!(state.total_lines_added, 0);
    assert_eq!(state.total_lines_removed, 0);
    assert!(state.model_usage.is_empty());
    assert_eq!(state.total_input_tokens(), 0);
}

#[test]
fn test_session_state_message_count() {
    let mut state = SessionState::default();
    assert_eq!(state.message_count, 0);

    state.increment_message_count();
    state.increment_message_count();
    state.increment_message_count();

    assert_eq!(state.message_count, 3);
}

#[test]
fn test_bootstrap_config_first_run() {
    let config = BootstrapConfig::default();
    assert!(config.is_first_run());
    assert!(config.needs_onboarding("1.0.0"));
}

#[test]
fn test_bootstrap_config_not_first_run_after_startup() {
    let mut config = BootstrapConfig::default();
    config.record_startup();

    assert!(!config.is_first_run());
    assert!(config.first_start_time.is_some());
    assert_eq!(config.num_startups, 1);
}

#[test]
fn test_bootstrap_config_onboarding_completed() {
    let mut config = BootstrapConfig::default();
    config.complete_onboarding("1.0.0");

    assert!(!config.needs_onboarding("1.0.0"));
    // Different version triggers re-onboarding
    assert!(config.needs_onboarding("2.0.0"));
}

#[test]
fn test_bootstrap_config_onboarding_no_version_reset() {
    let config = BootstrapConfig {
        has_completed_onboarding: true,
        last_onboarding_version: None,
        ..Default::default()
    };

    // No version recorded means no version-gated re-show
    assert!(!config.needs_onboarding("1.0.0"));
}

#[test]
fn test_uuid_v4_uniqueness() {
    let id1 = uuid_v4();
    let id2 = uuid_v4();
    assert_ne!(id1, id2);
}
