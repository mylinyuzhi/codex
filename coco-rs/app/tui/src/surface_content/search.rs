//! Global search state content builder.

use ratatui::prelude::Color;

use crate::presentation::picker;
use crate::presentation::styles::UiStyles;
use crate::state::GlobalSearchState;

pub(super) fn global_search_content(
    g: &GlobalSearchState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::global_search_content(g, styles)
}
