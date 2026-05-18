//! Tests for `on_turn_interrupted` auto-restore decision matrix.
//!
//! Mirrors TS `REPL.tsx:3010-3022` (`signal.reason === 'user-cancel'`
//! + idle guards + `messagesAfterAreOnlySynthetic`).
//!
//! The auto-restore decision is now centralised on this event —
//! removed from `on_turn_completed` and from `update::exit::on_interrupt`.

use pretty_assertions::assert_eq;

use coco_types::CancelReason;
use coco_types::TurnInterruptedParams;

use super::on_turn_interrupted;
use crate::state::AppState;
use crate::state::ChatMessage;
use crate::state::ModalState;

// ── Helpers ─────────────────────────────────────────────────────

fn user_cancel() -> TurnInterruptedParams {
    TurnInterruptedParams {
        turn_id: None,
        reason: Some(CancelReason::UserCancel),
    }
}

fn system_preempt() -> TurnInterruptedParams {
    TurnInterruptedParams {
        turn_id: None,
        reason: Some(CancelReason::SystemPreempt),
    }
}

fn legacy_no_reason() -> TurnInterruptedParams {
    TurnInterruptedParams {
        turn_id: None,
        reason: None,
    }
}

/// Idle session with a single user message and a synthetic (empty)
/// assistant message — the "lossless tail" auto-restore scenario.
fn idle_with_lossless_tail(user_id: &str, user_text: &str) -> AppState {
    let mut s = AppState::new();
    s.session
        .add_message(ChatMessage::user_text(user_id, user_text));
    s.session.add_message(ChatMessage::assistant_text("a1", ""));
    s
}

/// Idle session with a user message followed by a real assistant
/// response — auto-restore must be suppressed.
fn idle_with_meaningful_tail() -> AppState {
    let mut s = AppState::new();
    s.session.add_message(ChatMessage::user_text("u1", "ask"));
    s.session
        .add_message(ChatMessage::assistant_text("a1", "actual reply text"));
    s
}

// ── Auto-restore matrix ─────────────────────────────────────────

#[test]
fn user_cancel_with_lossless_tail_restores() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    on_turn_interrupted(&mut state, user_cancel());

    // Synthetic assistant tail truncated; user message also removed
    // because `truncate(idx)` drops the user message itself — its text
    // is what gets popped back into the input. No InterruptionMarker
    // is appended here because auto-restore already pulled the prompt
    // back into the input (TS parity).
    assert!(state.session.messages.is_empty());
    assert_eq!(state.ui.input.text(), "original prompt");
    // Fresh conversation_id assigned so next turn's cache key is new.
    assert!(state.session.conversation_id.is_some());
}

/// Returns true if `on_turn_interrupted` resulted in a different
/// message count than before. Used as the "did restore happen?" probe
/// alongside checking for an InterruptionMarker tail.
fn restored(before_len: usize, state: &AppState) -> bool {
    state.session.messages.len() != before_len
}

/// Returns true if the message list ends with an `InterruptionMarker`.
fn ends_with_interrupt_marker(state: &AppState) -> bool {
    matches!(
        state.session.messages.last().map(|m| &m.content),
        Some(crate::state::session::MessageContent::InterruptionMarker { .. })
    )
}

#[test]
fn user_cancel_with_meaningful_tail_does_not_restore() {
    let mut state = idle_with_meaningful_tail();
    let before_len = state.session.messages.len();
    on_turn_interrupted(&mut state, user_cancel());

    // Auto-restore suppressed (meaningful tail), so the marker is
    // appended instead — TS InterruptedByUser path.
    assert!(restored(before_len, &state));
    assert!(ends_with_interrupt_marker(&state));
    assert_eq!(state.ui.input.text(), "", "input unchanged");
}

#[test]
fn user_cancel_with_nonempty_input_does_not_restore() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    state.ui.input.textarea.set_text("user typed during cancel");
    let before_len = state.session.messages.len();

    on_turn_interrupted(&mut state, user_cancel());

    // No restore: nonempty input gates it off. Marker still appended.
    assert!(restored(before_len, &state));
    assert!(ends_with_interrupt_marker(&state));
    assert_eq!(
        state.ui.input.text(),
        "user typed during cancel",
        "user's in-flight text must NOT be clobbered",
    );
}

#[test]
fn user_cancel_with_active_surface_does_not_restore() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    state.ui.show_modal(ModalState::Help);
    let before_len = state.session.messages.len();

    on_turn_interrupted(&mut state, user_cancel());

    assert!(restored(before_len, &state));
    assert!(ends_with_interrupt_marker(&state));
    assert_eq!(state.ui.input.text(), "");
}

#[test]
fn user_cancel_with_queued_command_does_not_restore() {
    use crate::state::QueuedCommandDisplay;
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    state
        .session
        .queued_commands
        .push_back(QueuedCommandDisplay {
            id: "q1".into(),
            preview: "next".into(),
        });
    let before_len = state.session.messages.len();

    on_turn_interrupted(&mut state, user_cancel());

    assert!(restored(before_len, &state));
    assert!(ends_with_interrupt_marker(&state));
    assert_eq!(state.ui.input.text(), "");
}

#[test]
fn system_preempt_never_restores() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    let before_len = state.session.messages.len();

    on_turn_interrupted(&mut state, system_preempt());

    assert!(
        !restored(before_len, &state),
        "Clear/Compact/Rewind/Shutdown drains must not auto-restore",
    );
    // SystemPreempt does NOT append the marker either — the
    // preempting op (Clear/Compact/Rewind/Shutdown) owns whatever
    // gets written next.
    assert!(!ends_with_interrupt_marker(&state));
    assert_eq!(state.ui.input.text(), "");
}

#[test]
fn legacy_no_reason_is_treated_as_non_user_cancel() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    let before_len = state.session.messages.len();

    on_turn_interrupted(&mut state, legacy_no_reason());

    assert!(!restored(before_len, &state));
    assert_eq!(state.ui.input.text(), "");
}

#[test]
fn on_turn_interrupted_clears_streaming_and_busy() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    state.ui.streaming = Some(crate::state::StreamingState::default());
    state.session.set_busy(true);

    on_turn_interrupted(&mut state, user_cancel());

    assert!(state.ui.streaming.is_none());
    assert!(!state.session.is_busy());
}
