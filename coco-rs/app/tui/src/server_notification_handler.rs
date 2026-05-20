//! Server notification handler — processes protocol events from the agent loop.
//!
//! Architecture (post WS-2 refactor): this module receives `CoreEvent` from
//! the agent loop and dispatches directly to three exhaustive handlers:
//!
//! - [`protocol::handle`] — all `ServerNotification` variants (exhaustive)
//! - [`stream::handle`] — all `AgentStreamEvent` variants (exhaustive)
//! - [`tui_only::handle`] — TUI-exclusive `TuiOnlyEvent` variants
//!
//! The old `TuiNotification` bridge type has been deleted. See
//! `event-system-design.md` §1.7-1.8 and plan file WS-2 for rationale:
//! - 75% of variants were trivial pass-throughs with no real adaptation
//! - Scaling to 57 variants would create a 1:1 copy, tripling maintenance
//! - The TUI is not classical TEA; `TuiNotification` was a private
//!   intermediate for one of two orthogonal dispatch paths
//! - TS has no equivalent (direct dispatch via handleMessageFromStream)
//!
//! # Per-layer submodules
//!
//! Each `CoreEvent` layer owns its own file under
//! `server_notification_handler/`. This split was driven by the root file
//! exceeding 990 LoC — past CLAUDE.md's 800-line threshold. Per-layer files
//! keep each handler under 500 lines and make the exhaustive-match arms
//! scannable.
//!
//! - `protocol.rs` — 65 `ServerNotification` arms + `on_turn_completed`
//! - `stream.rs` — 7 `AgentStreamEvent` arms
//! - `tui_only.rs` — 21 `TuiOnlyEvent` arms + diff-stats, rewind, elicitation helpers
//!
//! Complex per-variant logic is extracted into named private functions
//! within each layer's file (e.g. `on_turn_completed`, `on_rewind_completed`).

use coco_types::CoreEvent;
use tokio::sync::mpsc::Sender;

use crate::command::UserCommand;
use crate::state::AppState;

mod projection;
mod protocol;
mod stream;
mod tui_only;

/// Handle a `CoreEvent` from the agent loop.
///
/// Dispatches to the per-layer handler. Each layer matches exhaustively —
/// adding a new variant in `coco-types` fails compilation in the matching
/// submodule until a TUI behavior is chosen.
///
/// `command_tx` lets handlers fire a follow-up [`UserCommand`] without
/// the older two-step `pending_*` field dance. Handlers call
/// `command_tx.try_send(...)` directly when an event must round-trip
/// back to the engine (e.g. auto-restore, TUI-originated system
/// messages). See `engine-tui-unified-transcript-plan.md` §6.4.
///
/// Returns `true` if any state changed and a redraw is needed.
pub fn handle_core_event(
    state: &mut AppState,
    event: CoreEvent,
    command_tx: &Sender<UserCommand>,
) -> bool {
    match event {
        CoreEvent::Protocol(notif) => protocol::handle(state, notif, command_tx),
        CoreEvent::Stream(stream_evt) => stream::handle(state, stream_evt),
        CoreEvent::Tui(tui_evt) => tui_only::handle(state, tui_evt, command_tx),
    }
}

/// Thin wrapper around [`handle_core_event`] that constructs a
/// dummy `Sender<UserCommand>` internally. Intended for tests that
/// don't need to assert on dispatched commands. Tests that DO need
/// to observe commands should call [`handle_core_event`] directly
/// with their own channel.
///
/// The receiver lives for the duration of the call, so any
/// `try_send` inside the handler succeeds; the message is then
/// dropped when the receiver goes out of scope — equivalent to
/// /dev/null for dispatched follow-up commands.
///
/// Marked `pub` (not `pub(crate)`) so integration tests under
/// `app/tui/tests/` can use it without spelling out the channel.
pub fn handle_event_for_test(state: &mut AppState, event: CoreEvent) -> bool {
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    handle_core_event(state, event, &tx)
}

#[cfg(test)]
#[path = "server_notification_handler.test.rs"]
mod tests;
