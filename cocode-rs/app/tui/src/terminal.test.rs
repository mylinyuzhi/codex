use super::*;

// Note: Most terminal tests require an actual terminal and can't run in CI.
// These tests focus on non-terminal-dependent functionality.

#[test]
fn test_app_state_default() {
    let state = AppState::new();
    assert!(!state.should_exit());
}
