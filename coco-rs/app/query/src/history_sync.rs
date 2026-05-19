//! Engine ↔ TUI/SDK history sync helpers.
//!
//! Pairs every `MessageHistory::push` with a `MessageAppended` event so
//! TUI / SDK consumers can maintain a derived view without recomputing
//! engine-internal state. Centralizes the cancel-marker finalization so
//! `for_tool_use` is computed once at the engine and never recomputed
//! downstream.
//!
//! See `engine-tui-unified-transcript-plan.md` §5.
//!
//! Backward compatibility note: `last_message_is_user_interruption`
//! recognizes both the new `SystemMessage::UserInterruption` form
//! emitted by `finalize_user_cancel` and the legacy
//! `INTERRUPT_MESSAGE` / `INTERRUPT_MESSAGE_FOR_TOOL_USE` text marker
//! that older JSONL transcripts may contain. Dedup works on both forms
//! across resume boundaries.

use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_messages::SystemMessage;
use coco_messages::create_user_interruption_system_message;
use coco_types::CoreEvent;
use coco_types::ServerNotification;
use tokio::sync::mpsc::Sender;

use crate::emit::emit_protocol;

/// Push `msg` into `history` and emit a typed `MessageAppended` protocol
/// notification. The notification clones the message so consumers
/// receive a stable copy independent of the history's storage.
pub async fn history_push_and_emit(
    history: &mut MessageHistory,
    msg: Message,
    event_tx: &Option<Sender<CoreEvent>>,
) {
    let notif_msg = msg.clone();
    history.push(msg);
    let _delivered = emit_protocol(
        event_tx,
        ServerNotification::MessageAppended { message: notif_msg },
    )
    .await;
}

/// Single writer for the user-cancel marker. Reads `in_flight_tool_calls`
/// from the engine (which holds the authoritative view of running tool
/// state at the cancel checkpoint) and pushes a typed
/// `SystemMessage::UserInterruption`; downstream consumers read
/// `for_tool_use` from the field and never recompute it.
///
/// Dedup: returns early if the last history entry is already a
/// `UserInterruption` (or legacy text-form marker from a resumed
/// transcript). Mirrors the TS idempotent contract in `query.ts:1015-1052`.
pub async fn finalize_user_cancel(
    history: &mut MessageHistory,
    in_flight_tool_calls: bool,
    event_tx: &Option<Sender<CoreEvent>>,
) {
    if last_message_is_user_interruption(history) {
        return;
    }
    let msg = create_user_interruption_system_message(in_flight_tool_calls);
    history_push_and_emit(history, msg, event_tx).await;
}

/// True when the tail of `history` is a user-cancellation marker —
/// either the typed `SystemMessage::UserInterruption` form emitted by
/// `finalize_user_cancel` or the legacy `INTERRUPT_MESSAGE*` text-form
/// User message preserved for older JSONL transcripts on resume.
pub fn last_message_is_user_interruption(history: &MessageHistory) -> bool {
    let Some(last) = history.messages.last() else {
        return false;
    };
    match last {
        Message::System(SystemMessage::UserInterruption(_)) => true,
        Message::User(user) => {
            let coco_messages::LlmMessage::User { content, .. } = &user.message else {
                return false;
            };
            let [coco_llm_types::UserContentPart::Text(text_part)] = content.as_slice() else {
                return false;
            };
            text_part.text == coco_messages::INTERRUPT_MESSAGE
                || text_part.text == coco_messages::INTERRUPT_MESSAGE_FOR_TOOL_USE
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "history_sync.test.rs"]
mod tests;
