//! Transcript overlay renderer.

use ratatui::prelude::Color;

use crate::presentation::transcript;
use crate::state::AppState;
use crate::state::overlay::TranscriptOverlay;
use crate::theme::Theme;

pub(super) fn transcript_overlay_content(
    state: &AppState,
    overlay: &TranscriptOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    transcript::transcript_overlay_content(state, overlay, theme)
}
