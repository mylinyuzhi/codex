//! Exit confirmation decision logic for Ctrl+C / Ctrl+D.
//!
//! Pure functions that compute what `update.rs` should do when the
//! user presses an exit key. The side-effect fanout (sending
//! `UserCommand`s, calling `state.quit()`) lives in `update.rs`. This
//! split keeps the decision tree fully unit-testable without spinning
//! up a tokio runtime or mocking the command channel.
//!
//! **Auto-restore is NOT decided here.** Busy Ctrl+C emits an
//! `Interrupt` command; idle Ctrl+C only arms the exit hint. The actual
//! rewind decision runs on the corresponding
//! `ServerNotification::TurnInterrupted` event in
//! `server_notification_handler::protocol::on_turn_interrupted`,
//! mirroring TS where `restoreMessageSync` fires inside the query's
//! `.finally` block (`REPL.tsx:3010-3022`), not on keypress.
//!
//! TS source: `src/hooks/useExitOnCtrlCD.ts` for the double-press
//! table; `src/hooks/useCancelRequest.ts:160-162` for the Ctrl+C
//! handler activation conditions.

use std::time::Instant;

use crate::double_press::Outcome;
use crate::state::AppState;

/// What `update::handle_command` should do after `on_interrupt` /
/// `on_request_exit` returns.
///
/// * `InterruptOnly` — busy session: cancel, no exit prompt.
/// * `ArmOnly`       — first idle Ctrl+C / Ctrl+D: no interrupt, only
///   arm "Press X again to exit" hint.
/// * `Quit`          — second press completed the double — shut down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitEffect {
    InterruptOnly,
    ArmOnly,
    Quit,
}

/// Decide how to handle Ctrl+C.
///
/// Mutates only the Ctrl+C double-press tracker. The caller turns the
/// returned [`ExitEffect`] into `UserCommand` sends + state changes.
pub fn on_interrupt(state: &mut AppState, now: Instant) -> ExitEffect {
    // Path A: a real task is in flight — Ctrl+C cancels it. Drop any
    // armed prompt so the user doesn't see "press again to exit" while
    // cancellation propagates. The backend's TurnInterrupted handler
    // owns any subsequent auto-restore.
    if state.has_interruptible_work() {
        state.ui.ctrl_c_tracker.reset();
        return ExitEffect::InterruptOnly;
    }
    // Path B: idle. Run the double-press machine.
    if state.ui.ctrl_c_tracker.poll((), now) == Outcome::Double {
        return ExitEffect::Quit;
    }
    // First press in idle only arms the hint. TS analogue:
    // `useExitOnCtrlCD.handleCtrlCDoublePress` setting
    // `exitState = { pending: true, keyName: 'Ctrl-C' }`.
    // The TS cancel handler is inactive when the main prompt is idle,
    // so there is no backend interrupt and no top "Interrupted" banner.
    ExitEffect::ArmOnly
}

/// Decide how to handle Ctrl+D.
///
/// Ctrl+D is exit-only (no cancel semantics) — first press arms, second
/// press quits. Mutates only the Ctrl+D double-press tracker.
pub fn on_request_exit(state: &mut AppState, now: Instant) -> ExitEffect {
    if state.ui.ctrl_d_tracker.poll((), now) == Outcome::Double {
        ExitEffect::Quit
    } else {
        ExitEffect::ArmOnly
    }
}

#[cfg(test)]
#[path = "exit.test.rs"]
mod tests;
