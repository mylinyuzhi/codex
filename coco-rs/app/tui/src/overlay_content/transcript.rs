//! Transcript overlay content builder.

use ratatui::prelude::Color;

use crate::presentation::styles::UiStyles;
use crate::presentation::transcript;
use crate::state::AppState;
use crate::state::overlay::TranscriptOverlay;

pub(super) fn transcript_overlay_content(
    state: &AppState,
    overlay: &TranscriptOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    transcript::transcript_overlay_content(state, overlay, styles)
}
