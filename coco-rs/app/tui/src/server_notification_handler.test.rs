//! Tests for server notification handler.
//!
//! Post WS-2: tests use CoreEvent directly instead of the deleted
//! TuiNotification bridge type.

use std::time::Duration;
use std::time::Instant;

use coco_types::AgentStreamEvent;
use coco_types::CoreEvent;
use coco_types::ServerNotification;

use crate::server_notification_handler::handle_core_event;
use crate::state::AppState;
use crate::state::session::ChatRole;
use crate::state::session::MessageContent;

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
                    input_token_details: coco_types::InputTokenDetails {
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
                        ..Default::default()
                    },
                    ..Default::default()
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
    assert_eq!(state.session.messages.len(), 1);
    assert_eq!(state.session.messages[0].role, ChatRole::Assistant);

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
    assert_eq!(state.session.messages.len(), 2);
    assert_eq!(state.session.messages[1].role, ChatRole::Tool);
}

#[test]
fn test_tool_use_completed_uses_event_name_when_execution_missing() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ToolUseCompleted {
            call_id: "missing".into(),
            name: "Read".into(),
            output: "done".into(),
            is_error: false,
        }),
    );

    assert!(matches!(
        &state.session.messages[0].content,
        MessageContent::ToolSuccess { tool_name, .. } if tool_name == "Read"
    ));
}

#[test]
fn test_tool_use_queued_formats_write_preview_without_content_json() {
    let mut state = AppState::new();
    let large_text = "x".repeat(2_000);

    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ToolUseQueued {
            call_id: "c1".into(),
            name: "Write".into(),
            input: serde_json::json!({"file_path": "/tmp/out.txt", "content": large_text}),
        }),
    );

    assert!(matches!(
        &state.session.messages[0].content,
        MessageContent::ToolUse { input_preview, .. } if input_preview == "/tmp/out.txt"
    ));
}

#[test]
fn test_tool_use_queued_formats_glob_preview_without_json() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ToolUseQueued {
            call_id: "c1".into(),
            name: coco_types::ToolName::Glob.as_str().into(),
            input: serde_json::json!({
                "path": "/Users/linyuzhi/codespace/myagent/codex",
                "pattern": "**/README.md"
            }),
        }),
    );

    assert!(matches!(
        &state.session.messages[0].content,
        MessageContent::ToolUse { input_preview, .. }
            if input_preview == "**/README.md in /Users/linyuzhi/codespace/myagent/codex"
    ));
}

#[test]
fn test_tool_use_queued_flushes_reasoning_before_tool_call() {
    let mut state = AppState::new();
    state.session.turn_count = 7;

    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ThinkingDelta {
            turn_id: "t1".into(),
            delta: "I should inspect the file first.".into(),
        }),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ToolUseQueued {
            call_id: "c1".into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": "/tmp/README.md", "limit": 3}),
        }),
    );

    assert_eq!(state.session.messages.len(), 2);
    assert!(matches!(
        state.session.messages[0].content,
        MessageContent::Thinking { .. }
    ));
    assert!(matches!(
        state.session.messages[1].content,
        MessageContent::ToolUse { .. }
    ));
}

#[test]
fn test_turn_completed_inserts_reasoning_token_only_message_before_text() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TurnStarted(
            coco_types::TurnStartedParams {
                turn_id: Some("t1".into()),
                turn_number: 3,
            },
        )),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::TextDelta {
            turn_id: "t1".into(),
            delta: "final answer".into(),
        }),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TurnCompleted(
            coco_types::TurnCompletedParams {
                turn_id: Some("t1".into()),
                usage: coco_types::TokenUsage {
                    output_token_details: coco_types::OutputTokenDetails {
                        reasoning_tokens: 220,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            },
        )),
    );

    assert_eq!(state.session.messages.len(), 2);
    assert!(matches!(
        &state.session.messages[0].content,
        MessageContent::Thinking {
            content,
            reasoning_tokens: Some(220),
            duration_ms: Some(_),
            ..
        } if content.is_empty()
    ));
    assert!(matches!(
        &state.session.messages[1].content,
        MessageContent::AssistantText(text) if text == "final answer"
    ));
}

#[test]
fn test_turn_completed_updates_tool_call_thinking_tokens() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TurnStarted(
            coco_types::TurnStartedParams {
                turn_id: Some("t1".into()),
                turn_number: 3,
            },
        )),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ThinkingDelta {
            turn_id: "t1".into(),
            delta: "Need a command.".into(),
        }),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ToolUseQueued {
            call_id: "c1".into(),
            name: "Bash".into(),
            input: serde_json::json!({"command": "ls -al"}),
        }),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ToolUseCompleted {
            call_id: "c1".into(),
            name: "Bash".into(),
            output: "ok".into(),
            is_error: false,
        }),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TurnCompleted(
            coco_types::TurnCompletedParams {
                turn_id: Some("t1".into()),
                usage: coco_types::TokenUsage {
                    output_token_details: coco_types::OutputTokenDetails {
                        reasoning_tokens: 13,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            },
        )),
    );

    assert_eq!(state.session.messages.len(), 3);
    assert!(matches!(
        &state.session.messages[0].content,
        MessageContent::Thinking {
            reasoning_tokens: Some(13),
            ..
        }
    ));
    assert!(matches!(
        state.session.messages[1].content,
        MessageContent::ToolUse { .. }
    ));
    assert!(matches!(
        state.session.messages[2].content,
        MessageContent::ToolSuccess { .. }
    ));
}

#[test]
fn test_turn_completed_inserts_token_only_thinking_before_tool_call() {
    let mut state = AppState::new();

    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TurnStarted(
            coco_types::TurnStartedParams {
                turn_id: Some("t1".into()),
                turn_number: 3,
            },
        )),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Stream(AgentStreamEvent::ToolUseQueued {
            call_id: "c1".into(),
            name: "Bash".into(),
            input: serde_json::json!({"command": "ls -al"}),
        }),
    );
    handle_core_event(
        &mut state,
        CoreEvent::Protocol(ServerNotification::TurnCompleted(
            coco_types::TurnCompletedParams {
                turn_id: Some("t1".into()),
                usage: coco_types::TokenUsage {
                    output_token_details: coco_types::OutputTokenDetails {
                        reasoning_tokens: 13,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            },
        )),
    );

    assert_eq!(state.session.messages.len(), 2);
    assert!(matches!(
        &state.session.messages[0].content,
        MessageContent::Thinking {
            content,
            reasoning_tokens: Some(13),
            duration_ms: Some(_),
            ..
        } if content.is_empty()
    ));
    assert!(matches!(
        state.session.messages[1].content,
        MessageContent::ToolUse { .. }
    ));
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
