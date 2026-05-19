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

/// `tracing` target for engine↔TUI/SDK history sync.
///
/// Use this canonical target on every emit so operators can pivot a
/// single filter (`coco::history_sync=debug`) to trace the full
/// authority round-trip without drowning in unrelated `coco_query`
/// chatter.
const HISTORY_SYNC_TARGET: &str = "coco::history_sync";

/// Push `msg` into `history` and emit a typed `MessageAppended` protocol
/// notification. The notification clones the message so consumers
/// receive a stable copy independent of the history's storage.
pub async fn history_push_and_emit(
    history: &mut MessageHistory,
    msg: Message,
    event_tx: &Option<Sender<CoreEvent>>,
) {
    let uuid = msg.uuid().copied();
    let kind = msg.kind();
    let notif_msg = msg.clone();
    history.push(msg);
    tracing::debug!(
        target: HISTORY_SYNC_TARGET,
        ?uuid,
        ?kind,
        history_len = history.len(),
        has_tx = event_tx.is_some(),
        "history append",
    );
    let _delivered = emit_protocol(
        event_tx,
        ServerNotification::MessageAppended { message: notif_msg },
    )
    .await;
}

/// Clear `history` and emit `MessageTruncated { keep_count: 0 }`. The
/// symmetric companion to [`history_push_and_emit`] — every transcript
/// mutation goes through a wire-visible event so the TUI's
/// `TranscriptView` and SDK NDJSON observers stay coherent with engine
/// state. Use this for plan-mode-exit clears and any other
/// "drop entire history" path that should NOT rotate session_id.
///
/// For `/clear` (which rotates session_id), call
/// [`history_clear_and_emit_session_reset`] instead so consumers
/// also rotate `conversation_id` / re-key the prompt cache.
pub async fn history_clear_and_emit(
    history: &mut MessageHistory,
    event_tx: &Option<Sender<CoreEvent>>,
) {
    let removed = history.len();
    history.clear();
    tracing::info!(
        target: HISTORY_SYNC_TARGET,
        removed,
        has_tx = event_tx.is_some(),
        "history cleared (no session rotation)",
    );
    let _delivered = emit_protocol(
        event_tx,
        ServerNotification::MessageTruncated { keep_count: 0 },
    )
    .await;
}

/// Clear `history` and emit `SessionResetForResume { session_id }`. Use
/// for `/clear` paths that rotate the session id — the same event the
/// resume path uses, since TUI / SDK consumers handle both with the
/// same teardown (wipe transcript, clear overlays, re-key
/// `conversation_id`).
pub async fn history_clear_and_emit_session_reset(
    history: &mut MessageHistory,
    new_session_id: String,
    event_tx: &Option<Sender<CoreEvent>>,
) {
    let removed = history.len();
    history.clear();
    tracing::info!(
        target: HISTORY_SYNC_TARGET,
        removed,
        new_session_id = %new_session_id,
        has_tx = event_tx.is_some(),
        "history cleared + session reset",
    );
    let _delivered = emit_protocol(
        event_tx,
        ServerNotification::SessionResetForResume {
            session_id: new_session_id,
        },
    )
    .await;
}

/// Replace `history.messages` wholesale and emit the event burst that
/// makes the swap observable: a `MessageTruncated { keep_count: 0 }`
/// followed by one `MessageAppended` per new message. Used by
/// compaction (partial / session-memory / full / reactive head-trim) so
/// the TUI's derived view tracks the engine-side rewrite.
///
/// Empty `new_messages` is allowed — equivalent to
/// [`history_clear_and_emit`] in that case.
pub async fn history_replace_and_emit(
    history: &mut MessageHistory,
    new_messages: Vec<Message>,
    event_tx: &Option<Sender<CoreEvent>>,
) {
    let removed = history.len();
    let incoming = new_messages.len();
    history.clear();
    tracing::info!(
        target: HISTORY_SYNC_TARGET,
        removed,
        incoming,
        has_tx = event_tx.is_some(),
        "history replace: clear + N appends",
    );
    let _delivered = emit_protocol(
        event_tx,
        ServerNotification::MessageTruncated { keep_count: 0 },
    )
    .await;
    for msg in new_messages {
        let notif_msg = msg.clone();
        history.push(msg);
        let _delivered = emit_protocol(
            event_tx,
            ServerNotification::MessageAppended { message: notif_msg },
        )
        .await;
    }
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
        // Common path on rapid double-Ctrl+C or cancel after a
        // previously-cancelled turn. `debug` (not `warn`) — it's the
        // documented dedup contract, not a bug. Operators chasing
        // "why didn't a second marker appear?" pivot on this line.
        tracing::debug!(
            target: HISTORY_SYNC_TARGET,
            in_flight_tool_calls,
            tail_kind = ?history.last().map(Message::kind),
            "finalize_user_cancel: dedup skipped (tail already an interruption marker)",
        );
        return;
    }
    tracing::info!(
        target: HISTORY_SYNC_TARGET,
        in_flight_tool_calls,
        history_len_before = history.len(),
        "finalize_user_cancel: pushing UserInterruption marker",
    );
    let msg = create_user_interruption_system_message(in_flight_tool_calls);
    history_push_and_emit(history, msg, event_tx).await;
}

/// True when the most recent semantically-meaningful message is a
/// user-cancellation marker — either the typed
/// `SystemMessage::UserInterruption` form emitted by
/// `finalize_user_cancel` or the legacy `INTERRUPT_MESSAGE*` text-form
/// User message preserved for older JSONL transcripts on resume.
///
/// Skips trailing UI-only ephemera (`Progress`, `Tombstone`) when
/// scanning from the tail. A `Progress` row landing between two
/// rapid Ctrl+C presses (e.g. a tool emitting late progress on the
/// cancelled call) must not break dedup: the second cancel would
/// otherwise see the Progress as `last()` and push a duplicate
/// `UserInterruption`.
pub fn last_message_is_user_interruption(history: &MessageHistory) -> bool {
    for msg in history.as_slice().iter().rev() {
        match msg {
            Message::Progress(_) | Message::Tombstone(_) => continue,
            Message::System(SystemMessage::UserInterruption(_)) => return true,
            Message::User(user) => {
                let coco_messages::LlmMessage::User { content, .. } = &user.message else {
                    return false;
                };
                let [coco_llm_types::UserContentPart::Text(text_part)] = content.as_slice() else {
                    return false;
                };
                return text_part.text == coco_messages::INTERRUPT_MESSAGE
                    || text_part.text == coco_messages::INTERRUPT_MESSAGE_FOR_TOOL_USE;
            }
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
#[path = "history_sync.test.rs"]
mod tests;
