//! Convenient hook trigger functions for core integration

use codex_hooks::trigger_hook;
use codex_protocol::hooks::{HookEventContext, HookEventData, HookEventName};
use crate::error::{CodexErr, Result as CodexResult};

/// Trigger PreToolUse hook
///
/// This should be called before executing a tool to allow hooks to intercept or modify the operation.
pub async fn trigger_pre_tool_use(
    session_id: &str,
    tool_name: &str,
    tool_input: &serde_json::Value,
    cwd: &std::path::Path,
) -> CodexResult<()> {
    let context = HookEventContext {
        session_id: session_id.to_string(),
        transcript_path: None,
        cwd: cwd.to_string_lossy().to_string(),
        hook_event_name: HookEventName::PreToolUse,
        timestamp: chrono::Utc::now().to_rfc3339(),
        event_data: HookEventData::PreToolUse {
            tool_name: tool_name.to_string(),
            tool_input: tool_input.clone(),
        },
    };

    trigger_hook(context)
        .await
        .map_err(|e| CodexErr::HookBlocked(e.to_string()))
}

/// Trigger PostToolUse hook
///
/// This should be called after a tool executes successfully to allow hooks to log or process the result.
pub async fn trigger_post_tool_use(
    session_id: &str,
    tool_name: &str,
    tool_output: &serde_json::Value,
    cwd: &std::path::Path,
) -> CodexResult<()> {
    let context = HookEventContext {
        session_id: session_id.to_string(),
        transcript_path: None,
        cwd: cwd.to_string_lossy().to_string(),
        hook_event_name: HookEventName::PostToolUse,
        timestamp: chrono::Utc::now().to_rfc3339(),
        event_data: HookEventData::PostToolUse {
            tool_name: tool_name.to_string(),
            tool_output: tool_output.clone(),
        },
    };

    // PostToolUse hooks should not block execution, so we ignore errors
    if let Err(e) = trigger_hook(context).await {
        tracing::warn!("PostToolUse hook failed (non-blocking): {}", e);
    }

    Ok(())
}

/// Trigger SessionStart hook
pub async fn trigger_session_start(
    session_id: &str,
    cwd: &std::path::Path,
) -> CodexResult<()> {
    let context = HookEventContext {
        session_id: session_id.to_string(),
        transcript_path: None,
        cwd: cwd.to_string_lossy().to_string(),
        hook_event_name: HookEventName::SessionStart,
        timestamp: chrono::Utc::now().to_rfc3339(),
        event_data: HookEventData::Other,
    };

    trigger_hook(context)
        .await
        .map_err(|e| CodexErr::HookBlocked(e.to_string()))
}

/// Trigger SessionEnd hook
pub async fn trigger_session_end(
    session_id: &str,
    cwd: &std::path::Path,
) -> CodexResult<()> {
    let context = HookEventContext {
        session_id: session_id.to_string(),
        transcript_path: None,
        cwd: cwd.to_string_lossy().to_string(),
        hook_event_name: HookEventName::SessionEnd,
        timestamp: chrono::Utc::now().to_rfc3339(),
        event_data: HookEventData::Other,
    };

    // SessionEnd hooks should not block, log errors only
    if let Err(e) = trigger_hook(context).await {
        tracing::warn!("SessionEnd hook failed (non-blocking): {}", e);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_trigger_pre_tool_use() {
        // Should succeed even without hooks configured
        let result = trigger_pre_tool_use(
            "test-session",
            "test_tool",
            &serde_json::json!({"arg": "value"}),
            &PathBuf::from("/tmp"),
        )
        .await;

        assert!(result.is_ok());
    }
}
