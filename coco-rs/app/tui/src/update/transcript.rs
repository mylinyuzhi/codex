//! `app:toggleTranscript` handler — open / close the transcript overlay.
//!
//! Mirrors TS `useGlobalKeybindings.tsx::handleToggleTranscript`
//! (lines 90-150 minus the KAIROS escape hatch). Two-way toggle:
//!
//! * not in transcript → open `Overlay::Transcript` with default state
//! * already in transcript → dismiss the overlay (back to chat)
//!
//! coco-rs's transcript is a read-only overlay (no alt-screen / virtual
//! scroll / search-bar), unlike TS which uses a full screen takeover.
//! The shape is the same: verbose, all messages, scrollable.

use crate::state::AppState;
use crate::state::Overlay;
use crate::state::overlay::TranscriptOverlay;

/// Open the transcript overlay if it isn't open; close it if it is.
pub(super) fn toggle(state: &mut AppState) {
    if matches!(state.ui.active_overlay(), Some(Overlay::Transcript(_))) {
        state.ui.dismiss_overlay();
    } else {
        state
            .ui
            .set_overlay(Overlay::Transcript(TranscriptOverlay::new()));
    }
}

/// Flip `show_all` on the active transcript overlay. No-op when the
/// active overlay is not a transcript — protects against the chord
/// firing while the user is in some unrelated overlay.
pub(super) fn toggle_show_all(state: &mut AppState) -> bool {
    if let Some(Overlay::Transcript(t)) = state.ui.active_overlay_mut() {
        t.show_all = !t.show_all;
        true
    } else {
        false
    }
}

#[cfg(test)]
#[path = "transcript.test.rs"]
mod tests;
