//! Build [`UnstampedToolCallOutcome`] from a single tool call's raw
//! result, running post-hooks and flattening `ToolMessageBuckets` in
//! TS-parity order.
//!
//! This is the `run_one` success/failure tail that follows
//! `tool.execute()`. The preparer (`tool_call_preparer.rs`) owns the
//! pre-execution lifecycle (pre-hook → re-validate → permission);
//! everything after the tool returns flows through here.

use std::sync::Arc;

use coco_hooks::HookExecutionEvent;
use coco_hooks::HookRegistry;
use coco_hooks::orchestration::OrchestrationContext;
use coco_messages::create_error_tool_result;
use coco_messages::create_tool_result_message;
use coco_system_reminder::AttachmentType as ReminderAttachmentType;
use coco_system_reminder::SystemReminder;
use coco_system_reminder::inject_reminders;
use coco_tool::Tool;
use coco_tool::ToolCallErrorKind;
use coco_tool::ToolMessagePath;
use coco_tool::ToolSideEffects;
use coco_tool::UnstampedToolCallOutcome;
use coco_types::Message;
use coco_types::ToolId;
use coco_types::ToolResult;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::warn;

use crate::hook_controller::HookController;
use crate::tool_message::ToolMessageBuckets;
use crate::tool_message::ToolMessageOrder;
use crate::tool_message::ToolMessagePath as RunnerMessagePath;

/// Inputs `run_one` feeds into the outcome builder AFTER the
/// preparer has resolved the effective input and permission.
pub(crate) struct RunOneTail<'a> {
    pub tool_use_id: String,
    pub tool_id: ToolId,
    pub tool_name: String,
    pub model_index: usize,
    pub tool: Arc<dyn Tool>,
    pub effective_input: Value,
    pub execute_result: Result<ToolResult<Value>, coco_tool::ToolError>,
    pub hooks: Option<&'a Arc<HookRegistry>>,
    pub orchestration_ctx: OrchestrationContext,
    pub hook_tx: Option<&'a mpsc::Sender<HookExecutionEvent>>,
}

/// Build an `UnstampedToolCallOutcome` from a completed tool call.
///
/// Runs PostToolUse / PostToolUseFailure hooks, assembles the
/// appropriate `ToolMessageBuckets`, flattens via `ToolMessageOrder`,
/// and packages side-effects into [`ToolSideEffects`] so the
/// scheduler can apply the patch at the correct moment.
///
/// TS parity: this is the success/failure tail of
/// `toolExecution.ts:1478-1737`.
pub(crate) async fn build_outcome_from_execution(args: RunOneTail<'_>) -> UnstampedToolCallOutcome {
    let RunOneTail {
        tool_use_id,
        tool_id,
        tool_name,
        model_index,
        tool,
        effective_input,
        execute_result,
        hooks,
        orchestration_ctx,
        hook_tx,
    } = args;
    let is_mcp = tool.is_mcp();
    let order = ToolMessageOrder::for_tool(&*tool);

    match execute_result {
        Ok(tool_result) => {
            let ToolResult {
                data,
                new_messages,
                app_state_patch,
            } = tool_result;
            let mut output_data = data;

            // PostToolUse runs on the success branch. Output rewrite
            // is MCP-only per TS `toolHooks.ts:145`.
            let post = HookController::new(hooks, orchestration_ctx, hook_tx)
                .run_post_tool_use(&tool_name, &tool_use_id, &effective_input, &output_data)
                .await;
            if is_mcp && let Some(updated) = post.updated_mcp_tool_output {
                output_data = updated;
            }

            let rendered_output = serde_json::to_string(&output_data).unwrap_or_default();
            let tool_result_msg = create_tool_result_message(
                &tool_use_id,
                &tool_name,
                tool_id.clone(),
                &rendered_output,
                /*is_error*/ false,
            );

            // Collect post-hook additional_contexts into message
            // form. TS emits them wrapped via system-reminder; we do
            // the same so the attachment kind + format match the
            // legacy `tool_result_processor` path.
            let post_hook_msgs = render_hook_context_messages(
                &tool_name,
                &post.additional_contexts,
                ReminderAttachmentType::HookAdditionalContext,
            );

            let prevent_attachment = if post.prevent_continuation {
                let reason = post
                    .stop_reason
                    .clone()
                    .unwrap_or_else(|| "PostToolUse hook stopped continuation".into());
                render_hook_stopped_continuation_message(&tool_name, &reason)
            } else {
                None
            };

            let buckets = ToolMessageBuckets {
                pre_hook: Vec::new(),
                tool_result: Some(tool_result_msg),
                new_messages,
                post_hook: post_hook_msgs,
                prevent_continuation_attachment: prevent_attachment,
                path: RunnerMessagePath::Success,
            };
            let ordered_messages = buckets.flatten(order);

            let prevent_reason = post.prevent_continuation.then(|| {
                post.stop_reason
                    .clone()
                    .unwrap_or_else(|| "PostToolUse hook stopped continuation".into())
            });

            UnstampedToolCallOutcome {
                tool_use_id,
                tool_id,
                model_index,
                ordered_messages,
                message_path: ToolMessagePath::Success,
                error_kind: None,
                permission_denial: None,
                prevent_continuation: prevent_reason,
                effects: ToolSideEffects { app_state_patch },
            }
        }
        Err(error) => {
            let error_message = error.to_string();
            let rendered_error = format!("Error: {error_message}");
            warn!(tool = %tool_name, error = %error, "tool execution failed");

            let post = HookController::new(hooks, orchestration_ctx, hook_tx)
                .run_post_tool_use_failure(&tool_name, &effective_input, &error_message)
                .await;

            let tool_result_msg = create_error_tool_result(
                &tool_use_id,
                &tool_name,
                tool_id.clone(),
                &rendered_error,
            );
            let post_hook_msgs = render_hook_context_messages(
                &tool_name,
                &post.additional_contexts,
                ReminderAttachmentType::HookAdditionalContext,
            );

            let buckets = ToolMessageBuckets {
                pre_hook: Vec::new(),
                tool_result: Some(tool_result_msg),
                new_messages: Vec::new(),
                post_hook: post_hook_msgs,
                prevent_continuation_attachment: None,
                path: RunnerMessagePath::Failure,
            };
            let ordered_messages = buckets.flatten(order);

            // Classify cancellation vs other execution errors so the
            // error_kind enum is accurate. The preparer already
            // short-circuits Cancelled-before-execute into an
            // EarlyOutcome, so anything we see here is either a
            // plain execution failure or a mid-execute cancel.
            let error_kind = match &error {
                coco_tool::ToolError::Cancelled => ToolCallErrorKind::ExecutionCancelled,
                _ => ToolCallErrorKind::ExecutionFailed,
            };

            UnstampedToolCallOutcome {
                tool_use_id,
                tool_id,
                model_index,
                ordered_messages,
                message_path: ToolMessagePath::Failure,
                error_kind: Some(error_kind),
                permission_denial: None,
                prevent_continuation: None,
                effects: ToolSideEffects::none(),
            }
        }
    }
}

/// Build an `UnstampedToolCallOutcome` for an EarlyReturn path —
/// unknown tool, schema failure, validation failure, pre-hook block,
/// or permission denial. The runner uses this when it decides pre-
/// execution that no tool run is going to happen, so `run_one` never
/// sees these calls.
#[allow(dead_code)]
pub(crate) fn build_early_outcome(
    tool_use_id: String,
    tool_id: ToolId,
    tool_name: &str,
    model_index: usize,
    error_kind: ToolCallErrorKind,
    synthetic_message: &str,
    permission_denial: Option<coco_types::PermissionDenialInfo>,
) -> UnstampedToolCallOutcome {
    let tool_result_msg =
        create_error_tool_result(&tool_use_id, tool_name, tool_id.clone(), synthetic_message);
    let buckets = ToolMessageBuckets {
        pre_hook: Vec::new(),
        tool_result: Some(tool_result_msg),
        new_messages: Vec::new(),
        post_hook: Vec::new(),
        prevent_continuation_attachment: None,
        path: RunnerMessagePath::EarlyReturn,
    };
    let ordered_messages = buckets.flatten(ToolMessageOrder::NonMcp);
    UnstampedToolCallOutcome {
        tool_use_id,
        tool_id,
        model_index,
        ordered_messages,
        message_path: ToolMessagePath::EarlyReturn,
        error_kind: Some(error_kind),
        permission_denial,
        prevent_continuation: None,
        effects: ToolSideEffects::none(),
    }
}

/// Wrap hook-provided additional_contexts into reminder-injected
/// `Message::Attachment`s. We inject through `inject_reminders` into
/// a throwaway Vec so the resulting attachment kind / wrap format
/// match the legacy path exactly.
fn render_hook_context_messages(
    hook_name: &str,
    additional_contexts: &[String],
    kind: ReminderAttachmentType,
) -> Vec<Message> {
    if additional_contexts.is_empty() {
        return Vec::new();
    }
    let reminders = additional_contexts
        .iter()
        .map(|ctx| SystemReminder::new(kind, format!("{hook_name} hook additional context: {ctx}")))
        .collect();
    let mut scratch: Vec<Message> = Vec::new();
    let _display_only = inject_reminders(reminders, &mut scratch);
    scratch
}

fn render_hook_stopped_continuation_message(hook_name: &str, reason: &str) -> Option<Message> {
    let reminders = vec![SystemReminder::new(
        ReminderAttachmentType::HookStoppedContinuation,
        format!("{hook_name} hook stopped continuation: {reason}"),
    )];
    let mut scratch: Vec<Message> = Vec::new();
    let _display_only = inject_reminders(reminders, &mut scratch);
    scratch.into_iter().next()
}
