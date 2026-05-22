use super::*;
use cocode_protocol::ToolName;

#[test]
fn test_approval_store() {
    let mut store = ApprovalStore::new();

    assert!(!store.is_approved(ToolName::Bash.as_str(), "git status"));
    store.approve_pattern(ToolName::Bash.as_str(), "git status");
    assert!(store.is_approved(ToolName::Bash.as_str(), "git status"));

    store.approve_session(ToolName::Read.as_str());
    assert!(store.is_approved(ToolName::Read.as_str(), "any_pattern"));
}

#[test]
fn test_approval_store_wildcard() {
    let mut store = ApprovalStore::new();

    // Prefix wildcard: "git *" matches "git push origin main"
    store.approve_pattern(ToolName::Bash.as_str(), "git *");
    assert!(store.is_approved(ToolName::Bash.as_str(), "git push origin main"));
    assert!(store.is_approved(ToolName::Bash.as_str(), "git status"));
    assert!(store.is_approved(ToolName::Bash.as_str(), "git"));
    assert!(!store.is_approved(ToolName::Bash.as_str(), "gitx"));
    assert!(!store.is_approved(ToolName::Bash.as_str(), "npm install"));

    // Different tool name should not match
    assert!(!store.is_approved(ToolName::Edit.as_str(), "git push"));

    // Glob wildcard: "npm*" matches "npm" and "npx"
    store.approve_pattern(ToolName::Bash.as_str(), "npm*");
    assert!(store.is_approved(ToolName::Bash.as_str(), "npm install"));
    assert!(store.is_approved(ToolName::Bash.as_str(), "npmrc"));
    assert!(!store.is_approved(ToolName::Bash.as_str(), "node index.js"));

    // Universal wildcard
    store.approve_pattern(ToolName::Bash.as_str(), "*");
    assert!(store.is_approved(ToolName::Bash.as_str(), "anything"));
}

#[test]
fn test_file_tracker() {
    let tracker = FileTracker::new();

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

    assert_eq!(ctx.identity.call_id, "call-1");
    assert_eq!(ctx.identity.session_id, "session-1");
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

    assert_eq!(ctx.env.cwd, PathBuf::from("/tmp"));
    assert_eq!(ctx.env.permission_mode, PermissionMode::Plan);
}

#[test]
fn test_file_tracker_is_unchanged() {
    let tracker = FileTracker::new();

    // Create a temp file for testing
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("cocode_test_unchanged.txt");

    // Write initial content
    std::fs::write(&test_file, "initial content").unwrap();
    let metadata = std::fs::metadata(&test_file).unwrap();
    let mtime = metadata.modified().ok();

    // Track the file read
    let state = FileReadState::complete("initial content".to_string(), mtime);
    tracker.track_read(&test_file, state);

    // File should be unchanged immediately after reading
    assert_eq!(tracker.is_unchanged(&test_file), Some(true));

    // Modify the file (sleep briefly so mtime changes on low-resolution filesystems)
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(&test_file, "modified content").unwrap();

    // File should now show as changed (is_unchanged = false)
    assert_eq!(tracker.is_unchanged(&test_file), Some(false));

    // Cleanup
    let _ = std::fs::remove_file(&test_file);
}

#[test]
fn test_file_tracker_is_unchanged_partial_read() {
    let tracker = FileTracker::new();

    // Create a temp file for testing
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("cocode_test_partial.txt");

    std::fs::write(&test_file, "content").unwrap();

    // Track a partial read - partial reads should NOT be cacheable
    let state = FileReadState::partial(0, 10, None);
    tracker.track_read(&test_file, state);

    // Partial reads should return None for is_unchanged (not cacheable)
    // This ensures @mentioned files with partial reads are always re-read
    assert_eq!(tracker.is_unchanged(&test_file), None);

    // Cleanup
    let _ = std::fs::remove_file(&test_file);
}

#[test]
fn test_file_tracker_is_unchanged_untracked() {
    let tracker = FileTracker::new();

    // Untracked file should return None
    let path = PathBuf::from("/nonexistent/file.txt");
    assert_eq!(tracker.is_unchanged(&path), None);
}
