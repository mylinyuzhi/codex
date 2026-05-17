//! Small stateless helpers used by the session loop.
//!
//! Extracted from `engine.rs` to keep that module focused on orchestration.
//! All functions here are pure or I/O-free (no awaits except for the queue
//! drain), and easy to unit-test in isolation.

#[cfg(test)]
#[path = "helpers.test.rs"]
mod tests;

use coco_inference::AssistantContentPart;
use coco_inference::FilePart;
use coco_inference::UserContentPart;
use coco_messages::AssistantContent;
use coco_messages::AttachmentMessage;
use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_messages::create_error_tool_result;
use coco_messages::wrapping::wrap_in_system_reminder;
use coco_system_reminder::wrap_command_text;
use coco_types::AttachmentKind;
use coco_types::ToolId;

use crate::BudgetTracker;
use crate::command_queue::CommandQueue;
use crate::command_queue::QueuePriority;
use crate::command_queue::QueuedCommand;
use crate::emit::emit_protocol;
use crate::emit::emit_stream;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ToolCompletionEventMode {
    Emit,
    Defer,
}

/// Convert between the two-name alias for `AssistantContent`.
///
/// `coco_messages::AssistantContent` and `coco_inference::AssistantContentPart`
/// are the same type re-exported under two aliases (see `coco-types` re-export
/// section). This wrapper exists only to make the conversion intent explicit.
pub(crate) fn convert_to_assistant_content(part: AssistantContentPart) -> AssistantContent {
    part
}

/// Drain queued commands up to `max_priority` from `queue` into `history` as
/// user messages, then emit one `CommandDequeued{id}` per drained item plus
/// a `QueueStateChanged{queued: remaining}` summary.
///
/// Shared by the mid-turn `Now`-only checkpoint and the end-of-turn full drain.
///
/// The TUI's local queued-commands display
/// (`SessionState::queued_commands`) keys off the per-item `CommandDequeued`
/// for ordered removal, with `QueueStateChanged` as a count reconciliation
/// safety net. SDK clients can do the same — every queue entry has a
/// stable [`coco_query::QueuedCommand::id`] minted at construction so the
/// `CommandQueued` / `CommandDequeued` event pair is well-formed.
pub async fn drain_command_queue_into_history(
    queue: &CommandQueue,
    history: &mut MessageHistory,
    event_tx: &Option<tokio::sync::mpsc::Sender<coco_types::CoreEvent>>,
    max_priority: QueuePriority,
    agent_id: Option<&str>,
) {
    let queued = queue
        .get_commands_by_max_priority(max_priority, agent_id)
        .await;
    if queued.is_empty() {
        return;
    }
    let ids_to_remove: Vec<uuid::Uuid> = queued.iter().map(|c| c.id).collect();
    for cmd in &queued {
        history.push(queued_command_to_attachment(cmd));
    }
    queue.remove_by_ids(&ids_to_remove).await;
    for id in &ids_to_remove {
        let _ = emit_protocol(
            event_tx,
            crate::ServerNotification::CommandDequeued { id: id.to_string() },
        )
        .await;
    }
    let remaining = queue.len().await as i32;
    let _ = emit_protocol(
        event_tx,
        crate::ServerNotification::QueueStateChanged { queued: remaining },
    )
    .await;
}

/// Convert a [`QueuedCommand`] drained from the queue into a model-bound
/// `AttachmentMessage` of kind [`AttachmentKind::QueuedCommand`].
///
/// Two-step wrapping (TS parity):
///
/// 1. [`wrap_command_text`] adds origin-specific framing prose
///    ("The user sent a new message while you were working:" /
///    "The coordinator sent a message…" / etc.), TS
///    `wrapCommandText` (`messages.ts:5496-5512`).
/// 2. [`wrap_in_system_reminder`] wraps the result in
///    `<system-reminder>…</system-reminder>` so the model recognises
///    the entry as a system-injected interruption rather than a fresh
///    user message — TS `wrapMessagesInSystemReminder`
///    (`messages.ts:3097`) called from
///    `normalizeAttachmentForAPI`'s `case 'queued_command':` branch
///    (`messages.ts:3739`).
///
/// Image attachments (mid-turn screenshot pastes) ride along as
/// additional `UserContentPart`s after the wrapped text, matching TS
/// `getQueuedCommandAttachments` (`attachments.ts:1062-1075`) which
/// appends image blocks after the text. Only the text gets the
/// system-reminder wrap; image bytes ride alongside untouched.
///
/// Lands as `Message::Attachment` with `kind = QueuedCommand`. The
/// kind threads through to UI rendering
/// (`AttachmentKind::renders_in_transcript`) and to the API
/// normalization path that surfaces the wrapped text to the model.
pub fn queued_command_to_attachment(cmd: &QueuedCommand) -> Message {
    let framed = wrap_command_text(&cmd.prompt, cmd.origin.as_ref());
    let wrapped = wrap_in_system_reminder(&framed);
    let llm_message = if cmd.images.is_empty() {
        LlmMessage::user_text(wrapped)
    } else {
        let mut parts: Vec<UserContentPart> = vec![UserContentPart::text(wrapped)];
        for img in &cmd.images {
            parts.push(UserContentPart::File(FilePart::image_base64(
                img.data_base64.clone(),
                img.media_type.clone(),
            )));
        }
        LlmMessage::user(parts)
    };
    Message::Attachment(AttachmentMessage::api(
        AttachmentKind::QueuedCommand,
        llm_message,
    ))
}

/// Whether the current budget state warrants forcing another turn.
///
/// TS: `query/tokenBudget.ts:64` — continue when under 90% of budget AND not
/// in diminishing-returns territory (continuation_count < 3).
pub(crate) fn should_continue_for_budget(budget: &BudgetTracker) -> bool {
    let Some(max) = budget.max_tokens else {
        return false;
    };
    if max <= 0 {
        return false;
    }
    let pct = budget.total_tokens() as f64 / max as f64;
    pct < 0.9 && budget.continuation_count() < 3
}

pub(crate) fn budget_pct_used(budget: &BudgetTracker) -> i32 {
    match budget.max_tokens {
        Some(max) if max > 0 => ((budget.total_tokens() as f64 / max as f64) * 100.0) as i32,
        _ => 0,
    }
}

/// Build the user-facing assistant message for an abnormal-stop_reason
/// turn — mirrors TS `services/api/claude.ts:2258-2292` and
/// `services/api/errors.ts:1184-1207` (`getErrorMessageIfRefusal`).
///
/// Returned message has empty content (the partial real response was
/// already pushed) and `api_error.message` carrying the human-readable
/// explanation. The typed [`coco_messages::StopReason`] is the
/// canonical 8-variant `UnifiedFinishReason` — `ContextWindowExceeded`
/// is a first-class variant (no raw-string sniffing needed). Message
/// text stays provider-agnostic so it covers the multi-LLM unified
/// bucket (Anthropic refusal, OpenAI content_filter, Google SAFETY /
/// RECITATION → coco-rs `ContentFilter`).
pub(crate) fn build_abnormal_stop_api_error_message(
    parsed: coco_messages::StopReason,
    effective_max_tokens: Option<i64>,
) -> coco_messages::Message {
    use coco_messages::StopReason;
    const PREFIX: &str = "API Error";
    let text = match parsed {
        StopReason::ContextWindowExceeded => {
            format!("{PREFIX}: The model has reached its context window limit.")
        }
        StopReason::MaxTokens => match effective_max_tokens {
            Some(n) if n > 0 => format!(
                "{PREFIX}: Model response exceeded the {n} output token maximum. \
                 To increase, set `max_output_tokens` in settings.json or via `--max-tokens`."
            ),
            _ => format!("{PREFIX}: Model response exceeded the configured output token maximum."),
        },
        StopReason::ContentFilter => format!(
            "{PREFIX}: Model declined to respond — the request appears to violate the \
             provider's content policy or safety filter. Try rephrasing the request or \
             start a new session."
        ),
        other => format!(
            "{PREFIX}: Turn ended on stop_reason={}.",
            other.as_wire_str()
        ),
    };
    coco_messages::create_assistant_error_message(&text, None)
}

/// Map `HookOutcome` to the protocol-layer `HookOutcomeStatus`.
/// Treats Blocking as Error since blocking is a user-visible failure from the
/// SDK consumer's perspective.
pub(crate) fn hook_outcome_to_status(
    outcome: coco_types::HookOutcome,
) -> coco_types::HookOutcomeStatus {
    match outcome {
        coco_types::HookOutcome::Success => coco_types::HookOutcomeStatus::Success,
        coco_types::HookOutcome::Blocking => coco_types::HookOutcomeStatus::Error,
        coco_types::HookOutcome::NonBlockingError => coco_types::HookOutcomeStatus::Error,
        coco_types::HookOutcome::Cancelled => coco_types::HookOutcomeStatus::Cancelled,
    }
}

/// Extract the last assistant text from message history.
pub(crate) fn extract_last_assistant_text(history: &MessageHistory) -> String {
    history
        .messages
        .iter()
        .rev()
        .find_map(|m| match m {
            Message::Assistant(a) => match &a.message {
                LlmMessage::Assistant { content, .. } => content.iter().find_map(|c| {
                    if let AssistantContent::Text(t) = c {
                        Some(t.text.clone())
                    } else {
                        None
                    }
                }),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_default()
}

/// Complete a committed tool call with a model-visible error result.
///
/// JSON parse failures never reach this helper because they are dropped before
/// the assistant message is committed. Every committed early-return path should
/// use this so the stream event and history pair stay in sync.
///
/// **Streaming-mode note (I1 ordering).** In the streaming agent loop, the
/// assistant message is committed *after* this helper runs (when the stream
/// hits `Finish`). For multi-tool streams where one call passes preparation
/// and another fails, the failing call's error `tool_result` lands in
/// `history` at index N while the assistant message lands at N+1 — a
/// `user(tool_result) → assistant(tool_use)` ordering that violates Anthropic's
/// adjacency invariant. The streaming path therefore captures synthetic-error
/// rows via [`MessageHistory::drain_pushed_since`] and replays them as
/// `StreamingHandle::feed_plan(ToolCallPlan::EarlyOutcome(...))` so
/// `commit_flush` surfaces them in the correct post-assistant slot. See
/// [`build_streaming_early_outcome`] for the wrap routine.
pub(crate) async fn complete_tool_call_with_error_mode(
    event_tx: &Option<tokio::sync::mpsc::Sender<coco_types::CoreEvent>>,
    history: &mut MessageHistory,
    tool_call_id: &str,
    tool_name: &str,
    tool_id: &ToolId,
    output: &str,
    event_mode: ToolCompletionEventMode,
) {
    if event_mode == ToolCompletionEventMode::Emit {
        let _delivered = emit_stream(
            event_tx,
            crate::AgentStreamEvent::ToolUseCompleted {
                call_id: tool_call_id.to_string(),
                name: tool_name.to_string(),
                output: output.to_string(),
                is_error: true,
            },
        )
        .await;
    }
    history.push(create_error_tool_result(
        tool_call_id,
        tool_name,
        tool_id.clone(),
        output,
    ));
}

/// Wrap a captured synthetic-error `tool_result` row into an
/// [`UnstampedToolCallOutcome`] so the streaming agent loop can surface it
/// via [`StreamingHandle::feed_plan(ToolCallPlan::EarlyOutcome(...))`].
///
/// The preparer pushes its synthetic-error rows directly to history; the
/// streaming caller drains them via [`MessageHistory::drain_pushed_since`]
/// and uses this routine to re-enqueue them through the same channel as
/// permission-deny / hook-block outcomes. `commit_flush` then commits the
/// outcome's `ordered_messages` *after* the assistant message lands, fixing
/// the tool_use/tool_result adjacency violation that the inline push would
/// otherwise cause.
///
/// `error_kind` is set to [`ToolCallErrorKind::ValidationFailed`] as the
/// catch-all bucket for early-return paths that lost their finer-grained
/// classification during the drain (unknown tool, schema fail, hook block,
/// permission deny all collapse here for streaming). Telemetry that cares
/// about the exact bucket should consume the non-streaming path or the
/// PostToolUseFailure hook channel; this kind is purely a placeholder so
/// `runs_post_tool_use_failure()` stays `false` (TS `:413` parity).
pub(crate) fn build_streaming_early_outcome(
    tool_use_id: &str,
    tool_id: ToolId,
    model_index: usize,
    captured_messages: Vec<Message>,
) -> coco_tool_runtime::UnstampedToolCallOutcome {
    coco_tool_runtime::UnstampedToolCallOutcome {
        tool_use_id: tool_use_id.to_string(),
        tool_id,
        model_index,
        ordered_messages: captured_messages,
        message_path: coco_tool_runtime::ToolMessagePath::EarlyReturn,
        error_kind: Some(coco_tool_runtime::ToolCallErrorKind::ValidationFailed),
        permission_denial: None,
        prevent_continuation: None,
        effects: coco_tool_runtime::ToolSideEffects::none(),
    }
}
