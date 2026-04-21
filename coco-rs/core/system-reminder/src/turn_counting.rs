//! Pure helpers for turning [`coco_types::Message`] slices into the scalar
//! counters that generators read from [`GeneratorContext`].
//!
//! These functions are the bridge between raw message history and the
//! pre-computed fields a generator expects — the engine calls them once per
//! turn, stores the results on `GeneratorContext`, and generators read them
//! synchronously.
//!
//! **TS-first semantics** (`getTodoReminderTurnCounts` at
//! `attachments.ts:3212-3264`):
//!
//! - Iterate messages backwards (newest first).
//! - "Assistant turns" exclude thinking-only messages (all-reasoning blocks).
//! - Stop counting once the matching tool_use is found; return the count.
//! - A tool never invoked returns the total number of assistant turns in
//!   history — i.e. "infinitely many turns ago" rounded to session length.
//!
//! [`GeneratorContext`]: crate::GeneratorContext

use coco_types::AssistantContent;
use coco_types::LlmMessage;
use coco_types::Message;
use coco_types::ToolName;

/// Task tools whose **invocation resets the task-reminder silence counter**.
///
/// Matches TS `getTaskReminderTurnCounts` (`attachments.ts:3345-3348`), which
/// treats only `TASK_CREATE_TOOL_NAME` / `TASK_UPDATE_TOOL_NAME` as "task
/// management activity". Read-only tools (`TaskGet`/`List`/`Stop`/`Output`)
/// don't count — using them does not acknowledge new or updated work.
///
/// Callers use this constant instead of a hand-rolled array so adding a new
/// mutation tool in [`ToolName`] flows through automatically.
pub const TASK_MANAGEMENT_TOOLS: &[ToolName] = &[ToolName::TaskCreate, ToolName::TaskUpdate];

/// Count assistant turns back from the end of history until we find an
/// assistant message that invoked *any* of `tools`.
///
/// Returns the count of assistant turns *before* the matching tool use
/// (i.e. if the very last assistant turn invoked the tool, returns 0).
///
/// Skips thinking-only messages — matches TS `isThinkingMessage` skip.
///
/// If the tool is never found, returns the total number of assistant turns
/// in `messages` (capped at `i32::MAX`). Callers treat this as "infinitely
/// many turns ago" since any threshold below session length will pass.
pub fn count_assistant_turns_since_any_tool(messages: &[Message], tools: &[ToolName]) -> i32 {
    let mut count: i32 = 0;
    for msg in messages.iter().rev() {
        let Message::Assistant(a) = msg else {
            continue;
        };
        if is_thinking_only(&a.message) {
            continue;
        }
        if message_invokes_any_tool(&a.message, tools) {
            return count;
        }
        count = count.saturating_add(1);
    }
    count
}

/// Convenience wrapper: single typed tool.
pub fn count_assistant_turns_since_tool(messages: &[Message], tool: ToolName) -> i32 {
    count_assistant_turns_since_any_tool(messages, &[tool])
}

/// Total number of assistant turns (non-thinking) in the history. Useful
/// upper bound for turn-gated logic and for tests.
pub fn total_assistant_turns(messages: &[Message]) -> i32 {
    let mut count: i32 = 0;
    for msg in messages {
        if let Message::Assistant(a) = msg
            && !is_thinking_only(&a.message)
        {
            count = count.saturating_add(1);
        }
    }
    count
}

/// Count **human turns**: non-meta `User` messages in history.
///
/// This is the canonical `turn_number` to feed
/// [`crate::SystemReminderOrchestrator::generate_all`] so plan-mode /
/// auto-mode throttle cadence matches TS exactly (TS counts human turns,
/// not LLM iterations — `attachments.ts:getPlanModeAttachmentTurnCount`).
///
/// Tool-result rounds within one human turn do NOT advance the counter
/// because they aren't new `User` messages — each tool-call iteration
/// shares the originating human turn.
pub fn count_human_turns(messages: &[Message]) -> i32 {
    let mut count: i32 = 0;
    for msg in messages {
        // Post-Phase-2: reminder-injected content is Message::Attachment,
        // so every `Message::User` is a genuine human turn.
        if matches!(msg, Message::User(_)) {
            count = count.saturating_add(1);
        }
    }
    count
}

/// Returns true when this assistant message has content and every content
/// part is a reasoning block. An empty-content message is treated as non-
/// thinking so the count matches TS (TS doesn't skip empty messages).
fn is_thinking_only(msg: &LlmMessage) -> bool {
    let LlmMessage::Assistant { content, .. } = msg else {
        return false;
    };
    if content.is_empty() {
        return false;
    }
    content.iter().all(|c| {
        matches!(
            c,
            AssistantContent::Reasoning(_) | AssistantContent::ReasoningFile(_)
        )
    })
}

fn message_invokes_any_tool(msg: &LlmMessage, tools: &[ToolName]) -> bool {
    let LlmMessage::Assistant { content, .. } = msg else {
        return false;
    };
    content.iter().any(|c| match c {
        AssistantContent::ToolCall(tc) => tools.iter().any(|t| tc.tool_name == t.as_str()),
        _ => false,
    })
}

#[cfg(test)]
#[path = "turn_counting.test.rs"]
mod tests;
