//! Tests for server notification handler.

use crate::server_notification_handler::ServerNotification;
use crate::server_notification_handler::handle_server_notification;
use crate::state::AppState;
use crate::state::session::ChatRole;
use crate::state::session::TokenUsage;

#[test]
fn test_turn_lifecycle() {
    let mut state = AppState::new();

    // Turn started
    let changed = handle_server_notification(
        &mut state,
        ServerNotification::TurnStarted { turn_number: 1 },
    );
    assert!(changed);
    assert_eq!(state.session.turn_count, 1);
    assert!(state.session.is_busy());
    assert!(state.is_streaming());

    // Text delta
    handle_server_notification(
        &mut state,
        ServerNotification::TextDelta {
            delta: "Hello ".to_string(),
        },
    );
    handle_server_notification(
        &mut state,
        ServerNotification::TextDelta {
            delta: "world".to_string(),
        },
    );
    assert_eq!(
        state.ui.streaming.as_ref().map(|s| s.content.as_str()),
        Some("Hello world")
    );

    // Turn completed — streaming committed to messages
    handle_server_notification(
        &mut state,
        ServerNotification::TurnCompleted {
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            },
        },
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

    // Tool queued
    handle_server_notification(
        &mut state,
        ServerNotification::ToolUseQueued {
            call_id: "c1".to_string(),
            name: "Bash".to_string(),
            input_preview: "ls -la".to_string(),
        },
    );
    assert_eq!(state.session.tool_executions.len(), 1);

    // Tool completed
    handle_server_notification(
        &mut state,
        ServerNotification::ToolUseCompleted {
            call_id: "c1".to_string(),
            output: "file1.rs\nfile2.rs".to_string(),
            is_error: false,
        },
    );
    // Tool result added to messages
    assert!(!state.session.messages.is_empty());
    assert_eq!(state.session.messages[0].role, ChatRole::Tool);
}

#[test]
fn test_subagent_lifecycle() {
    let mut state = AppState::new();

    handle_server_notification(
        &mut state,
        ServerNotification::SubagentSpawned {
            agent_id: "a1".to_string(),
            agent_type: "Explore".to_string(),
            description: "Searching codebase".to_string(),
            color: None,
        },
    );
    assert_eq!(state.session.subagents.len(), 1);

    handle_server_notification(
        &mut state,
        ServerNotification::SubagentCompleted {
            agent_id: "a1".to_string(),
            result: "Found 3 files".to_string(),
            is_error: false,
        },
    );
    assert_eq!(
        state.session.subagents[0].status,
        crate::state::session::SubagentStatus::Completed
    );
}

#[test]
fn test_permission_request_shows_overlay() {
    let mut state = AppState::new();

    handle_server_notification(
        &mut state,
        ServerNotification::PermissionRequest {
            request_id: "req-1".to_string(),
            tool_name: "Bash".to_string(),
            description: "Execute command".to_string(),
            input_preview: "rm -rf /tmp/test".to_string(),
        },
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

    handle_server_notification(
        &mut state,
        ServerNotification::Error {
            message: "API rate limit".to_string(),
            retryable: true,
        },
    );
    assert!(state.ui.has_toasts());
}

#[test]
fn test_mcp_status() {
    let mut state = AppState::new();

    // New server
    handle_server_notification(
        &mut state,
        ServerNotification::McpStatus {
            server_name: "github".to_string(),
            connected: true,
            tool_count: 5,
        },
    );
    assert_eq!(state.session.mcp_servers.len(), 1);
    assert_eq!(state.session.connected_mcp_count(), 1);

    // Update existing
    handle_server_notification(
        &mut state,
        ServerNotification::McpStatus {
            server_name: "github".to_string(),
            connected: false,
            tool_count: 0,
        },
    );
    assert_eq!(state.session.mcp_servers.len(), 1);
    assert_eq!(state.session.connected_mcp_count(), 0);
}

#[test]
fn test_session_ended_quits() {
    let mut state = AppState::new();

    handle_server_notification(
        &mut state,
        ServerNotification::SessionEnded {
            reason: "max_turns".to_string(),
        },
    );
    assert!(state.should_exit());
}
