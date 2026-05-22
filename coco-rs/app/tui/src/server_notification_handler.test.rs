//! Tests for server notification handler.
//!
//! Tests use `CoreEvent` directly. Turn-lifecycle and tool-use-lifecycle
//! suites that asserted on TUI-synthesised buffer entries are gone —
//! the engine pushes authoritative `Message::Assistant` /
//! `Message::ToolResult` via `MessageAppended`; the TUI just renders
//! the cells. Reasoning-token metadata is stamped via
//! `TranscriptView::record_reasoning_tokens` — exercised by the cell
//! renderer tests in `state::transcript_view` and `widgets`.

use std::time::Duration;
use std::time::Instant;

use coco_types::CoreEvent;
use coco_types::ServerNotification;

use coco_types::AgentStreamEvent;

use crate::server_notification_handler::handle_event_for_test as handle_core_event;
use crate::state::AppState;

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
fn test_permission_request_shows_prompt() {
    let mut state = AppState::new();
    let ready_at = Instant::now() + Duration::from_secs(2);

    handle_core_event(
        &mut state,
        CoreEvent::Tui(coco_types::TuiOnlyEvent::ApprovalRequired {
            request_id: "req-1".into(),
            tool_name: "Bash".into(),
            description: "Execute command".into(),
            display_input: coco_types::PermissionDisplayInput::Command("rm -rf /tmp/test".into()),
            show_always_allow: true,
            choices: None,
            permission_suggestions: vec![],
            original_input: None,
        }),
    );
    assert!(!state.has_active_surface());
    assert!(state.ui.flush_delayed_permissions(ready_at));

    assert!(state.has_active_surface());
    assert!(matches!(
        state.ui.interaction.active_prompt,
        Some(crate::state::PanePromptState::Permission(_))
    ));
    match state.ui.interaction.active_prompt.as_ref() {
        Some(crate::state::PanePromptState::Permission(state)) => {
            assert!(state.show_always_allow);
        }
        other => panic!("expected permission state, got {other:?}"),
    }
}

#[test]
fn test_permission_request_hides_always_allow_when_disabled() {
    let mut state = AppState::new();
    let ready_at = Instant::now() + Duration::from_secs(2);

    handle_core_event(
        &mut state,
        CoreEvent::Tui(coco_types::TuiOnlyEvent::ApprovalRequired {
            request_id: "req-1".into(),
            tool_name: "Bash".into(),
            description: "Execute command".into(),
            display_input: coco_types::PermissionDisplayInput::Command("rm -rf /tmp/test".into()),
            show_always_allow: false,
            choices: None,
            permission_suggestions: vec![],
            original_input: None,
        }),
    );
    assert!(state.ui.flush_delayed_permissions(ready_at));

    match state.ui.interaction.active_prompt.as_ref() {
        Some(crate::state::PanePromptState::Permission(state)) => {
            assert!(!state.show_always_allow);
        }
        other => panic!("expected permission state, got {other:?}"),
    }
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
                trigger: coco_types::CompactTrigger::Auto,
                pre_tokens: None,
                post_tokens: None,
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
