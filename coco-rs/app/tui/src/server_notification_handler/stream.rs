//! Stream-layer handler.
//!
//! Handles [`AgentStreamEvent`] — raw agent-loop stream deltas (text,
//! thinking, tool use lifecycle, MCP tool call lifecycle). The TUI consumes
//! these directly for real-time display; the SDK dispatcher routes them
//! through `StreamAccumulator` separately.
//!
//! Per-layer defensive invariant: if a stream delta arrives before
//! `TurnStarted` has created `state.ui.streaming`, the handler lazily
//! creates the streaming state so deltas are never silently dropped. See
//! `server_notification_handler.test.rs` for the regression test.

use coco_types::AgentStreamEvent;
use coco_types::MCP_TOOL_PREFIX;
use coco_types::MCP_TOOL_SEPARATOR;

use crate::state::AppState;
use crate::state::session::ChatMessage;

pub(super) fn handle(state: &mut AppState, event: AgentStreamEvent) -> bool {
    match event {
        AgentStreamEvent::TextDelta { delta, .. } => {
            // Defensive: in normal flow TurnStarted creates the streaming
            // state before any delta arrives. If a delta lands first (e.g.
            // channel reordering between engine emit sites, or a unit test
            // driving this handler directly), fall back to lazy creation
            // so the content isn't silently dropped.
            state
                .ui
                .streaming
                .get_or_insert_with(crate::state::ui::StreamingState::new)
                .append_text(&delta);
            true
        }
        AgentStreamEvent::ThinkingDelta { delta, .. } => {
            state
                .ui
                .streaming
                .get_or_insert_with(crate::state::ui::StreamingState::new)
                .append_thinking(&delta);
            true
        }
        AgentStreamEvent::ToolUseQueued { call_id, name, .. } => {
            state.session.start_tool(call_id, name);
            true
        }
        AgentStreamEvent::ToolUseStarted { call_id, .. } => {
            state.session.run_tool(&call_id);
            true
        }
        AgentStreamEvent::ToolUseCompleted {
            call_id,
            name: _,
            output,
            is_error,
        } => {
            state.session.complete_tool(&call_id, is_error);
            let tool_name_str = state
                .session
                .tool_executions
                .iter()
                .find(|t| t.call_id == call_id)
                .map(|t| t.name.clone())
                .unwrap_or_default();
            if is_error {
                state.session.add_message(ChatMessage::tool_error(
                    format!("tool-{call_id}"),
                    &tool_name_str,
                    output,
                ));
            } else {
                state.session.add_message(ChatMessage::tool_success(
                    format!("tool-{call_id}"),
                    &tool_name_str,
                    output,
                ));
            }
            true
        }
        AgentStreamEvent::McpToolCallBegin {
            server,
            tool,
            call_id,
        } => {
            // Only create if not already tracked via ToolUseQueued (avoids
            // duplicate for MCP tools that go through the regular pipeline).
            let already_tracked = state
                .session
                .tool_executions
                .iter()
                .any(|t| t.call_id == call_id);
            if !already_tracked {
                state.session.start_tool(
                    call_id,
                    format!("{MCP_TOOL_PREFIX}{server}{MCP_TOOL_SEPARATOR}{tool}"),
                );
            }
            true
        }
        AgentStreamEvent::McpToolCallEnd {
            call_id, is_error, ..
        } => {
            state.session.complete_tool(&call_id, is_error);
            true
        }
    }
}
