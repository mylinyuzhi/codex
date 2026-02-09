use super::*;

#[test]
fn test_app_state_new() {
    let state = AppState::new();
    assert!(!state.should_exit());
    assert!(!state.session.plan_mode);
    assert!(!state.has_overlay());
}

#[test]
fn test_toggle_plan_mode() {
    let mut state = AppState::new();
    assert!(!state.session.plan_mode);

    state.toggle_plan_mode();
    assert!(state.session.plan_mode);

    state.toggle_plan_mode();
    assert!(!state.session.plan_mode);
}

#[test]
fn test_cycle_thinking_level() {
    let mut state = AppState::new();

    // Start at None
    assert_eq!(state.session.thinking_level.effort, ReasoningEffort::None);

    // Cycle through levels
    state.cycle_thinking_level();
    assert_eq!(state.session.thinking_level.effort, ReasoningEffort::Low);

    state.cycle_thinking_level();
    assert_eq!(state.session.thinking_level.effort, ReasoningEffort::Medium);

    state.cycle_thinking_level();
    assert_eq!(state.session.thinking_level.effort, ReasoningEffort::High);

    state.cycle_thinking_level();
    assert_eq!(state.session.thinking_level.effort, ReasoningEffort::XHigh);

    state.cycle_thinking_level();
    assert_eq!(state.session.thinking_level.effort, ReasoningEffort::None);
}

#[test]
fn test_quit() {
    let mut state = AppState::new();
    assert!(!state.should_exit());

    state.quit();
    assert!(state.should_exit());
}

#[test]
fn test_with_model() {
    let state = AppState::with_model("gpt-4");
    assert_eq!(state.session.current_model, "gpt-4");
}
