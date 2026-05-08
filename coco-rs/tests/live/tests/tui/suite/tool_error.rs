//! Tool failure path. Scripts a `Read` on a path that doesn't exist —
//! the engine dispatches the tool, the tool returns an error result,
//! and the agent loop *still* completes cleanly with a follow-up
//! assistant message. Verifies:
//!
//! - `AgentStreamEvent::ToolUseCompleted` carries `is_error = true`
//!   (`tool_completions()` surfaces it as `(name, true)`).
//! - The stream handler folds that into a `MessageContent::ToolError`
//!   chat entry — the user-visible "tool failed" line.
//! - The engine recovers: it re-enters the loop with the tool result
//!   and the next scripted reply lands as a normal assistant text.
//! - `SessionResult.is_error` stays `false` — a failed *tool* is data,
//!   not a session-level error.
//!
//! Mirrors the production path where a model retries / explains after a
//! file-not-found Read instead of crashing the turn.

use std::time::Duration;

use anyhow::Result;
use coco_tui::state::session::ChatRole;
use coco_tui::state::session::MessageContent;
use serde_json::json;

use crate::tui::harness::TuiHarness;
use crate::tui::scripted_model::Reply;

pub async fn run() -> Result<()> {
    // Pre-mint workdir so we can bake the absolute (non-existent) path
    // into the scripted Read input.
    let workdir = tempfile::Builder::new()
        .prefix("coco-tests-tui-err-")
        .tempdir_in("/tmp")?;
    let missing_path = workdir.path().join("does-not-exist.txt");
    let missing_str = missing_path.to_string_lossy().into_owned();

    let mut harness = TuiHarness::builder()
        .with_workdir(workdir)
        .with_replies([
            Reply::tool_call(
                "call_read_missing",
                "Read",
                json!({ "file_path": missing_str }),
            ),
            Reply::text("the file is gone — nothing to show"),
        ])
        .with_max_turns(6)
        .build()
        .await?;

    harness.submit("read the missing file").await;
    let ok = harness.pump_until_idle(Duration::from_secs(20)).await?;
    assert!(
        ok,
        "tool_error: SessionResult flagged is_error=true — a failed tool \
         shouldn't promote to a session-level error",
    );

    // Pre + post-tool turns: 2 model calls.
    assert_eq!(
        harness.model.call_count(),
        2,
        "tool_error: expected 2 LLM calls (pre + post tool), got {}",
        harness.model.call_count(),
    );

    // Engine surfaced the failure on the wire.
    let completions = harness.tool_completions();
    assert_eq!(
        completions,
        vec![("Read", true)],
        "tool_error: expected exactly one Read completion with is_error=true, \
         got {completions:?}",
    );

    // AppState surface: a `ToolError` ChatMessage for `Read` is folded in.
    let has_tool_error = harness.state.session.messages.iter().any(|m| {
        matches!(
            &m.content,
            MessageContent::ToolError { tool_name, error }
                if tool_name == "Read" && !error.is_empty()
        )
    });
    assert!(
        has_tool_error,
        "tool_error: missing MessageContent::ToolError(Read) in session.messages \
         (got {} messages: {:?})",
        harness.state.session.messages.len(),
        harness
            .state
            .session
            .messages
            .iter()
            .map(|m| (m.role, m.text_content().to_string()))
            .collect::<Vec<_>>(),
    );

    // Loop continued: the post-tool assistant text landed.
    let saw_recovery = harness.state.session.messages.iter().any(|m| {
        matches!(m.role, ChatRole::Assistant) && m.text_content().contains("the file is gone")
    });
    assert!(
        saw_recovery,
        "tool_error: post-tool assistant recovery message missing — \
         engine should have re-entered the loop after the tool error",
    );

    harness.shutdown().await;
    Ok(())
}
