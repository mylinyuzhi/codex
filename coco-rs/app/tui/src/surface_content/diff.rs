//! Full-screen diff view state content builder.

use ratatui::prelude::Color;

use crate::presentation::information;
use crate::presentation::styles::UiStyles;
use crate::state::DiffViewState;

pub(super) fn diff_view_content(
    d: &DiffViewState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    information::diff_view_content(d, styles)
}
