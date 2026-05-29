use std::sync::Arc;

use coco_llm_types::ToolCallPart;
use coco_messages::MessageHistory;
use coco_tool_runtime::DynTool;
use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use coco_types::CoreEvent;
use coco_types::ToolId;
use tokio::sync::mpsc;
use tracing::warn;

use crate::emit::emit_stream;
use crate::helpers::ToolCompletionEventMode;
use crate::helpers::complete_tool_call_with_error_mode;

/// Resolved and validated tool call ready for permission/hook/execution.
pub(crate) struct PreparedToolCall {
    pub tool_id: ToolId,
    pub tool: Arc<dyn DynTool>,
}

/// Prepare one committed assistant tool call.
///
/// This owns the first part of the tool-result pairing invariant:
/// every committed call emits `ToolUseQueued`; calls that cannot become
/// runnable because the tool is unknown or the input is invalid are completed
/// here with exactly one model-visible error result.
///
/// `tool_call.input` is already the observable input: both the streaming
/// and non-streaming engine paths run
/// `tool_input_normalizer::normalize_observable_tool_input` while building
/// the assistant-message `ToolCallPart` this function receives, so no
/// re-normalization happens here.
pub(crate) async fn prepare_committed_tool_call(
    event_tx: &Option<mpsc::Sender<CoreEvent>>,
    history: &mut MessageHistory,
    tools: &ToolRegistry,
    ctx: &ToolUseContext,
    tool_call: &ToolCallPart,
    completion_event_mode: ToolCompletionEventMode,
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

    let Some(tool) = tools.get(&tool_id) else {
        warn!(tool = tool_call.tool_name, "tool not found in registry");
        // Mirror error wrap's `<tool_use_error>No such tool available: ...>`
        // wrap so the model sees the same format whether the
        // unknown-tool branch fires here (registry miss) or in
        // `tool_call_preparer` (schema validation catch for hallucinated names
        // not in the per-call tools list).
        let output = format!(
            "<tool_use_error>No such tool available: {}</tool_use_error>",
            tool_call.tool_name
        );
        complete_tool_call_with_error_mode(
            event_tx,
            history,
            &tool_call.tool_call_id,
            &tool_call.tool_name,
            &tool_id,
            &output,
            completion_event_mode,
        )
        .await;
        return None;
    };

    // wire parsing + schema validation short-circuit. The provider adapter (wire parsing)
    // may have flagged the call as `invalid` when raw `arguments`
    // bytes were unrecoverable. schema validation runs only
    // when wire parsing left the call unflagged; otherwise we preserve
    // wire parsing's reason. Both paths converge on the same `<tool_use_error>`
    // wrap selection so the model sees one format whether the failure
    // originated on the wire or in the schema validator.
    let mut validated = tool_call.clone();
    if !validated.invalid {
        // v4.2: synchronous, lock-free — the validator is owned by the
        // tool's `runtime_validation_schema()`.
        crate::tool_input_validate::validate_tool_call(&mut validated, Some(&tool));
    }
    if validated.invalid {
        let message = match validated.invalid_reason {
            Some(coco_llm_types::ToolInputInvalidReason::SchemaViolation { message }) => {
                format!("<tool_use_error>InputValidationError: {message}</tool_use_error>")
            }
            Some(coco_llm_types::ToolInputInvalidReason::NoSuchTool { tool_name }) => {
                format!("<tool_use_error>No such tool available: {tool_name}</tool_use_error>")
            }
            Some(coco_llm_types::ToolInputInvalidReason::JsonParseFailed { error, .. }) => {
                format!(
                    "<tool_use_error>The tool call arguments could not be parsed as JSON: {error}. \
                     Please retry with valid JSON.</tool_use_error>"
                )
            }
            None => "<tool_use_error>Invalid tool call</tool_use_error>".to_string(),
        };
        complete_tool_call_with_error_mode(
            event_tx,
            history,
            &tool_call.tool_call_id,
            &tool_call.tool_name,
            &tool_id,
            &message,
            completion_event_mode,
        )
        .await;
        return None;
    }

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
        complete_tool_call_with_error_mode(
            event_tx,
            history,
            &tool_call.tool_call_id,
            &tool_call.tool_name,
            &tool_id,
            &message,
            completion_event_mode,
        )
        .await;
        return None;
    }

    Some(PreparedToolCall { tool_id, tool })
}
