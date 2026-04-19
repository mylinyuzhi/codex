//! Small stateless helpers used by the session loop.
//!
//! Extracted from `engine.rs` to keep that module focused on orchestration.
//! All functions here are pure or I/O-free (no awaits except for the queue
//! drain), and easy to unit-test in isolation.

use coco_messages::MessageHistory;
use coco_types::AssistantContent;
use coco_types::LlmMessage;
use coco_types::Message;
use coco_types::ToolId;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::ToolResultContent;

use crate::BudgetTracker;
use crate::command_queue::CommandQueue;
use crate::command_queue::QueuePriority;
use crate::emit::emit_protocol;

/// Convert between the two-name alias for `AssistantContent`.
///
/// `coco_types::AssistantContent` and `vercel_ai_provider::AssistantContentPart`
/// are the same type re-exported under two aliases (see `coco-types` re-export
/// section). This wrapper exists only to make the conversion intent explicit.
pub(crate) fn convert_to_assistant_content(part: AssistantContentPart) -> AssistantContent {
    part
}

/// Drain queued commands up to `max_priority` from `queue` into `history` as
/// user messages, then emit `QueueStateChanged` with the remaining count.
///
/// Shared by the mid-turn `Now`-only checkpoint and the end-of-turn full drain.
pub(crate) async fn drain_command_queue_into_history(
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
    let prompts_to_remove: Vec<String> = queued.iter().map(|c| c.prompt.clone()).collect();
    for cmd in queued {
        history.push(coco_messages::create_user_message(&cmd.prompt));
    }
    queue.remove(&prompts_to_remove).await;
    let remaining = queue.len().await as i32;
    let _ = emit_protocol(
        event_tx,
        crate::ServerNotification::QueueStateChanged { queued: remaining },
    )
    .await;
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

pub(crate) fn parse_stop_reason(s: &str) -> Option<coco_types::StopReason> {
    match s {
        "stop" => Some(coco_types::StopReason::EndTurn),
        "length" => Some(coco_types::StopReason::MaxTokens),
        "tool-calls" => Some(coco_types::StopReason::ToolUse),
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

/// Build a tool error message for history.
pub(crate) fn make_tool_error_message(
    tool_call_id: &str,
    tool_name: &str,
    tool_id: &ToolId,
    message: &str,
) -> Message {
    Message::ToolResult(coco_types::ToolResultMessage {
        uuid: uuid::Uuid::new_v4(),
        message: LlmMessage::Tool {
            content: vec![coco_types::ToolContent::ToolResult(
                coco_types::ToolResultContent {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    output: ToolResultContent::error_text(message.to_string()),
                    is_error: true,
                    provider_metadata: None,
                },
            )],
            provider_options: None,
        },
        tool_use_id: tool_call_id.to_string(),
        tool_id: tool_id.clone(),
        is_error: true,
    })
}
