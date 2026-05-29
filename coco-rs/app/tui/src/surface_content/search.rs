//! Global search state content builder.

use ratatui::prelude::Color;

use crate::presentation::picker;
use crate::state::GlobalSearchState;
use coco_tui_ui::style::UiStyles;

pub(super) fn global_search_content(
    g: &GlobalSearchState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::global_search_content(g, styles)
}
