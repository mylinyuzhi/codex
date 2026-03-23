use cocode_protocol::LoopEvent;

use crate::state::AppState;

use super::handle_agent_event;

#[test]
fn test_stream_request_start_does_not_panic() {
    let mut state = AppState::default();
    handle_agent_event(&mut state, LoopEvent::StreamRequestStart);
}

#[test]
fn test_interrupted_stops_streaming() {
    let mut state = AppState::default();
    state.ui.start_streaming("turn-1".to_string());
    handle_agent_event(&mut state, LoopEvent::Interrupted);
    assert!(state.ui.streaming.is_none());
}

#[test]
fn test_background_task_lifecycle() {
    let mut state = AppState::default();

    handle_agent_event(
        &mut state,
        LoopEvent::BackgroundTaskStarted {
            task_id: "t1".to_string(),
            task_type: cocode_protocol::TaskType::Shell,
        },
    );
    assert_eq!(state.session.background_tasks.len(), 1);

    handle_agent_event(
        &mut state,
        LoopEvent::BackgroundTaskCompleted {
            task_id: "t1".to_string(),
            result: "done".to_string(),
        },
    );
    assert_eq!(
        state.session.background_tasks[0].status,
        crate::state::BackgroundTaskStatus::Completed
    );
}

#[test]
fn test_mcp_tool_call_lifecycle() {
    let mut state = AppState::default();

    handle_agent_event(
        &mut state,
        LoopEvent::McpToolCallBegin {
            call_id: "c1".to_string(),
            server: "srv".to_string(),
            tool: "read".to_string(),
        },
    );
    assert_eq!(state.session.mcp_tool_calls.len(), 1);

    handle_agent_event(
        &mut state,
        LoopEvent::McpToolCallEnd {
            call_id: "c1".to_string(),
            server: "srv".to_string(),
            tool: "read".to_string(),
            is_error: false,
        },
    );
    assert_eq!(
        state.session.mcp_tool_calls[0].status,
        crate::state::ToolStatus::Completed
    );
}

#[test]
fn test_mcp_tool_call_error_shows_toast() {
    let mut state = AppState::default();

    handle_agent_event(
        &mut state,
        LoopEvent::McpToolCallBegin {
            call_id: "c2".to_string(),
            server: "srv".to_string(),
            tool: "write".to_string(),
        },
    );

    handle_agent_event(
        &mut state,
        LoopEvent::McpToolCallEnd {
            call_id: "c2".to_string(),
            server: "srv".to_string(),
            tool: "write".to_string(),
            is_error: true,
        },
    );
    assert_eq!(
        state.session.mcp_tool_calls[0].status,
        crate::state::ToolStatus::Failed
    );
    assert!(!state.ui.toasts.is_empty());
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

    handle_agent_event(
        &mut state,
        LoopEvent::AllAgentsKilled {
            count: 1,
            agent_ids: vec!["a1".to_string()],
        },
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
    handle_agent_event(
        &mut state,
        LoopEvent::Retry {
            attempt: 2,
            max_attempts: 3,
            delay_ms: 1000,
        },
    );
    assert!(!state.ui.toasts.is_empty());
}

#[test]
fn test_api_error_shows_error_toast() {
    let mut state = AppState::default();
    handle_agent_event(
        &mut state,
        LoopEvent::ApiError {
            error: cocode_protocol::ApiErrorInfo {
                code: "rate_limit".to_string(),
                message: "Too many requests".to_string(),
                status: Some(429),
            },
            retry_info: None,
        },
    );
    assert!(!state.ui.toasts.is_empty());
}

#[test]
fn test_session_memory_extraction_lifecycle() {
    let mut state = AppState::default();

    handle_agent_event(
        &mut state,
        LoopEvent::SessionMemoryExtractionStarted {
            current_tokens: 50000,
            tool_calls_since: 10,
        },
    );
    assert_eq!(state.ui.toasts.len(), 1);

    handle_agent_event(
        &mut state,
        LoopEvent::SessionMemoryExtractionCompleted {
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
    handle_agent_event(
        &mut state,
        LoopEvent::SessionMemoryExtractionFailed {
            error: "timeout".to_string(),
            attempts: 3,
        },
    );
    assert!(!state.ui.toasts.is_empty());
}

#[test]
fn test_speculative_rolled_back_shows_warning() {
    let mut state = AppState::default();
    handle_agent_event(
        &mut state,
        LoopEvent::SpeculativeRolledBack {
            speculation_id: "spec-1".to_string(),
            reason: "model reconsideration".to_string(),
            rolled_back_calls: vec!["call-1".to_string()],
        },
    );
    assert!(!state.ui.toasts.is_empty());
}

#[test]
fn test_mcp_startup_update_all_variants() {
    use cocode_protocol::McpStartupStatus;

    let mut state = AppState::default();

    // Starting and Connecting should not add toasts
    handle_agent_event(
        &mut state,
        LoopEvent::McpStartupUpdate {
            server: "srv".to_string(),
            status: McpStartupStatus::Starting,
        },
    );
    assert!(state.ui.toasts.is_empty());

    handle_agent_event(
        &mut state,
        LoopEvent::McpStartupUpdate {
            server: "srv".to_string(),
            status: McpStartupStatus::Connecting,
        },
    );
    assert!(state.ui.toasts.is_empty());

    // Ready should add success toast
    handle_agent_event(
        &mut state,
        LoopEvent::McpStartupUpdate {
            server: "srv".to_string(),
            status: McpStartupStatus::Ready,
        },
    );
    assert_eq!(state.ui.toasts.len(), 1);

    // Failed should add error toast
    handle_agent_event(
        &mut state,
        LoopEvent::McpStartupUpdate {
            server: "srv".to_string(),
            status: McpStartupStatus::Failed,
        },
    );
    assert_eq!(state.ui.toasts.len(), 2);
}
