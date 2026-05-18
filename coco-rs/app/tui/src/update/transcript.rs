//! `app:toggleTranscript` handler — open / close the transcript overlay.
//!
//! Mirrors TS `useGlobalKeybindings.tsx::handleToggleTranscript`
//! (lines 90-150 minus the KAIROS escape hatch). Two-way toggle:
//!
//! * not in transcript → open `Overlay::Transcript` with default state
//! * already in transcript → dismiss the overlay (back to chat)
//!
//! coco-rs's transcript is a cell-level reader. It keeps expansion state in
//! the overlay only and does not mirror TS's show-all path.

use crate::state::AppState;
use crate::state::Overlay;
use crate::state::transcript::TranscriptOverlay;
use crate::state::transcript::TranscriptScrollPosition;

/// Open the transcript overlay if it isn't open; close it if it is.
pub(super) fn toggle(state: &mut AppState) {
    if matches!(state.ui.active_overlay(), Some(Overlay::Transcript(_))) {
        state.ui.dismiss_overlay();
    } else {
        let anchor = crate::presentation::transcript::latest_expandable_cell_id(state);
        state
            .ui
            .set_overlay(Overlay::Transcript(TranscriptOverlay::new_with_anchor(
                anchor,
            )));
    }
}

pub(super) fn scroll_lines(state: &mut AppState, delta: i32) -> bool {
    let Some(Overlay::Transcript(overlay)) = state.ui.active_overlay_mut() else {
        return false;
    };
    overlay.scroll.scroll_lines(delta);
    true
}

pub(super) fn page(state: &mut AppState, delta: i32) -> bool {
    let amount = transcript_page_rows(state);
    let Some(Overlay::Transcript(overlay)) = state.ui.active_overlay_mut() else {
        return false;
    };
    let signed = amount.min(i32::MAX as usize) as i32;
    overlay
        .scroll
        .scroll_lines(if delta < 0 { -signed } else { signed });
    true
}

pub(super) fn jump_start(state: &mut AppState) -> bool {
    let Some(Overlay::Transcript(overlay)) = state.ui.active_overlay_mut() else {
        return false;
    };
    overlay.scroll.jump_start();
    true
}

pub(super) fn jump_end(state: &mut AppState) -> bool {
    let Some(Overlay::Transcript(overlay)) = state.ui.active_overlay_mut() else {
        return false;
    };
    overlay.scroll.jump_end();
    true
}

pub(super) fn select_expandable(state: &mut AppState, delta: i32) -> bool {
    let ids = crate::presentation::transcript::transcript_expandable_cell_ids(state);
    let Some(Overlay::Transcript(overlay)) = state.ui.active_overlay_mut() else {
        return false;
    };
    if ids.is_empty() {
        overlay.selected_cell_id = None;
        return true;
    }

    let current = overlay
        .selected_cell_id
        .as_ref()
        .and_then(|id| ids.iter().position(|candidate| candidate == id));
    let next = match (current, delta.cmp(&0)) {
        (Some(index), std::cmp::Ordering::Less | std::cmp::Ordering::Greater) => {
            (index as i32 + delta).rem_euclid(ids.len() as i32) as usize
        }
        (Some(index), std::cmp::Ordering::Equal) => index,
        (None, std::cmp::Ordering::Less) => ids.len() - 1,
        (None, _) => 0,
    };
    overlay.selected_cell_id = Some(ids[next].clone());
    overlay.scroll = TranscriptScrollPosition::anchor(ids[next].clone());
    true
}

pub(super) fn toggle_selected_cell(state: &mut AppState) -> bool {
    let expandable = crate::presentation::transcript::transcript_expandable_cell_ids(state);
    let Some(Overlay::Transcript(overlay)) = state.ui.active_overlay_mut() else {
        return false;
    };
    let Some(id) = overlay.selected_cell_id.clone() else {
        return true;
    };
    if !expandable.iter().any(|candidate| candidate == &id) {
        return true;
    }
    if !overlay.collapsed_cell_ids.insert(id.clone()) {
        overlay.collapsed_cell_ids.remove(&id);
    }
    overlay.scroll = TranscriptScrollPosition::anchor(id);
    true
}

fn transcript_page_rows(state: &AppState) -> usize {
    let size = state.ui.terminal_size;
    // Transcript uses the alt-screen with one border row on each side and a
    // two-line footer when space allows.
    usize::from(size.height.saturating_sub(4)).max(1)
}

#[cfg(test)]
#[path = "transcript.test.rs"]
mod tests;
