//! Full-screen diff view overlay content builder.

use ratatui::prelude::Color;

use crate::presentation::information;
use crate::presentation::styles::UiStyles;
use crate::state::DiffViewOverlay;

pub(super) fn diff_view_content(
    d: &DiffViewOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    information::diff_view_content(d, styles)
}
