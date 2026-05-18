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

use super::projection::flush_streaming_to_messages;
use crate::state::AppState;
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;
use crate::state::session::ToolUseStatus;

const TOOL_INPUT_PREVIEW_MAX_CHARS: usize = 512;

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
        AgentStreamEvent::ToolUseQueued {
            call_id,
            name,
            input,
        } => {
            flush_streaming_to_messages(state);
            let input_preview = tool_input_preview(&name, &input);
            state.session.start_tool(call_id.clone(), name.clone());
            state.session.add_message(ChatMessage {
                id: format!("tool-use-{call_id}"),
                role: crate::state::ChatRole::Assistant,
                content: MessageContent::ToolUse {
                    tool_name: name,
                    call_id,
                    input_preview,
                    status: ToolUseStatus::Queued,
                },
                is_meta: false,
                created_at_ms: crate::state::session::now_ms(),
                is_compact_summary: false,
                is_visible_in_transcript_only: false,
                permission_mode: None,
            });
            true
        }
        AgentStreamEvent::ToolUseStarted { call_id, .. } => {
            state.session.run_tool(&call_id);
            true
        }
        AgentStreamEvent::ToolUseCompleted {
            call_id,
            name,
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
                .unwrap_or(name);
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

fn tool_input_preview(tool_name: &str, input: &serde_json::Value) -> String {
    let normalized = tool_name
        .rsplit(MCP_TOOL_SEPARATOR)
        .next()
        .unwrap_or(tool_name)
        .to_ascii_lowercase();
    if matches!(normalized.as_str(), "bash" | "powershell")
        && let Some(command) = input.get("command").and_then(serde_json::Value::as_str)
    {
        return single_line_capped(command, TOOL_INPUT_PREVIEW_MAX_CHARS);
    }
    serde_json::to_string(input)
        .map(|s| single_line_capped(&s, TOOL_INPUT_PREVIEW_MAX_CHARS))
        .unwrap_or_default()
}

fn single_line_capped(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    for chunk in text.split_whitespace() {
        let space = usize::from(!out.is_empty());
        let chunk_len = chunk.chars().count();
        if count + space + chunk_len > max_chars {
            if max_chars > 3 {
                while count + 3 > max_chars {
                    out.pop();
                    count = count.saturating_sub(1);
                }
                out.push_str("...");
            }
            return out;
        }
        if space == 1 {
            out.push(' ');
            count += 1;
        }
        out.push_str(chunk);
        count += chunk_len;
    }
    out
}
