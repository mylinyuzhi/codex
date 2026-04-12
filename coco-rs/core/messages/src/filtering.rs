//! Message filtering functions — 11 filters for cleaning message lists.

use coco_types::Message;

use crate::predicates;

/// Filter out tombstoned messages.
pub fn filter_tombstones(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| !predicates::is_tombstone(m))
        .cloned()
        .collect()
}

/// Filter out virtual messages (not sent to API).
pub fn filter_virtual(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| !predicates::is_virtual_message(m))
        .cloned()
        .collect()
}

/// Filter out progress messages (UI-only).
pub fn filter_progress(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| !predicates::is_progress_message(m))
        .cloned()
        .collect()
}

/// Filter out meta messages (system-injected, hidden from UI).
pub fn filter_meta(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| !predicates::is_meta_message(m))
        .cloned()
        .collect()
}

/// Filter out tool use summary messages.
pub fn filter_tool_use_summaries(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| !predicates::is_tool_use_summary(m))
        .cloned()
        .collect()
}

/// Filter out API error system messages.
pub fn filter_api_errors(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| !predicates::is_api_error_message(m))
        .cloned()
        .collect()
}

/// Filter to only user-visible messages (for UI display).
/// Removes: meta, virtual, progress, tombstone, tool_use_summary.
pub fn filter_for_display(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| {
            !predicates::is_meta_message(m)
                && !predicates::is_virtual_message(m)
                && !predicates::is_progress_message(m)
                && !predicates::is_tombstone(m)
                && !predicates::is_tool_use_summary(m)
        })
        .cloned()
        .collect()
}

/// Filter messages to keep only the last N turns.
/// A "turn" is a user message followed by an assistant message.
pub fn keep_last_n_turns(messages: &[Message], n: usize) -> Vec<Message> {
    if n == 0 {
        return Vec::new();
    }

    // Find turn boundaries (user messages)
    let user_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| predicates::is_user_message(m) && !predicates::is_meta_message(m))
        .map(|(i, _)| i)
        .collect();

    if user_indices.len() <= n {
        return messages.to_vec();
    }

    let cutoff = user_indices[user_indices.len() - n];
    messages[cutoff..].to_vec()
}

/// Ensure every tool result has a matching tool call.
/// Removes orphaned tool results.
pub fn filter_orphaned_tool_results(messages: &[Message]) -> Vec<Message> {
    // Collect all tool_use_ids from assistant messages
    let tool_use_ids: std::collections::HashSet<String> = messages
        .iter()
        .filter_map(|m| match m {
            Message::Assistant(a) => match &a.message {
                coco_types::LlmMessage::Assistant { content, .. } => {
                    let ids: Vec<String> = content
                        .iter()
                        .filter_map(|c| match c {
                            coco_types::AssistantContent::ToolCall(tc) => {
                                Some(tc.tool_call_id.clone())
                            }
                            _ => None,
                        })
                        .collect();
                    Some(ids)
                }
                _ => None,
            },
            _ => None,
        })
        .flatten()
        .collect();

    messages
        .iter()
        .filter(|m| match m {
            Message::ToolResult(tr) => tool_use_ids.contains(&tr.tool_use_id),
            _ => true,
        })
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "filtering.test.rs"]
mod tests;
