//! Context window visualization state content builder.

use ratatui::prelude::Color;

use crate::presentation::information;
use crate::presentation::styles::UiStyles;
use crate::state::AppState;

pub(super) fn context_viz_content(
    state: &AppState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    information::context_viz_content(state, styles)
}
