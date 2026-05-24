//! Tabbed settings panel state content builder.

use ratatui::prelude::Color;

use crate::presentation::settings;
use crate::presentation::styles::UiStyles;
use crate::widgets::settings_panel::SettingsPanelState;

pub(super) fn settings_surface_content(
    s: &SettingsPanelState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    settings::settings_surface_content(s, styles)
}
