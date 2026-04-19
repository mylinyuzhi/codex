//! Tests for server notification handler.
//!
//! Post WS-2: tests use CoreEvent directly instead of the deleted
//! TuiNotification bridge type.

use coco_types::AgentStreamEvent;
use coco_types::CoreEvent;
use coco_types::ServerNotification;

use crate::server_notification_handler::handle_core_event;
use crate::state::AppState;
use crate::state::session::ChatRole;

#[test]
fn test_turn_lifecycle() {
    let mut state = AppState::new();

    // Turn started
    let changed = handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TurnStarted(
            coco_types::TurnStartedParams {
                turn_id: Some("t1".into()),
                turn_number: 1,
            },
        )),
    );
    assert!(changed);
    assert_eq!(state.session.turn_count, 1);
    assert!(state.session.is_busy());
    assert!(state.is_streaming());

    // Text delta (via Stream layer — TUI consumes directly)
    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::TextDelta {
            turn_id: "t1".into(),
            delta: "Hello ".into(),
        }),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::TextDelta {
            turn_id: "t1".into(),
            delta: "world".into(),
        }),
    );
    assert_eq!(
        state.ui.streaming.as_ref().map(|s| s.content.as_str()),
        Some("Hello world")
    );

    // Turn completed — streaming committed to messages
    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TurnCompleted(
            coco_types::TurnCompletedParams {
                turn_id: Some("t1".into()),
                usage: coco_types::TokenUsage {
                    input_tokens: 100,
                    output_tokens: 50,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
            },
        )),
    );
    assert!(!state.session.is_busy());
    assert!(!state.is_streaming());
    assert_eq!(state.session.messages.len(), 1);
    assert_eq!(state.session.messages[0].text_content(), "Hello world");
    assert_eq!(state.session.messages[0].role, ChatRole::Assistant);
}

#[test]
fn test_tool_use_lifecycle() {
    let mut state = AppState::new();

    // Tool queued (via Stream layer)
    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ToolUseQueued {
            call_id: "c1".into(),
            name: "Bash".into(),
            input: serde_json::json!({"command": "ls -la"}),
        }),
    );
    assert_eq!(state.session.tool_executions.len(), 1);

    // Tool completed (via Stream layer)
    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ToolUseCompleted {
            call_id: "c1".into(),
            name: "Bash".into(),
            output: "file1.rs\nfile2.rs".into(),
            is_error: false,
        }),
    );
    assert!(!state.session.messages.is_empty());
    assert_eq!(state.session.messages[0].role, ChatRole::Tool);
}

#[test]
fn test_subagent_lifecycle() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::SubagentSpawned(
            coco_types::SubagentSpawnedParams {
                agent_id: "a1".into(),
                agent_type: "Explore".into(),
                description: "Searching codebase".into(),
                color: None,
            },
        )),
    );
    assert_eq!(state.session.subagents.len(), 1);

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::SubagentCompleted(
            coco_types::SubagentCompletedParams {
                agent_id: "a1".into(),
                result: "Found 3 files".into(),
                is_error: false,
            },
        )),
    );
    assert_eq!(
        state.session.subagents[0].status,
        crate::state::session::SubagentStatus::Completed
    );
}

#[test]
fn test_permission_request_shows_overlay() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Tui(coco_types::TuiOnlyEvent::ApprovalRequired {
            request_id: "req-1".into(),
            tool_name: "Bash".into(),
            description: "Execute command".into(),
            input_preview: "rm -rf /tmp/test".into(),
        }),
    );
    assert!(state.has_overlay());
    assert!(matches!(
        state.ui.overlay,
        Some(crate::state::Overlay::Permission(_))
    ));
}

#[test]
fn test_error_shows_toast() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::Error(coco_types::ErrorParams {
            message: "API rate limit".into(),
            category: None,
            retryable: true,
        })),
    );
    assert!(state.ui.has_toasts());
}

#[test]
fn test_mcp_status() {
    let mut state = AppState::new();

    // New server
    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::McpStartupStatus(
            coco_types::McpStartupStatusParams {
                server: "github".into(),
                status: coco_types::McpConnectionStatus::Connected,
            },
        )),
    );
    assert_eq!(state.session.mcp_servers.len(), 1);
    assert_eq!(state.session.connected_mcp_count(), 1);

    // Update existing — disconnected
    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::McpStartupStatus(
            coco_types::McpStartupStatusParams {
                server: "github".into(),
                status: coco_types::McpConnectionStatus::Failed,
            },
        )),
    );
    assert_eq!(state.session.mcp_servers.len(), 1);
    assert_eq!(state.session.connected_mcp_count(), 0);
}

#[test]
fn test_session_ended_quits() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::SessionEnded(
            coco_types::SessionEndedParams {
                reason: "max_turns".into(),
            },
        )),
    );
    assert!(state.should_exit());
}

#[test]
fn test_plan_mode_changed() {
    let mut state = AppState::new();
    assert!(!state.is_plan_mode());

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::PlanModeChanged(
            coco_types::PlanModeChangedParams {
                entered: true,
                plan_file: None,
                approved: None,
            },
        )),
    );
    assert!(state.is_plan_mode());

    // Exit path: entered=false flips Plan back to Default.
    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::PlanModeChanged(
            coco_types::PlanModeChangedParams {
                entered: false,
                plan_file: None,
                approved: None,
            },
        )),
    );
    assert!(!state.is_plan_mode());
}

#[test]
fn test_context_compacted_toast() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::ContextCompacted(
            coco_types::ContextCompactedParams {
                removed_messages: 10,
                summary_tokens: 500,
            },
        )),
    );
    assert!(state.ui.has_toasts());
}

/// Regression: a stream delta arriving before TurnStarted must not be
/// silently dropped. Before the fix, handle_stream would no-op when
/// `state.ui.streaming` was None, so the first delta content was lost
/// whenever the channel reordered emission across senders.
#[test]
fn test_text_delta_before_turn_started_creates_streaming_state() {
    let mut state = AppState::new();
    assert!(state.ui.streaming.is_none());

    let changed = handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::TextDelta {
            turn_id: "t1".into(),
            delta: "orphan".into(),
        }),
    );

    assert!(changed);
    let streaming = state
        .ui
        .streaming
        .as_ref()
        .expect("streaming state must be created lazily");
    assert_eq!(
        streaming.content, "orphan",
        "delta must be appended, not dropped"
    );
}

/// Same invariant for ThinkingDelta.
#[test]
fn test_thinking_delta_before_turn_started_creates_streaming_state() {
    let mut state = AppState::new();
    assert!(state.ui.streaming.is_none());

    let changed = handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ThinkingDelta {
            turn_id: "t1".into(),
            delta: "early thought".into(),
        }),
    );

    assert!(changed);
    let streaming = state
        .ui
        .streaming
        .as_ref()
        .expect("streaming state must be created lazily");
    assert_eq!(streaming.thinking, "early thought");
}
