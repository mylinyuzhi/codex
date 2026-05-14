//! Permission overlay content renderer.

use ratatui::prelude::Color;

use crate::presentation::request;
use crate::state::PermissionOverlay;
use crate::theme::Theme;

pub(super) fn permission_content(p: &PermissionOverlay, theme: &Theme) -> (String, String, Color) {
    request::permission_content(p, theme)
}
