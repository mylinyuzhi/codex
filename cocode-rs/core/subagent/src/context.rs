use std::collections::HashSet;

use cocode_message::TrackedMessage;
use cocode_message::has_tool_use;
use cocode_message::is_assistant_message;
use serde::Deserialize;
use serde::Serialize;

/// Context linking a child subagent session back to its parent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildToolUseContext {
    /// Session ID of the parent agent that spawned this child.
    pub parent_session_id: String,

    /// Session ID assigned to the child subagent.
    pub child_session_id: String,

    /// The turn number in the parent at which the child was forked.
    pub forked_from_turn: i32,
}

/// Filter orphaned tool_use messages from a forked conversation history.
///
/// Removes assistant messages containing tool_use blocks that have no
/// corresponding tool_result blocks in the conversation. This is used
/// when forking context for subagents (analogous to Claude Code's
/// `cloneForkContext`).
///
/// A tool_use is considered orphaned if no subsequent message contains
/// a tool_result with a matching `tool_use_id`.
///
/// ## Two-pass algorithm
///
/// This function uses two passes over the message list:
///
/// 1. **Pass 1 (collect resolved IDs):** Scan all messages to build a set of
///    tool_call_ids that have corresponding results. This must be a full scan
///    because tool results may appear *before* their corresponding tool_use in
///    the message list (e.g., when messages are reordered during compaction or
///    fork context reconstruction).
///
/// 2. **Pass 2 (filter):** Iterate messages again, keeping only those whose
///    tool_use blocks all have entries in the resolved set from pass 1.
pub fn filter_orphaned_tool_uses(messages: &[TrackedMessage]) -> Vec<TrackedMessage> {
    // Pass 1: Collect all tool_call_ids that have corresponding results.
    // A full scan is needed because results may appear before their uses in
    // the message list (e.g., after compaction or fork context reconstruction).
    // Results can appear in:
    // 1. Assistant messages (via extract_tool_result on AssistantContentPart)
    // 2. Tool messages (via MessageSource::Tool { call_id })
    let mut resolved_ids: HashSet<String> = HashSet::new();

    for msg in messages {
        // Check assistant messages for inline tool results
        if let cocode_message::LanguageModelMessage::Assistant { content, .. } = &msg.inner {
            for part in content {
                if let Some((id, _, _)) = cocode_message::extract_tool_result(part) {
                    resolved_ids.insert(id.to_string());
                }
            }
        }

        // Tool messages carry the call_id in their tracked source
        if let cocode_message::MessageSource::Tool { call_id } = &msg.source {
            resolved_ids.insert(call_id.clone());
        }
    }

    // Pass 2: Filter messages. Keep all non-assistant messages and assistant
    // messages without tool_use. For assistant messages with tool_use, keep
    // only if ALL tool_call_ids are resolved in the set from pass 1.
    messages
        .iter()
        .filter(|msg| {
            if !is_assistant_message(&msg.inner) || !has_tool_use(&msg.inner) {
                return true;
            }

            // Get all tool_call_ids from this assistant message
            let tool_calls = cocode_message::get_tool_calls(&msg.inner);
            if tool_calls.is_empty() {
                return true;
            }

            // Keep the message only if ALL its tool_use blocks have results
            tool_calls
                .iter()
                .all(|tc| resolved_ids.contains(&tc.tool_call_id))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
