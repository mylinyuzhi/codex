use crate::AssistantContent;
use crate::LlmMessage;
use crate::Message;
use crate::MessageKind;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

/// In-memory message history with turn tracking.
///
/// The backing storage is `pub(crate)` so internal helpers can do
/// bulk operations cheaply; external callers go through the
/// controlled API ([`push`](Self::push), [`truncate`](Self::truncate),
/// [`clear`](Self::clear), [`iter`](Self::iter), [`as_slice`](Self::as_slice),
/// [`first`](Self::first), [`last`](Self::last), [`get`](Self::get))
/// or the explicit escape hatch
/// [`messages_mut`](Self::messages_mut) for raw in-place mutation
/// (which obligates the caller to call [`rebuild_index`](Self::rebuild_index)
/// and emit `MessageTruncated`/`MessageAppended` events).
///
/// Restricting direct field access enforces I-1 from
/// `docs/coco-rs/engine-tui-unified-transcript-plan.md`: every
/// transcript mutation must be observable via the wire-level event
/// stream so TUI / SDK consumers stay coherent with engine state.
#[derive(Debug, Default)]
pub struct MessageHistory {
    pub(crate) messages: Vec<Message>,
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

    /// Raw mutable access to the backing message vector. **Bypasses
    /// the controlled API.** Callers MUST:
    ///
    /// 1. Call [`rebuild_index`](Self::rebuild_index) after any
    ///    structural change (push / insert / remove / truncate /
    ///    reorder).
    /// 2. Emit the matching `ServerNotification` events
    ///    (`MessageAppended` for new entries, `MessageTruncated` for
    ///    removals) so TUI / SDK consumers stay coherent.
    ///
    /// Reserved for in-place compaction passes (`coco_compact::*`)
    /// that operate on the vector representation and re-publish via
    /// `history_replace_and_emit` afterwards. New call sites should
    /// prefer [`push`](Self::push), [`truncate`](Self::truncate),
    /// [`clear`](Self::clear), or
    /// `coco_query::history_sync::history_push_and_emit`.
    pub fn messages_mut(&mut self) -> &mut Vec<Message> {
        &mut self.messages
    }

    /// Read-only iterator over the underlying messages.
    pub fn iter(&self) -> std::slice::Iter<'_, Message> {
        self.messages.iter()
    }

    /// First message, if any.
    pub fn first(&self) -> Option<&Message> {
        self.messages.first()
    }

    /// Last message, if any.
    pub fn last(&self) -> Option<&Message> {
        self.messages.last()
    }

    /// Indexed access; returns `None` if out of bounds.
    pub fn get(&self, idx: usize) -> Option<&Message> {
        self.messages.get(idx)
    }

    /// Cloned snapshot of the underlying messages.
    pub fn to_vec(&self) -> Vec<Message> {
        self.messages.clone()
    }

    /// Drain messages pushed since `since_len` and rebuild the UUID index.
    ///
    /// Used by the streaming agent loop to capture synthetic-error
    /// `tool_result` rows that the preparer pushed during a streamed
    /// `ToolCallEnd` so they can be re-committed *after* the assistant
    /// message lands — preserving Anthropic's `tool_use` / `tool_result`
    /// adjacency invariant (I1). Without this, an early-error tool call
    /// in the middle of a multi-tool stream would land at history index
    /// `N` while the assistant message lands at `N+1`, producing a
    /// malformed user→assistant ordering.
    ///
    /// The index is fully rebuilt because mid-vec drain shifts every
    /// later message's positional index and the `HashMap<Uuid, usize>`
    /// must point at the post-drain offsets (or contain no stale keys).
    /// Cost is O(N) over all remaining messages, but the streaming path
    /// only calls this on synthetic-error capture (rare) so the
    /// amortized impact is negligible.
    pub fn drain_pushed_since(&mut self, since_len: usize) -> Vec<Message> {
        if since_len >= self.messages.len() {
            return Vec::new();
        }
        let captured: Vec<Message> = self.messages.drain(since_len..).collect();
        self.rebuild_index();
        captured
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
    /// Walks the message content in emission order and emits text parts
    /// separated by `\n`, with `[tool: <name>]` placeholder lines for
    /// non-text parts. Preserves the tool-call boundary so consumers
    /// (Stop hook input, memory extraction, etc.) see the structural
    /// transitions instead of a silently-concatenated blob.
    ///
    /// Before the multi-part streaming reconstruction landed, this
    /// method always saw `Vec<Text(combined)>` and the empty-string
    /// join was correct. With per-part `provider_metadata` now
    /// preserved, multiple `Text` parts can appear interleaved with
    /// `ToolCall`s — the placeholder keeps downstream semantics intact.
    pub fn last_assistant_text(&self) -> Option<String> {
        self.messages.iter().rev().find_map(|msg| match msg {
            Message::Assistant(a) => match &a.message {
                LlmMessage::Assistant { content, .. } => {
                    let mut chunks: Vec<String> = Vec::new();
                    for c in content {
                        match c {
                            AssistantContent::Text(t) if !t.text.is_empty() => {
                                chunks.push(t.text.clone());
                            }
                            AssistantContent::ToolCall(tc) => {
                                chunks.push(format!("[tool: {}]", tc.tool_name));
                            }
                            _ => {}
                        }
                    }
                    if chunks.is_empty() {
                        None
                    } else {
                        Some(chunks.join("\n"))
                    }
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

    /// Truncate the history to the first `keep_count` messages.
    /// Indices `>= keep_count` are discarded and the UUID index
    /// rebuilt.
    ///
    /// Used by the engine `Rewind` handler (both explicit and
    /// auto-restore modes per
    /// `engine-tui-unified-transcript-plan.md` §4.2) — the resulting
    /// length is the new authoritative history size that the engine
    /// then broadcasts via `ServerNotification::MessageTruncated`.
    ///
    /// No-op when `keep_count >= len()`.
    pub fn truncate(&mut self, keep_count: usize) {
        if keep_count >= self.messages.len() {
            return;
        }
        self.messages.truncate(keep_count);
        self.rebuild_index();
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

    /// Rebuild the UUID index from the current messages. Must be
    /// called by any caller that mutated the underlying vector via
    /// [`messages_mut`](Self::messages_mut) in a way that changed
    /// indices (push/insert/remove/reorder/truncate).
    pub fn rebuild_index(&mut self) {
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
