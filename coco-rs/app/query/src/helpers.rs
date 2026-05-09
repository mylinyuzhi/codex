//! Small stateless helpers used by the session loop.
//!
//! Extracted from `engine.rs` to keep that module focused on orchestration.
//! All functions here are pure or I/O-free (no awaits except for the queue
//! drain), and easy to unit-test in isolation.

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

pub(crate) fn parse_stop_reason(s: &str) -> Option<coco_messages::StopReason> {
    match s {
        "stop" => Some(coco_messages::StopReason::EndTurn),
        "length" => Some(coco_messages::StopReason::MaxTokens),
        "tool-calls" => Some(coco_messages::StopReason::ToolUse),
        _ => None,
    }
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
pub(crate) async fn complete_tool_call_with_error(
    event_tx: &Option<tokio::sync::mpsc::Sender<coco_types::CoreEvent>>,
    history: &mut MessageHistory,
    tool_call_id: &str,
    tool_name: &str,
    tool_id: &ToolId,
    output: &str,
) {
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
    history.push(create_error_tool_result(
        tool_call_id,
        tool_name,
        tool_id.clone(),
        output,
    ));
}
