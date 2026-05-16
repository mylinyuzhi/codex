//! Tabbed settings panel overlay renderer.

use ratatui::prelude::Color;

use crate::presentation::settings;
use crate::presentation::styles::UiStyles;
use crate::widgets::settings_panel::SettingsPanelState;

pub(super) fn settings_overlay_content(
    s: &SettingsPanelState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    settings::settings_overlay_content(s, styles)
}
