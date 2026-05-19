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
use crate::state::ModalState;
use crate::state::derive::test_helpers;

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
    test_helpers::push_user_text(&mut s.session, user_id, user_text);
    test_helpers::push_assistant_text(&mut s.session, "");
    s
}

/// Idle session with a user message followed by a real assistant
/// response — auto-restore must be suppressed.
fn idle_with_meaningful_tail() -> AppState {
    let mut s = AppState::new();
    test_helpers::push_user_text(&mut s.session, "u1", "ask");
    test_helpers::push_assistant_text(&mut s.session, "actual reply text");
    s
}

// ── Auto-restore matrix ─────────────────────────────────────────

/// Map a legacy test id ("u1") to the v5 UUID string the cell mirror
/// produces. `apply_auto_restore` reads message ids from
/// `transcript.cells()` (= `cell.message_uuid.to_string()`), so the
/// expected `pending_auto_restore_truncate` value is the same
/// derivation, not the raw fixture id.
fn test_id(s: &str) -> String {
    crate::state::derive::id_to_uuid(s).to_string()
}

#[test]
fn user_cancel_with_lossless_tail_restores() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    on_turn_interrupted(&mut state, user_cancel());

    // Auto-restore now lives entirely on the engine round-trip — the
    // TUI sets `pending_auto_restore_truncate` and pulls the prompt
    // back into the input; the actual transcript truncation happens
    // when `MessageTruncated` arrives from the engine. We just verify
    // the TUI-side outcome here.
    assert_eq!(state.ui.input.text(), "original prompt");
    assert!(state.session.conversation_id.is_some());
    assert_eq!(
        state.session.pending_auto_restore_truncate.as_deref(),
        Some(test_id("u1").as_str()),
    );
}

#[test]
fn user_cancel_without_auto_restore_leaves_pending_none() {
    // Meaningful tail → no auto-restore → no engine truncation
    // expected → pending field stays None.
    let mut state = idle_with_meaningful_tail();
    on_turn_interrupted(&mut state, user_cancel());
    assert!(state.session.pending_auto_restore_truncate.is_none());
}

/// Returns true if auto-restore fired — TUI-side signal is
/// `pending_auto_restore_truncate` being set (the engine round-trip
/// completes the truncation later).
fn restored(state: &AppState) -> bool {
    state.session.pending_auto_restore_truncate.is_some()
}

#[test]
fn user_cancel_with_meaningful_tail_does_not_restore() {
    let mut state = idle_with_meaningful_tail();
    on_turn_interrupted(&mut state, user_cancel());

    // Auto-restore suppressed (meaningful tail). Engine pushes its
    // own `SystemMessage::UserInterruption` marker through
    // `MessageAppended` — tested at the renderer layer, not here.
    assert!(!restored(&state));
    assert_eq!(state.ui.input.text(), "", "input unchanged");
}

#[test]
fn user_cancel_with_nonempty_input_does_not_restore() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    state.ui.input.textarea.set_text("user typed during cancel");

    on_turn_interrupted(&mut state, user_cancel());

    // No restore: nonempty input gates it off.
    assert!(!restored(&state));
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

    on_turn_interrupted(&mut state, user_cancel());

    assert!(!restored(&state));
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

    on_turn_interrupted(&mut state, user_cancel());

    assert!(!restored(&state));
    assert_eq!(state.ui.input.text(), "");
}

#[test]
fn system_preempt_never_restores() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");

    on_turn_interrupted(&mut state, system_preempt());

    assert!(
        !restored(&state),
        "Clear/Compact/Rewind/Shutdown drains must not auto-restore",
    );
    // SystemPreempt does NOT append the marker either — the
    // preempting op (Clear/Compact/Rewind/Shutdown) owns whatever
    // gets written next.
    assert_eq!(state.ui.input.text(), "");
}

#[test]
fn legacy_no_reason_is_treated_as_non_user_cancel() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");

    on_turn_interrupted(&mut state, legacy_no_reason());

    assert!(!restored(&state));
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
