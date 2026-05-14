//! Context window visualization overlay renderer.

use ratatui::prelude::Color;

use crate::presentation::information;
use crate::state::AppState;
use crate::theme::Theme;

pub(super) fn context_viz_content(state: &AppState, theme: &Theme) -> (String, String, Color) {
    information::context_viz_content(state, theme)
}
