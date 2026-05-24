//! Rewind state content builder.

use ratatui::prelude::Color;

use crate::presentation::rewind;
use crate::presentation::styles::UiStyles;
use crate::state::rewind::RewindState;

pub(super) fn rewind_surface_content(
    state: &RewindState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    rewind::rewind_surface_content(state, styles)
}
