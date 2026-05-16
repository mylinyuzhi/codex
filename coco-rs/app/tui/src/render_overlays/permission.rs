//! Permission overlay content renderer.

use ratatui::prelude::Color;

use crate::presentation::request;
use crate::presentation::styles::UiStyles;
use crate::state::PermissionOverlay;

pub(super) fn permission_content(
    p: &PermissionOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    request::permission_content(p, styles)
}
