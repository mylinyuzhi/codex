//! Full-screen diff view overlay renderer.

use ratatui::prelude::Color;

use crate::presentation::information;
use crate::state::DiffViewOverlay;
use crate::theme::Theme;

pub(super) fn diff_view_content(d: &DiffViewOverlay, theme: &Theme) -> (String, String, Color) {
    information::diff_view_content(d, theme)
}
