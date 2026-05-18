//! Permission state content builder.

use ratatui::prelude::Color;

use crate::presentation::request;
use crate::presentation::styles::UiStyles;
use crate::state::PermissionPromptState;

pub(super) fn permission_content(
    p: &PermissionPromptState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    request::permission_content(p, styles)
}
