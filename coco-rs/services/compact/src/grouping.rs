//! Message grouping by API round.
//!
//! TS: grouping.ts (64 LOC) — segment messages at API-round boundaries.
//!
//! Groups by **assistant message UUID** (not user messages). A new group starts
//! when an assistant message with a different UUID appears. This correctly
//! handles single-prompt agentic sessions where the entire workload is one
//! human turn with many tool-call rounds.
//!
//! **TS uses `message.id` (API response ID)**, which is shared across streaming
//! chunks from the same response. Rust uses `AssistantMessage.uuid` (internal
//! unique ID). These produce identical results because coco-rs creates exactly
//! one `AssistantMessage` per API round via stream collection. If the message
//! pipeline ever changes to yield multiple `AssistantMessage`s per response,
//! this function must be updated to use a shared response ID instead.
//!
//! When no assistant messages exist, all messages land in a single group
//! (matching TS behavior).

use coco_types::Message;

/// Group messages by API round (assistant UUID boundaries).
///
/// A new group starts when we encounter an assistant message whose UUID
/// differs from the last assistant UUID seen. This keeps all tool_use and
/// tool_result messages from the same API response in one group.
pub fn group_messages_by_api_round(messages: &[Message]) -> Vec<Vec<&Message>> {
    let mut groups: Vec<Vec<&Message>> = Vec::new();
    let mut current_group: Vec<&Message> = Vec::new();
    let mut last_assistant_uuid: Option<uuid::Uuid> = None;

    for msg in messages {
        if let Message::Assistant(asst) = msg {
            let this_uuid = asst.uuid;
            let is_new_round = last_assistant_uuid.is_some_and(|prev| prev != this_uuid);

            if is_new_round && !current_group.is_empty() {
                groups.push(std::mem::take(&mut current_group));
            }
            last_assistant_uuid = Some(this_uuid);
        }

        current_group.push(msg);
    }

    if !current_group.is_empty() {
        groups.push(current_group);
    }

    groups
}

#[cfg(test)]
#[path = "grouping.test.rs"]
mod tests;
