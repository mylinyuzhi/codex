//! Direct TUI handling of [`StreamEvent`] for streaming display.
//!
//! Processes streaming events for the TUI's real-time display (streaming
//! buffers, tool call tracking, spinner state). The same events are also
//! fed to [`StreamAccumulator`] for protocol-level notifications.

use cocode_protocol::ToolResultContent;
use cocode_protocol::stream_event::StreamEvent;

use crate::i18n::t;
use crate::state::AppState;

/// Handle a stream event for TUI display purposes.
///
/// Takes a reference since the caller also passes the event to
/// `StreamAccumulator::process()`.
pub fn handle_stream_event_tui(state: &mut AppState, event: &StreamEvent) {
    match event {
        StreamEvent::TextDelta { delta, .. } => {
            if state.ui.is_thinking() {
                state.ui.stop_thinking();
            }
            state.ui.append_streaming(delta);
        }
        StreamEvent::ThinkingDelta { delta, .. } => {
            state.ui.start_thinking();
            state.ui.append_streaming_thinking(delta);
        }
        StreamEvent::ToolUseQueued {
            call_id,
            name,
            input,
        } => {
            let input_str = serde_json::to_string(input).unwrap_or_default();
            state
                .ui
                .add_streaming_tool_use_with_input(call_id.clone(), name.clone(), input_str);
        }
        StreamEvent::ToolUseStarted {
            call_id,
            name,
            batch_id,
        } => {
            state.ui.set_stream_mode_tool_use();
            state.ui.spinner_text = Some(name.clone());
            state
                .session
                .start_tool_with_batch(call_id.clone(), name.clone(), batch_id.clone());
        }
        StreamEvent::ToolUseCompleted {
            call_id,
            output,
            is_error,
        } => {
            let output_str = match output {
                ToolResultContent::Text(s) => s.clone(),
                ToolResultContent::Structured(v) => v.to_string(),
            };
            state.session.complete_tool(call_id, output_str, *is_error);
            state.session.cleanup_completed_tools(10);
            state.ui.spinner_text = None;
        }
        StreamEvent::McpToolCallBegin {
            call_id,
            server,
            tool,
        } => {
            state
                .session
                .start_mcp_tool_call(call_id.clone(), server.clone(), tool.clone());
        }
        StreamEvent::McpToolCallEnd {
            call_id,
            server,
            tool,
            is_error,
        } => {
            state.session.complete_mcp_tool_call(call_id, *is_error);
            state.session.cleanup_completed_mcp_calls(10);
            if *is_error {
                state.ui.toast_error(
                    t!("toast.mcp_tool_failed", server = server, tool = tool).to_string(),
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "stream_event_handler.test.rs"]
mod tests;
