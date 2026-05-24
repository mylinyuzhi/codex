use cocode_protocol::stream_event::StreamEvent;

use crate::state::AppState;

use super::handle_stream_event_tui;

#[test]
fn test_mcp_tool_call_lifecycle() {
    let mut state = AppState::default();

    handle_stream_event_tui(
        &mut state,
        &StreamEvent::McpToolCallBegin {
            call_id: "c1".to_string(),
            server: "srv".to_string(),
            tool: "read".to_string(),
        },
    );
    assert_eq!(state.session.mcp_tool_calls.len(), 1);

    handle_stream_event_tui(
        &mut state,
        &StreamEvent::McpToolCallEnd {
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

    handle_stream_event_tui(
        &mut state,
        &StreamEvent::McpToolCallBegin {
            call_id: "c2".to_string(),
            server: "srv".to_string(),
            tool: "write".to_string(),
        },
    );

    handle_stream_event_tui(
        &mut state,
        &StreamEvent::McpToolCallEnd {
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
