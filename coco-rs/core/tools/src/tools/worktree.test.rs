//! Tests for ExitWorktreeTool. These focus on input validation and
//! restoration-target resolution paths that don't require a real git
//! worktree. The happy-path end-to-end test would need `git worktree
//! add`/`remove` against a real repo, which is out of scope for unit
//! tests — integration coverage lives separately.

use super::ExitWorktreeTool;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolUseContext;
use serde_json::json;

// ---------------------------------------------------------------------------
// Input validation
// ---------------------------------------------------------------------------

/// Missing `path` parameter must fail with InvalidInput before we even
/// try to invoke git.
#[tokio::test]
async fn test_exit_worktree_rejects_missing_path() {
    let ctx = ToolUseContext::test_default();
    let result = ExitWorktreeTool.execute(json!({}), &ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("path"), "should mention path: {err}");
}

#[tokio::test]
async fn test_exit_worktree_rejects_empty_path() {
    let ctx = ToolUseContext::test_default();
    let result = ExitWorktreeTool.execute(json!({"path": ""}), &ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_exit_worktree_rejects_whitespace_path() {
    let ctx = ToolUseContext::test_default();
    let result = ExitWorktreeTool.execute(json!({"path": "   "}), &ctx).await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Restoration target resolution
// ---------------------------------------------------------------------------
//
// The restoration target is picked in priority order (explicit >
// parent-dir > current-cwd). We can exercise the non-existent-path
// branch which hits git worktree remove's failure path and verify our
// error-handling returns ToolError::ExecutionFailed instead of panicking.

#[tokio::test]
async fn test_exit_worktree_nonexistent_path_fails_gracefully() {
    let ctx = ToolUseContext::test_default();
    let result = ExitWorktreeTool
        .execute(
            json!({
                "path": "/nonexistent/worktree/that/does/not/exist/xyz-12345",
                "previous_cwd": "/tmp"
            }),
            &ctx,
        )
        .await;
    // Either git is not installed (InstalledError path) or git reports
    // failure — both land in ExecutionFailed with a non-empty message.
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        !err.is_empty() && !err.contains("panic"),
        "error should be structured, not a panic: {err}"
    );
}

/// The schema must document `previous_cwd` so the model knows how to
/// invoke the tool. TS contract check.
#[test]
fn test_exit_worktree_schema_advertises_previous_cwd() {
    let schema = ExitWorktreeTool.input_schema();
    assert!(
        schema.properties.contains_key("previous_cwd"),
        "schema must expose previous_cwd parameter"
    );
    assert!(
        schema.properties.contains_key("path"),
        "schema must expose path parameter"
    );
    assert!(
        schema.properties.contains_key("force"),
        "schema must expose force parameter"
    );
}
