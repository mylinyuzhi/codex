//! Rewind overlay renderer.

use ratatui::prelude::Color;

use crate::presentation::rewind;
use crate::presentation::styles::UiStyles;
use crate::state::rewind::RewindOverlay;

pub(super) fn rewind_overlay_content(
    overlay: &RewindOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    rewind::rewind_overlay_content(overlay, styles)
}
