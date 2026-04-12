use coco_types::AssistantContent;
use coco_types::LlmMessage;
use coco_types::Message;
use coco_types::MessageKind;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

/// In-memory message history with turn tracking.
#[derive(Debug, Default)]
pub struct MessageHistory {
    pub messages: Vec<Message>,
    /// Message UUID -> index in messages vec.
    index: HashMap<Uuid, usize>,
}

impl MessageHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, msg: Message) {
        if let Some(uuid) = msg.uuid() {
            self.index.insert(*uuid, self.messages.len());
        }
        self.messages.push(msg);
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn find_by_uuid(&self, uuid: &Uuid) -> Option<&Message> {
        self.index.get(uuid).and_then(|&i| self.messages.get(i))
    }

    /// Get messages as a slice.
    pub fn as_slice(&self) -> &[Message] {
        &self.messages
    }

    /// Return the text from the last Assistant message, if any.
    ///
    /// Concatenates all `Text` content parts in the last assistant message.
    pub fn last_assistant_text(&self) -> Option<String> {
        self.messages.iter().rev().find_map(|msg| match msg {
            Message::Assistant(a) => match &a.message {
                LlmMessage::Assistant { content, .. } => {
                    let text: String = content
                        .iter()
                        .filter_map(|c| match c {
                            AssistantContent::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    if text.is_empty() { None } else { Some(text) }
                }
                _ => None,
            },
            _ => None,
        })
    }

    /// Count messages of a given kind.
    pub fn count_by_kind(&self, kind: MessageKind) -> usize {
        self.messages.iter().filter(|m| m.kind() == kind).count()
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.index.clear();
    }

    /// Truncate to keep only the last `n` messages.
    ///
    /// Rebuilds the UUID index after truncation.
    pub fn truncate_keep_last(&mut self, n: usize) {
        if n >= self.messages.len() {
            return;
        }
        let start = self.messages.len() - n;
        self.messages.drain(..start);
        self.rebuild_index();
    }

    /// Rebuild the UUID index from the current messages.
    fn rebuild_index(&mut self) {
        self.index.clear();
        for (i, msg) in self.messages.iter().enumerate() {
            if let Some(uuid) = msg.uuid() {
                self.index.insert(*uuid, i);
            }
        }
    }
}

#[cfg(test)]
#[path = "history.test.rs"]
mod tests;

/// Persisted history entry (for session replay).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub display: String,
    pub timestamp: String,
    pub project: String,
    pub session_id: String,
}
