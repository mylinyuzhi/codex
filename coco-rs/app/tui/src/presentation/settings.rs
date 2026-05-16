//! Presentation for the settings overlay.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::presentation::styles::UiStyles;
use crate::widgets::settings_panel::SettingsPanelState;
use crate::widgets::settings_panel::SettingsTab;
use crate::widgets::settings_panel::syntax_highlighting_status_for_display;

pub(crate) fn settings_overlay_content(
    s: &SettingsPanelState,
    styles: UiStyles<'_>,
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
        SettingsTab::Theme => {
            let mut items: Vec<String> = s
                .themes
                .iter()
                .enumerate()
                .map(|(i, choice)| {
                    let marker = if i as i32 == s.selected { "▸ " } else { "  " };
                    let active = if choice.setting == s.active_theme {
                        "✓ "
                    } else {
                        "  "
                    };
                    format!("{marker}{active}{}", choice.label)
                })
                .collect();
            let marker = if s.is_syntax_highlighting_selected() {
                "▸ "
            } else {
                "  "
            };
            let active = if s.display_settings.syntax_highlighting.is_enabled() {
                "✓ "
            } else {
                "  "
            };
            let status = syntax_highlighting_status_for_display(s.display_settings);
            items.push(String::new());
            items.push(format!(
                "{marker}{active}{}: {status}",
                t!("settings.syntax_highlighting")
            ));
            items
        }
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
    (
        t!("dialog.title_settings").to_string(),
        body,
        styles.primary(),
    )
}

#[cfg(test)]
#[path = "settings.test.rs"]
mod tests;
