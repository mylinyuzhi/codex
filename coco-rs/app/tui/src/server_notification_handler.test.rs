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
fn test_bg_agent_task_started_bridges_into_subagents() {
    // BgAgent TaskStarted (wire `task_type == "local_agent"`) populates
    // `session.subagents`. The bridge links via `tool_use_id` so the
    // inline `AgentProgressLine` renderer can later attach the row to
    // the parent `Agent` tool execution.
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TaskStarted(
            coco_types::TaskStartedParams {
                task_id: "agent-bg-1".into(),
                tool_use_id: Some("tu-42".into()),
                description: "Investigate auth flow".into(),
                task_type: Some("local_agent".into()),
                workflow_name: None,
                prompt: None,
                agent_name: None,
                team_name: None,
                color: None,
                backend_kind: None,
            },
        )),
    );

    assert_eq!(state.session.subagents.len(), 1);
    let agent = &state.session.subagents[0];
    assert_eq!(agent.agent_id, "agent-bg-1");
    assert_eq!(agent.tool_use_id.as_deref(), Some("tu-42"));
    assert_eq!(agent.description, "Investigate auth flow");
    assert_eq!(agent.status, crate::state::session::SubagentStatus::Running);
    // active_tasks projection still works.
    assert_eq!(state.session.active_tasks.len(), 1);
}

#[test]
fn test_shell_task_started_does_not_create_subagent() {
    // Only `local_agent` / `in_process_teammate` task_types bridge into
    // `session.subagents`; `local_bash` (and `dream`) stay in
    // `active_tasks` only — they're not "subagents" in the TS sense.
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TaskStarted(
            coco_types::TaskStartedParams {
                task_id: "sh-1".into(),
                tool_use_id: Some("tu-99".into()),
                description: "sleep 30".into(),
                task_type: Some("local_bash".into()),
                workflow_name: None,
                prompt: None,
                agent_name: None,
                team_name: None,
                color: None,
                backend_kind: None,
            },
        )),
    );

    assert_eq!(state.session.subagents.len(), 0);
    assert_eq!(state.session.active_tasks.len(), 1);
}

#[test]
fn test_in_process_teammate_task_started_creates_teammate_kind_row() {
    // TS-aligned spawn: coordinator emits `task/started` with
    // `task_type == "in_process_teammate"` and the optional teammate
    // metadata populated. coco-rs projects that into a SubagentInstance
    // with kind=Teammate, team_name set, tool_use_id None.
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TaskStarted(
            coco_types::TaskStartedParams {
                task_id: "researcher@my-team".into(),
                tool_use_id: None,
                description: "Kick off auth research".into(),
                task_type: Some("in_process_teammate".into()),
                workflow_name: None,
                prompt: None,
                agent_name: Some("researcher".into()),
                team_name: Some("my-team".into()),
                color: Some("blue".into()),
                backend_kind: Some("in_process".into()),
            },
        )),
    );

    assert_eq!(state.session.subagents.len(), 1);
    let agent = &state.session.subagents[0];
    assert_eq!(agent.agent_id, "researcher@my-team");
    assert!(matches!(
        agent.kind,
        crate::state::session::SubagentKind::Teammate
    ));
    assert_eq!(agent.agent_type, "researcher");
    assert_eq!(agent.team_name.as_deref(), Some("my-team"));
    assert_eq!(agent.tool_use_id, None);
    assert_eq!(agent.color.as_deref(), Some("blue"));
    // active_tasks projection works in parallel.
    assert_eq!(state.session.active_tasks.len(), 1);
}

#[test]
fn test_teammate_task_started_dedupes_on_task_id() {
    // Re-emit with the same task_id is a no-op (coordinator may
    // republish refresh-style events without duplicating rows).
    let mut state = AppState::new();
    let params = coco_types::TaskStartedParams {
        task_id: "r@t".into(),
        tool_use_id: None,
        description: "".into(),
        task_type: Some("in_process_teammate".into()),
        workflow_name: None,
        prompt: None,
        agent_name: Some("r".into()),
        team_name: Some("t".into()),
        color: None,
        backend_kind: Some("in_process".into()),
    };
    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TaskStarted(params.clone())),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TaskStarted(params)),
    );
    // active_tasks accumulates (TS does the same — the dedup is only
    // for the subagent projection).
    assert_eq!(state.session.subagents.len(), 1);
}

#[test]
fn test_bg_agent_task_started_marks_subagent_kind() {
    // The BgAgent bridge should set `kind == Subagent`, not Teammate.
    let mut state = AppState::new();
    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TaskStarted(
            coco_types::TaskStartedParams {
                task_id: "agent-bg-X".into(),
                tool_use_id: Some("tu-1".into()),
                description: "task".into(),
                task_type: Some("local_agent".into()),
                workflow_name: None,
                prompt: None,
                agent_name: None,
                team_name: None,
                color: None,
                backend_kind: None,
            },
        )),
    );
    let agent = &state.session.subagents[0];
    assert!(matches!(
        agent.kind,
        crate::state::session::SubagentKind::Subagent
    ));
    assert_eq!(agent.team_name, None);
}

#[test]
fn test_bg_agent_task_completed_updates_subagent_status() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TaskStarted(
            coco_types::TaskStartedParams {
                task_id: "agent-bg-1".into(),
                tool_use_id: Some("tu-42".into()),
                description: "task".into(),
                task_type: Some("local_agent".into()),
                workflow_name: None,
                prompt: None,
                agent_name: None,
                team_name: None,
                color: None,
                backend_kind: None,
            },
        )),
    );

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TaskCompleted(
            coco_types::TaskCompletedParams {
                task_id: "agent-bg-1".into(),
                tool_use_id: Some("tu-42".into()),
                status: coco_types::TaskCompletionStatus::Completed,
                output_file: String::new(),
                summary: "Found 7 callers".into(),
                usage: None,
            },
        )),
    );

    let agent = &state.session.subagents[0];
    assert_eq!(
        agent.status,
        crate::state::session::SubagentStatus::Completed
    );
    assert_eq!(agent.final_message.as_deref(), Some("Found 7 callers"));
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
