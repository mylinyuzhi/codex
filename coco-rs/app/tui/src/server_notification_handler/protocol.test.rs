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
/// expected dispatched `Rewind { mode: AutoRestore }` carries the
/// same derivation, not the raw fixture id.
fn test_id(s: &str) -> String {
    crate::state::derive::id_to_uuid(s).to_string()
}

/// Channel pair scoped to one test. Caller drives `on_turn_interrupted`
/// with `&tx` and observes `rx.try_recv()` for the dispatched
/// `UserCommand::Rewind { mode: AutoRestore }`.
fn channel() -> (
    tokio::sync::mpsc::Sender<crate::command::UserCommand>,
    tokio::sync::mpsc::Receiver<crate::command::UserCommand>,
) {
    tokio::sync::mpsc::channel(16)
}

/// True if the receiver got a `Rewind { mode: AutoRestore }`. Drains
/// the channel; tests that need to inspect the message id should call
/// `rx.try_recv()` directly.
fn drained_auto_restore(
    rx: &mut tokio::sync::mpsc::Receiver<crate::command::UserCommand>,
) -> Option<String> {
    while let Ok(cmd) = rx.try_recv() {
        if let crate::command::UserCommand::Rewind {
            message_id,
            mode: crate::command::RewindMode::AutoRestore,
        } = cmd
        {
            return Some(message_id);
        }
    }
    None
}

#[test]
fn user_cancel_with_lossless_tail_restores() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    let (tx, mut rx) = channel();
    on_turn_interrupted(&mut state, user_cancel(), &tx);

    // Auto-restore lives entirely on the engine round-trip — the TUI
    // dispatches `UserCommand::Rewind { mode: AutoRestore }` directly
    // and pulls the prompt back into the input; the actual transcript
    // truncation happens when `MessageTruncated` arrives from the
    // engine.
    assert_eq!(state.ui.input.text(), "original prompt");
    assert!(state.session.conversation_id.is_some());
    assert_eq!(
        drained_auto_restore(&mut rx).as_deref(),
        Some(test_id("u1").as_str()),
    );
}

#[test]
fn user_cancel_without_auto_restore_leaves_no_dispatch() {
    // Meaningful tail → no auto-restore → no Rewind dispatch.
    let mut state = idle_with_meaningful_tail();
    let (tx, mut rx) = channel();
    on_turn_interrupted(&mut state, user_cancel(), &tx);
    assert!(drained_auto_restore(&mut rx).is_none());
}

/// True when an auto-restore Rewind landed on the channel.
fn restored(rx: &mut tokio::sync::mpsc::Receiver<crate::command::UserCommand>) -> bool {
    drained_auto_restore(rx).is_some()
}

#[test]
fn user_cancel_with_meaningful_tail_does_not_restore() {
    let mut state = idle_with_meaningful_tail();
    let (tx, mut rx) = channel();
    on_turn_interrupted(&mut state, user_cancel(), &tx);

    // Auto-restore suppressed (meaningful tail). Engine pushes its
    // own `SystemMessage::UserInterruption` marker through
    // `MessageAppended` — tested at the renderer layer, not here.
    assert!(!restored(&mut rx));
    assert_eq!(state.ui.input.text(), "", "input unchanged");
}

#[test]
fn user_cancel_with_nonempty_input_does_not_restore() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    state.ui.input.textarea.set_text("user typed during cancel");
    let (tx, mut rx) = channel();

    on_turn_interrupted(&mut state, user_cancel(), &tx);

    // No restore: nonempty input gates it off.
    assert!(!restored(&mut rx));
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
    let (tx, mut rx) = channel();

    on_turn_interrupted(&mut state, user_cancel(), &tx);

    assert!(!restored(&mut rx));
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
    let (tx, mut rx) = channel();

    on_turn_interrupted(&mut state, user_cancel(), &tx);

    assert!(!restored(&mut rx));
    assert_eq!(state.ui.input.text(), "");
}

#[test]
fn system_preempt_never_restores() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    let (tx, mut rx) = channel();

    on_turn_interrupted(&mut state, system_preempt(), &tx);

    assert!(
        !restored(&mut rx),
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
    let (tx, mut rx) = channel();

    on_turn_interrupted(&mut state, legacy_no_reason(), &tx);

    assert!(!restored(&mut rx));
    assert_eq!(state.ui.input.text(), "");
}

#[test]
fn on_turn_interrupted_clears_streaming_and_busy() {
    let mut state = idle_with_lossless_tail("u1", "original prompt");
    state.ui.streaming = Some(crate::state::StreamingState::default());
    state.session.set_busy(true);
    let (tx, _rx) = channel();

    on_turn_interrupted(&mut state, user_cancel(), &tx);

    assert!(state.ui.streaming.is_none());
    assert!(!state.session.is_busy());
}
