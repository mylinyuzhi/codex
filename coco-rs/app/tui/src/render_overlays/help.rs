//! Help overlay renderer.

use ratatui::prelude::Color;

use crate::presentation::help;
use crate::state::AppState;
use crate::theme::Theme;

pub(super) fn help_content(state: &AppState, theme: &Theme) -> (String, String, Color) {
    help::help_content(state, theme)
}
