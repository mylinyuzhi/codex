//! Global search overlay content builder.

use ratatui::prelude::Color;

use crate::presentation::picker;
use crate::presentation::styles::UiStyles;
use crate::state::GlobalSearchOverlay;

pub(super) fn global_search_content(
    g: &GlobalSearchOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::global_search_content(g, styles)
}
