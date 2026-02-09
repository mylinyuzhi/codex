use super::*;

#[test]
fn test_plan_mode_state_lifecycle() {
    let mut state = PlanModeState::new();

    // Initial state
    assert!(!state.is_active);
    assert!(state.plan_file_path.is_none());
    assert!(!state.has_exited);

    // Enter plan mode
    let path = PathBuf::from("/home/user/.cocode/plans/test-plan.md");
    state.enter(path.clone(), "test-plan".to_string(), 1);

    assert!(state.is_active);
    assert_eq!(state.plan_file_path, Some(path.clone()));
    assert_eq!(state.plan_slug, Some("test-plan".to_string()));
    assert_eq!(state.entered_at_turn, Some(1));
    assert!(!state.is_reentry());

    // Exit plan mode
    state.exit(5);

    assert!(!state.is_active);
    assert!(state.has_exited);
    assert!(state.needs_exit_attachment);
    assert_eq!(state.exited_at_turn, Some(5));

    // Clear exit attachment
    state.clear_exit_attachment();
    assert!(!state.needs_exit_attachment);

    // Re-enter plan mode
    state.enter(path.clone(), "test-plan".to_string(), 6);
    assert!(state.is_active);
    assert!(state.is_reentry());
}

#[test]
fn test_get_plan_file() {
    let mut state = PlanModeState::new();
    let path = PathBuf::from("/home/user/.cocode/plans/test.md");

    // Not in plan mode
    assert!(state.get_plan_file().is_none());

    // In plan mode
    state.enter(path.clone(), "test".to_string(), 1);
    assert_eq!(state.get_plan_file(), Some(path.as_path()));

    // Exited plan mode
    state.exit(2);
    assert!(state.get_plan_file().is_none());
}

#[test]
fn test_is_safe_file() {
    let plan_path = PathBuf::from("/home/user/.cocode/plans/test-plan.md");
    let other_path = PathBuf::from("/home/user/project/src/main.rs");

    // No plan file set
    assert!(!is_safe_file(&plan_path, None));
    assert!(!is_safe_file(&other_path, None));

    // Plan file set
    assert!(is_safe_file(&plan_path, Some(&plan_path)));
    assert!(!is_safe_file(&other_path, Some(&plan_path)));
}
