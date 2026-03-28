use cocode_app_server_protocol::ServerNotification;
use cocode_protocol::tui_event::TuiEvent;

use crate::server_notification_handler::handle_server_notification;
use crate::state::AppState;

use super::handle_tui_event;

#[test]
fn test_interrupted_stops_streaming() {
    let mut state = AppState::default();
    state.ui.start_streaming("turn-1".to_string());
    handle_server_notification(
        &mut state,
        ServerNotification::TurnInterrupted(cocode_app_server_protocol::TurnInterruptedParams {
            turn_id: None,
        }),
    );
    assert!(state.ui.streaming.is_none());
}

#[test]
fn test_background_task_lifecycle() {
    let mut state = AppState::default();

    handle_server_notification(
        &mut state,
        ServerNotification::TaskStarted(cocode_app_server_protocol::TaskStartedParams {
            task_id: "t1".to_string(),
            task_type: "shell".to_string(),
        }),
    );
    assert_eq!(state.session.background_tasks.len(), 1);

    handle_server_notification(
        &mut state,
        ServerNotification::TaskCompleted(cocode_app_server_protocol::TaskCompletedParams {
            task_id: "t1".to_string(),
            result: "done".to_string(),
            is_error: false,
        }),
    );
    assert_eq!(
        state.session.background_tasks[0].status,
        crate::state::BackgroundTaskStatus::Completed
    );
}

#[test]
fn test_all_agents_killed_marks_subagents_failed() {
    let mut state = AppState::default();
    state.session.start_subagent(
        "a1".to_string(),
        "explore".to_string(),
        "test".to_string(),
        None,
    );

    handle_server_notification(
        &mut state,
        ServerNotification::AgentsKilled(cocode_app_server_protocol::AgentsKilledParams {
            count: 1,
            agent_ids: vec!["a1".to_string()],
        }),
    );
    assert_eq!(
        state.session.subagents[0].status,
        crate::state::SubagentStatus::Failed
    );
    assert!(!state.ui.toasts.is_empty());
}

#[test]
fn test_retry_shows_info_toast() {
    let mut state = AppState::default();
    handle_server_notification(
        &mut state,
        ServerNotification::TurnRetry(cocode_app_server_protocol::TurnRetryParams {
            attempt: 2,
            max_attempts: 3,
            delay_ms: 1000,
        }),
    );
    assert!(!state.ui.toasts.is_empty());
}

#[test]
fn test_api_error_shows_error_toast() {
    let mut state = AppState::default();
    handle_server_notification(
        &mut state,
        ServerNotification::Error(cocode_app_server_protocol::ErrorNotificationParams {
            message: "Too many requests".to_string(),
            category: Some(cocode_app_server_protocol::ErrorCategory::Api),
            retryable: true,
            error_info: None,
        }),
    );
    assert!(!state.ui.toasts.is_empty());
}

#[test]
fn test_session_memory_extraction_lifecycle() {
    let mut state = AppState::default();

    handle_tui_event(
        &mut state,
        TuiEvent::SessionMemoryExtractionStarted {
            current_tokens: 50000,
            tool_calls_since: 10,
        },
    );
    assert_eq!(state.ui.toasts.len(), 1);

    handle_tui_event(
        &mut state,
        TuiEvent::SessionMemoryExtractionCompleted {
            summary_tokens: 2000,
            last_summarized_id: "msg-42".to_string(),
            messages_summarized: 15,
        },
    );
    assert_eq!(state.ui.toasts.len(), 2);
}

#[test]
fn test_session_memory_extraction_failed_shows_error() {
    let mut state = AppState::default();
    handle_tui_event(
        &mut state,
        TuiEvent::SessionMemoryExtractionFailed {
            error: "timeout".to_string(),
            attempts: 3,
        },
    );
    assert!(!state.ui.toasts.is_empty());
}

#[test]
fn test_speculative_rolled_back_shows_warning() {
    let mut state = AppState::default();
    handle_tui_event(
        &mut state,
        TuiEvent::SpeculativeRolledBack {
            speculation_id: "spec-1".to_string(),
            reason: "model reconsideration".to_string(),
            rolled_back_calls: vec!["call-1".to_string()],
        },
    );
    assert!(!state.ui.toasts.is_empty());
}

#[test]
fn test_mcp_startup_status_all_variants() {
    let mut state = AppState::default();

    // Starting and Connecting should not add toasts
    handle_server_notification(
        &mut state,
        ServerNotification::McpStartupStatus(cocode_app_server_protocol::McpStartupStatusParams {
            server: "srv".to_string(),
            status: "starting".to_string(),
        }),
    );
    assert!(state.ui.toasts.is_empty());

    handle_server_notification(
        &mut state,
        ServerNotification::McpStartupStatus(cocode_app_server_protocol::McpStartupStatusParams {
            server: "srv".to_string(),
            status: "connecting".to_string(),
        }),
    );
    assert!(state.ui.toasts.is_empty());

    // Ready should add success toast
    handle_server_notification(
        &mut state,
        ServerNotification::McpStartupStatus(cocode_app_server_protocol::McpStartupStatusParams {
            server: "srv".to_string(),
            status: "ready".to_string(),
        }),
    );
    assert_eq!(state.ui.toasts.len(), 1);

    // Failed should add error toast
    handle_server_notification(
        &mut state,
        ServerNotification::McpStartupStatus(cocode_app_server_protocol::McpStartupStatusParams {
            server: "srv".to_string(),
            status: "failed".to_string(),
        }),
    );
    assert_eq!(state.ui.toasts.len(), 2);
}
