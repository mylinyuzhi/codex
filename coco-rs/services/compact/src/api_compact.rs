//! API-level micro-compaction: clear tool uses and thinking blocks via
//! native API content editing (no LLM call required).
//!
//! TS: apiMicrocompact.ts — `clear_tool_uses`, `clear_thinking`.
//!
//! These functions operate directly on message content blocks, removing
//! tool-use inputs or reasoning blocks from older messages while preserving
//! the message structure required by the API.

use coco_types::AssistantContent;
use coco_types::LlmMessage;
use coco_types::Message;

use crate::types::MicrocompactResult;

/// Clear tool-use content blocks from older assistant messages.
///
/// Walks assistant messages oldest-first and replaces tool-call `input`
/// fields with an empty JSON object, preserving the tool-call structure
/// so the API still sees matching call/result pairs. Skips the most
/// recent `keep_recent_count` assistant messages and any tools named
/// in `exclude_tools`.
pub fn clear_tool_uses(
    messages: &mut [Message],
    keep_recent_count: usize,
    exclude_tools: &[String],
) -> MicrocompactResult {
    let assistant_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(i, m)| matches!(m, Message::Assistant(_)).then_some(i))
        .collect();

    let cutoff_count = assistant_indices.len().saturating_sub(keep_recent_count);
    let indices_to_clear = &assistant_indices[..cutoff_count];

    let mut cleared: i32 = 0;
    let mut tokens_freed: i64 = 0;

    for &idx in indices_to_clear {
        let Message::Assistant(ref mut asst) = messages[idx] else {
            continue;
        };

        let LlmMessage::Assistant {
            ref mut content, ..
        } = asst.message
        else {
            continue;
        };

        for part in content.iter_mut() {
            let AssistantContent::ToolCall(ref mut tc) = *part else {
                continue;
            };

            if exclude_tools.iter().any(|ex| tc.tool_name.contains(ex)) {
                continue;
            }

            // Only clear if input is non-trivial
            let input_str = tc.input.to_string();
            let est_tokens = (input_str.len() as i64) / 4;
            if est_tokens <= 5 {
                continue;
            }

            tc.input = serde_json::Value::Object(serde_json::Map::new());
            tokens_freed += est_tokens;
            cleared += 1;
        }
    }

    MicrocompactResult {
        messages_cleared: cleared,
        tokens_saved_estimate: tokens_freed,
        was_time_triggered: false,
    }
}

/// Clear reasoning/thinking blocks from all assistant messages.
///
/// Removes `Reasoning` content parts entirely, freeing tokens used by
/// chain-of-thought that is no longer needed. Preserves text and
/// tool-call parts.
pub fn clear_thinking(messages: &mut [Message]) -> MicrocompactResult {
    let mut cleared: i32 = 0;
    let mut tokens_freed: i64 = 0;

    for msg in messages.iter_mut() {
        let Message::Assistant(ref mut asst) = *msg else {
            continue;
        };

        let LlmMessage::Assistant {
            ref mut content, ..
        } = asst.message
        else {
            continue;
        };

        let original_len = content.len();
        let mut removed_tokens: i64 = 0;

        content.retain(|part| match part {
            AssistantContent::Reasoning(r) => {
                removed_tokens += (r.text.len() as i64) / 4;
                false
            }
            _ => true,
        });

        let removed = (original_len - content.len()) as i32;
        if removed > 0 {
            cleared += removed;
            tokens_freed += removed_tokens;
        }
    }

    MicrocompactResult {
        messages_cleared: cleared,
        tokens_saved_estimate: tokens_freed,
        was_time_triggered: false,
    }
}

#[cfg(test)]
#[path = "api_compact.test.rs"]
mod tests;
