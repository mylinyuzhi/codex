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

use crate::state::AppState;

mod protocol;
mod stream;
mod tui_only;

/// Handle a `CoreEvent` from the agent loop.
///
/// Dispatches to the per-layer handler. Each layer matches exhaustively —
/// adding a new variant in `coco-types` fails compilation in the matching
/// submodule until a TUI behavior is chosen.
///
/// Returns `true` if any state changed and a redraw is needed.
pub fn handle_core_event(state: &mut AppState, event: CoreEvent) -> bool {
    match event {
        CoreEvent::Protocol(notif) => protocol::handle(state, notif),
        CoreEvent::Stream(stream_evt) => stream::handle(state, stream_evt),
        CoreEvent::Tui(tui_evt) => tui_only::handle(state, tui_evt),
    }
}

#[cfg(test)]
#[path = "server_notification_handler.test.rs"]
mod tests;
