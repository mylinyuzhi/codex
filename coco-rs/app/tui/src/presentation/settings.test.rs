use super::*;
use pretty_assertions::assert_eq;

use crate::i18n::locale_test_guard;
use crate::presentation::styles::UiStyles;
use crate::theme::Theme;
use crate::widgets::settings_panel::PermissionRuleDisplay;
use crate::widgets::settings_panel::SettingsPanelState;
use crate::widgets::settings_panel::SettingsTab;

#[test]
fn settings_overlay_content_renders_theme_tab_and_syntax_row() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = SettingsPanelState::default();

    let (title, body, border) = settings_overlay_content(&state, UiStyles::new(&theme));

    assert_eq!(title, " Settings ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("[Theme]   Output    Permissions    About "));
    assert!(body.contains("Syntax highlighting: Enabled"));
    assert!(body.contains("Tab Switch tab"));
}

#[test]
fn settings_overlay_content_marks_output_style_selection() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut state = SettingsPanelState {
        active_tab: SettingsTab::OutputStyle,
        selected: 1,
        output_styles: vec!["Brief".to_string(), "Detailed".to_string()],
        ..SettingsPanelState::default()
    };

    let (_, body, _) = settings_overlay_content(&state, UiStyles::new(&theme));
    assert!(body.contains("  Brief"));
    assert!(body.contains("▸ Detailed"));

    state.output_styles.clear();
    let (_, empty_body, _) = settings_overlay_content(&state, UiStyles::new(&theme));
    assert!(empty_body.contains("(no custom output styles)"));
}

#[test]
fn settings_overlay_content_lists_permission_rules() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let state = SettingsPanelState {
        active_tab: SettingsTab::Permissions,
        permission_rules: vec![PermissionRuleDisplay {
            tool: "Bash".to_string(),
            behavior: "allow".to_string(),
            source: "project".to_string(),
        }],
        ..SettingsPanelState::default()
    };

    let (_, body, _) = settings_overlay_content(&state, UiStyles::new(&theme));

    assert!(body.contains("[Permissions]"));
    assert!(body.contains("  Bash → allow (project)"));
}
