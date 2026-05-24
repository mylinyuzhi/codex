//! Tests for ExitWorktreeTool. These focus on input validation and
//! restoration-target resolution paths that don't require a real git
//! worktree. The happy-path end-to-end test would need `git worktree
//! add`/`remove` against a real repo, which is out of scope for unit
//! tests — integration coverage lives separately.

use super::EnterWorktreeTool;
use super::ExitWorktreeTool;
use coco_tool_runtime::DynTool;
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
    let result = <ExitWorktreeTool as DynTool>::execute(&ExitWorktreeTool, json!({}), &ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("path"), "should mention path: {err}");
}

#[tokio::test]
async fn test_exit_worktree_rejects_empty_path() {
    let ctx = ToolUseContext::test_default();
    let result =
        <ExitWorktreeTool as DynTool>::execute(&ExitWorktreeTool, json!({"path": ""}), &ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_exit_worktree_rejects_whitespace_path() {
    let ctx = ToolUseContext::test_default();
    let result =
        <ExitWorktreeTool as DynTool>::execute(&ExitWorktreeTool, json!({"path": "   "}), &ctx)
            .await;
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
    let result = <ExitWorktreeTool as DynTool>::execute(
        &ExitWorktreeTool,
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
    let schema = <ExitWorktreeTool as DynTool>::input_schema(&ExitWorktreeTool);
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

// ---------------------------------------------------------------------------
// render_for_model — both worktree tools surface the prebuilt `message` field
// ---------------------------------------------------------------------------

#[test]
fn enter_worktree_render_emits_message_text_only() {
    use coco_tool_runtime::ToolResultContentPart;
    use serde_json::json;
    let data = json!({
        "message": "Created worktree at '/tmp/wt' on branch 'feat/x'",
        "path": "/tmp/wt",
        "branch": "feat/x",
    });
    let parts = <EnterWorktreeTool as DynTool>::render_for_model(&EnterWorktreeTool, &data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    assert_eq!(text, "Created worktree at '/tmp/wt' on branch 'feat/x'");
}

#[test]
fn exit_worktree_render_emits_message_text_only() {
    use coco_tool_runtime::ToolResultContentPart;
    use serde_json::json;
    let data = json!({
        "message": "Removed worktree at '/tmp/wt'",
        "path": "/tmp/wt",
        "restoration": {"cwd_restored": true},
    });
    let parts = <ExitWorktreeTool as DynTool>::render_for_model(&ExitWorktreeTool, &data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    assert_eq!(text, "Removed worktree at '/tmp/wt'");
}
