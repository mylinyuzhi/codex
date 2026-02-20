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
fn test_cycle_thinking_level_with_supported_levels() {
    use cocode_protocol::model::ModelSpec;

    let mut state = AppState::new();

    // Set up a selection with supported levels: Low, Medium, High
    let selection = RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4"))
        .with_supported_thinking_levels(vec![
            ThinkingLevel::low(),
            ThinkingLevel::medium(),
            ThinkingLevel::high(),
        ]);
    state.session.current_selection = Some(selection);

    // Start at None (default)
    assert_eq!(
        state
            .session
            .current_selection
            .as_ref()
            .unwrap()
            .effective_thinking_level()
            .effort,
        ReasoningEffort::None
    );

    // Cycle: None → Low
    state.cycle_thinking_level();
    assert_eq!(
        state
            .session
            .current_selection
            .as_ref()
            .unwrap()
            .effective_thinking_level()
            .effort,
        ReasoningEffort::Low
    );

    // Cycle: Low → Medium
    state.cycle_thinking_level();
    assert_eq!(
        state
            .session
            .current_selection
            .as_ref()
            .unwrap()
            .effective_thinking_level()
            .effort,
        ReasoningEffort::Medium
    );

    // Cycle: Medium → High
    state.cycle_thinking_level();
    assert_eq!(
        state
            .session
            .current_selection
            .as_ref()
            .unwrap()
            .effective_thinking_level()
            .effort,
        ReasoningEffort::High
    );

    // Cycle: High → None (wraps around)
    state.cycle_thinking_level();
    assert_eq!(
        state
            .session
            .current_selection
            .as_ref()
            .unwrap()
            .effective_thinking_level()
            .effort,
        ReasoningEffort::None
    );
}

#[test]
fn test_cycle_thinking_level_no_supported_levels() {
    use cocode_protocol::model::ModelSpec;

    let mut state = AppState::new();

    // Set up a selection without supported_thinking_levels (fallback to full set)
    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    state.session.current_selection = Some(selection);

    // Cycle: None → Low
    state.cycle_thinking_level();
    assert_eq!(
        state
            .session
            .current_selection
            .as_ref()
            .unwrap()
            .effective_thinking_level()
            .effort,
        ReasoningEffort::Low
    );

    // Cycle: Low → Medium → High → XHigh
    state.cycle_thinking_level();
    state.cycle_thinking_level();
    state.cycle_thinking_level();
    assert_eq!(
        state
            .session
            .current_selection
            .as_ref()
            .unwrap()
            .effective_thinking_level()
            .effort,
        ReasoningEffort::XHigh
    );

    // Cycle: XHigh → None
    state.cycle_thinking_level();
    assert_eq!(
        state
            .session
            .current_selection
            .as_ref()
            .unwrap()
            .effective_thinking_level()
            .effort,
        ReasoningEffort::None
    );
}

#[test]
fn test_cycle_thinking_level_no_selection() {
    let mut state = AppState::new();
    assert!(state.session.current_selection.is_none());

    // Should not panic
    state.cycle_thinking_level();
    assert!(state.session.current_selection.is_none());
}

#[test]
fn test_quit() {
    let mut state = AppState::new();
    assert!(!state.should_exit());

    state.quit();
    assert!(state.should_exit());
}

#[test]
fn test_with_selection() {
    use cocode_protocol::model::ModelSpec;

    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let state = AppState::with_selection(selection);
    assert_eq!(
        state
            .session
            .current_selection
            .as_ref()
            .unwrap()
            .model
            .model,
        "gpt-5"
    );
}
