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
    state.enter(path, "test-plan".to_string(), 6);
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

#[test]
fn test_is_safe_file_with_symlink() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let real_file = dir.path().join("plan.md");
    std::fs::write(&real_file, "# Plan").unwrap();

    let link = dir.path().join("plan-link.md");
    symlink(&real_file, &link).unwrap();

    // Symlinked path should match the real path
    assert!(is_safe_file(&link, Some(&real_file)));
    assert!(is_safe_file(&real_file, Some(&link)));
}

#[test]
fn test_is_safe_file_with_dot_dot_components() {
    let dir = tempfile::tempdir().unwrap();
    let plans_dir = dir.path().join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();

    let plan_file = plans_dir.join("plan.md");
    std::fs::write(&plan_file, "# Plan").unwrap();

    // Path with .. that resolves to the plan file
    let traversal_path = dir
        .path()
        .join("plans")
        .join("..")
        .join("plans")
        .join("plan.md");
    assert!(is_safe_file(&traversal_path, Some(&plan_file)));

    // Path with .. that resolves to a different file
    let other_dir = dir.path().join("other");
    std::fs::create_dir_all(&other_dir).unwrap();
    let other_file = other_dir.join("file.md");
    std::fs::write(&other_file, "other").unwrap();

    let traversal_other = dir
        .path()
        .join("plans")
        .join("..")
        .join("other")
        .join("file.md");
    assert!(!is_safe_file(&traversal_other, Some(&plan_file)));
}

#[test]
fn test_is_safe_file_nonexistent_files_direct_comparison() {
    // When neither file exists, canonicalize fails and falls back to direct comparison
    let plan_path = PathBuf::from("/nonexistent/dir/plan.md");
    let same_path = PathBuf::from("/nonexistent/dir/plan.md");
    let different_path = PathBuf::from("/nonexistent/dir/other.md");

    assert!(is_safe_file(&same_path, Some(&plan_path)));
    assert!(!is_safe_file(&different_path, Some(&plan_path)));
}
