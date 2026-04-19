//! Tabbed settings panel overlay renderer.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::theme::Theme;
use crate::widgets::settings_panel::SettingsPanelState;
use crate::widgets::settings_panel::SettingsTab;

pub(super) fn settings_overlay_content(
    s: &SettingsPanelState,
    theme: &Theme,
) -> (String, String, Color) {
    let tab_bar = [
        (SettingsTab::Theme, t!("dialog.settings_tab_theme")),
        (SettingsTab::OutputStyle, t!("dialog.settings_tab_output")),
        (
            SettingsTab::Permissions,
            t!("dialog.settings_tab_permissions"),
        ),
        (SettingsTab::About, t!("dialog.settings_tab_about")),
    ]
    .iter()
    .map(|(tab, label)| {
        if *tab == s.active_tab {
            format!("[{label}]")
        } else {
            format!(" {label} ")
        }
    })
    .collect::<Vec<_>>()
    .join("  ");

    let items: Vec<String> = match s.active_tab {
        SettingsTab::Theme => s
            .themes
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let marker = if i as i32 == s.selected { "▸ " } else { "  " };
                format!("{marker}{t:?}")
            })
            .collect(),
        SettingsTab::OutputStyle => {
            if s.output_styles.is_empty() {
                vec![t!("dialog.settings_no_custom_styles").to_string()]
            } else {
                s.output_styles
                    .iter()
                    .enumerate()
                    .map(|(i, style)| {
                        let marker = if i as i32 == s.selected { "▸ " } else { "  " };
                        format!("{marker}{style}")
                    })
                    .collect()
            }
        }
        SettingsTab::Permissions => {
            if s.permission_rules.is_empty() {
                vec![t!("dialog.settings_no_rules").to_string()]
            } else {
                s.permission_rules
                    .iter()
                    .map(|r| format!("  {} → {} ({})", r.tool, r.behavior, r.source))
                    .collect()
            }
        }
        SettingsTab::About => vec![
            t!("dialog.settings_about_title").to_string(),
            t!("dialog.settings_about_arch").to_string(),
            t!("dialog.settings_about_built").to_string(),
        ],
    };

    let body = format!(
        "{tab_bar}\n\n{}\n\n{}",
        items.join("\n"),
        t!("dialog.hints_settings")
    );
    (t!("dialog.title_settings").to_string(), body, theme.primary)
}
