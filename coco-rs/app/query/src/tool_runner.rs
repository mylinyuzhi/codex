use std::sync::Arc;

use coco_messages::MessageHistory;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use coco_types::CoreEvent;
use coco_types::ToolId;
use tokio::sync::mpsc;
use tracing::warn;
use vercel_ai_provider::ToolCallPart;

use crate::emit::emit_stream;
use crate::helpers::complete_tool_call_with_error;

/// Resolved and validated tool call ready for permission/hook/execution.
pub(crate) struct PreparedToolCall {
    pub tool_id: ToolId,
    pub tool: Arc<dyn Tool>,
}

/// Prepare one committed assistant tool call.
///
/// This owns the first part of the tool-result pairing invariant:
/// every committed call emits `ToolUseQueued`; calls that cannot become
/// runnable because the tool is unknown or the input is invalid are completed
/// here with exactly one model-visible error result.
pub(crate) async fn prepare_committed_tool_call(
    event_tx: &Option<mpsc::Sender<CoreEvent>>,
    history: &mut MessageHistory,
    tools: &ToolRegistry,
    ctx: &ToolUseContext,
    tool_call: &ToolCallPart,
) -> Option<PreparedToolCall> {
    let tool_id: ToolId = tool_call
        .tool_name
        .parse()
        .unwrap_or_else(|_| ToolId::Custom(tool_call.tool_name.clone()));

    let _delivered = emit_stream(
        event_tx,
        crate::AgentStreamEvent::ToolUseQueued {
            call_id: tool_call.tool_call_id.clone(),
            name: tool_call.tool_name.clone(),
            input: tool_call.input.clone(),
        },
    )
    .await;

    let Some(tool) = tools.get(&tool_id).cloned() else {
        warn!(tool = tool_call.tool_name, "tool not found in registry");
        let output = format!("Unknown tool: {}", tool_call.tool_name);
        complete_tool_call_with_error(
            event_tx,
            history,
            &tool_call.tool_call_id,
            &tool_call.tool_name,
            &tool_id,
            &output,
        )
        .await;
        return None;
    };

    let validation = tool.validate_input(&tool_call.input, ctx);
    if !validation.is_valid() {
        let message = match validation {
            coco_tool_runtime::ValidationResult::Invalid { message, .. } => {
                format!("Invalid input: {message}")
            }
            coco_tool_runtime::ValidationResult::Valid => "Invalid input".to_string(),
        };
        warn!(
            tool = tool_call.tool_name,
            tool_use_id = tool_call.tool_call_id,
            %message,
            "tool input validation failed"
        );
        complete_tool_call_with_error(
            event_tx,
            history,
            &tool_call.tool_call_id,
            &tool_call.tool_name,
            &tool_id,
            &message,
        )
        .await;
        return None;
    }

    Some(PreparedToolCall { tool_id, tool })
}
