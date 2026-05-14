//! Global search overlay (ripgrep streaming) renderer.

use ratatui::prelude::Color;

use crate::presentation::picker;
use crate::state::GlobalSearchOverlay;
use crate::theme::Theme;

pub(super) fn global_search_content(
    g: &GlobalSearchOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    picker::global_search_content(g, theme)
}
