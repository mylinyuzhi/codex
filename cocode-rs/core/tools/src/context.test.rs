use super::*;

#[test]
fn test_approval_store() {
    let mut store = ApprovalStore::new();

    assert!(!store.is_approved("Bash", "git status"));
    store.approve_pattern("Bash", "git status");
    assert!(store.is_approved("Bash", "git status"));

    store.approve_session("Read");
    assert!(store.is_approved("Read", "any_pattern"));
}

#[test]
fn test_approval_store_wildcard() {
    let mut store = ApprovalStore::new();

    // Prefix wildcard: "git *" matches "git push origin main"
    store.approve_pattern("Bash", "git *");
    assert!(store.is_approved("Bash", "git push origin main"));
    assert!(store.is_approved("Bash", "git status"));
    assert!(store.is_approved("Bash", "git"));
    assert!(!store.is_approved("Bash", "gitx"));
    assert!(!store.is_approved("Bash", "npm install"));

    // Different tool name should not match
    assert!(!store.is_approved("Edit", "git push"));

    // Glob wildcard: "npm*" matches "npm" and "npx"
    store.approve_pattern("Bash", "npm*");
    assert!(store.is_approved("Bash", "npm install"));
    assert!(store.is_approved("Bash", "npmrc"));
    assert!(!store.is_approved("Bash", "node index.js"));

    // Universal wildcard
    store.approve_pattern("Bash", "*");
    assert!(store.is_approved("Bash", "anything"));
}

#[test]
fn test_matches_wildcard() {
    // Universal
    assert!(ApprovalStore::matches_wildcard("*", "anything"));

    // Space-star prefix
    assert!(ApprovalStore::matches_wildcard("git *", "git push"));
    assert!(ApprovalStore::matches_wildcard("git *", "git"));
    assert!(!ApprovalStore::matches_wildcard("git *", "gitx"));

    // Trailing star (no space)
    assert!(ApprovalStore::matches_wildcard("git*", "git"));
    assert!(ApprovalStore::matches_wildcard("git*", "gitx"));
    assert!(ApprovalStore::matches_wildcard("git*", "git push"));

    // Exact match
    assert!(ApprovalStore::matches_wildcard("git status", "git status"));
    assert!(!ApprovalStore::matches_wildcard("git status", "git push"));
}

#[test]
fn test_file_tracker() {
    let mut tracker = FileTracker::new();

    let path = PathBuf::from("/test/file.txt");
    assert!(!tracker.was_read(&path));

    tracker.record_read(&path);
    assert!(tracker.was_read(&path));
    assert!(!tracker.was_modified(&path));

    tracker.record_modified(&path);
    assert!(tracker.was_modified(&path));
}

#[tokio::test]
async fn test_tool_context() {
    let ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    assert_eq!(ctx.call_id, "call-1");
    assert_eq!(ctx.session_id, "session-1");
    assert!(!ctx.is_cancelled());
}

#[test]
fn test_resolve_path() {
    let ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/home/user/project"));

    // Relative path
    assert_eq!(
        ctx.resolve_path("src/main.rs"),
        PathBuf::from("/home/user/project/src/main.rs")
    );

    // Absolute path
    assert_eq!(
        ctx.resolve_path("/etc/passwd"),
        PathBuf::from("/etc/passwd")
    );
}

#[tokio::test]
async fn test_context_builder() {
    let ctx = ToolContextBuilder::new("call-1", "session-1")
        .cwd("/tmp")
        .permission_mode(PermissionMode::Plan)
        .build();

    assert_eq!(ctx.cwd, PathBuf::from("/tmp"));
    assert_eq!(ctx.permission_mode, PermissionMode::Plan);
}
