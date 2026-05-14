//! Rewind overlay renderer.

use ratatui::prelude::Color;

use crate::presentation::rewind;
use crate::state::rewind::RewindOverlay;
use crate::theme::Theme;

pub(super) fn rewind_overlay_content(
    overlay: &RewindOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    rewind::rewind_overlay_content(overlay, theme)
}
