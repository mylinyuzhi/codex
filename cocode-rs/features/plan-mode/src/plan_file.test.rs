use super::*;
use crate::plan_slug::clear_slug_cache;

#[test]
fn test_get_plan_dir() {
    let dir = get_plan_dir();
    assert!(dir.ends_with("plans"));
    assert!(dir.to_string_lossy().contains(".cocode"));
}

#[test]
fn test_get_plan_file_path_main_agent() {
    clear_slug_cache();
    let path = get_plan_file_path("test-session", None);
    assert!(path.extension().unwrap_or_default() == "md");
    assert!(!path.to_string_lossy().contains("agent-"));
}

#[test]
fn test_get_plan_file_path_subagent() {
    clear_slug_cache();
    let path = get_plan_file_path("test-session", Some("explore-1"));
    assert!(path.extension().unwrap_or_default() == "md");
    assert!(path.to_string_lossy().contains("agent-explore-1"));
}

#[test]
fn test_is_plan_file() {
    let plan_path = PathBuf::from("/home/user/.cocode/plans/test-plan.md");
    let other_path = PathBuf::from("/home/user/project/src/main.rs");

    assert!(is_plan_file(&plan_path, &plan_path));
    assert!(!is_plan_file(&other_path, &plan_path));
}

#[test]
fn test_plan_file_manager() {
    clear_slug_cache();

    let manager = PlanFileManager::new("session-1");
    assert_eq!(manager.session_id(), "session-1");
    assert!(manager.agent_id().is_none());

    let path = manager.path();
    assert!(path.extension().unwrap_or_default() == "md");
}

#[test]
fn test_plan_file_manager_for_agent() {
    clear_slug_cache();

    let manager = PlanFileManager::for_agent("session-1", "explore");
    assert_eq!(manager.session_id(), "session-1");
    assert_eq!(manager.agent_id(), Some("explore"));

    let path = manager.path();
    assert!(path.to_string_lossy().contains("agent-explore"));
}
