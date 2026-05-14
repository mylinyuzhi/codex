//! Tabbed settings panel overlay renderer.

use ratatui::prelude::Color;

use crate::presentation::settings;
use crate::theme::Theme;
use crate::widgets::settings_panel::SettingsPanelState;

pub(super) fn settings_overlay_content(
    s: &SettingsPanelState,
    theme: &Theme,
) -> (String, String, Color) {
    settings::settings_overlay_content(s, theme)
}
