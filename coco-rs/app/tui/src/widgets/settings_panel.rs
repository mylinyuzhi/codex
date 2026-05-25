//! Settings panel state — backing struct for the in-TUI settings
//! modal. Rendering lives in [`crate::presentation::settings`]; this
//! module owns only the typed state + helper methods.
//!
//! TS: src/components/Settings/ (4 files, 2.5K LOC). The Rust port
//! splits state/render across crate modules so the same state can be
//! rendered into both the modal surface and the embedded surface
//! content path without duplicating the widget.

use crate::display_settings::DisplaySettings;
use crate::display_settings::SyntaxHighlighting;
use crate::i18n::t;
use crate::theme::ThemeChoice;
use crate::theme::ThemeRuntimeState;
use crate::theme::ThemeSetting;

/// Settings panel tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Theme,
    OutputStyle,
    Permissions,
    About,
}

/// Settings panel state.
#[derive(Debug, Clone)]
pub struct SettingsPanelState {
    pub active_tab: SettingsTab,
    pub selected: i32,
    pub themes: Vec<ThemeChoice>,
    pub active_theme: ThemeSetting,
    pub display_settings: DisplaySettings,
    pub output_styles: Vec<String>,
    pub permission_rules: Vec<PermissionRuleDisplay>,
}

/// A permission rule for display.
#[derive(Debug, Clone)]
pub struct PermissionRuleDisplay {
    pub tool: String,
    pub behavior: String,
    pub source: String,
}

impl SettingsPanelState {
    pub fn new(theme_state: &ThemeRuntimeState, display_settings: DisplaySettings) -> Self {
        Self {
            active_tab: SettingsTab::Theme,
            selected: selected_theme_index(&theme_state.choices, &theme_state.setting),
            themes: theme_state.choices.clone(),
            active_theme: theme_state.setting.clone(),
            display_settings,
            output_styles: Vec::new(),
            permission_rules: Vec::new(),
        }
    }

    pub fn set_themes(&mut self, themes: Vec<ThemeChoice>, active_theme: ThemeSetting) {
        self.selected = selected_theme_index(&themes, &active_theme);
        self.themes = themes;
        self.active_theme = active_theme;
    }

    pub fn set_display_settings(&mut self, display_settings: DisplaySettings) {
        self.display_settings = display_settings;
    }

    pub fn selected_theme_choice(&self) -> Option<&ThemeChoice> {
        usize::try_from(self.selected)
            .ok()
            .and_then(|selected| self.themes.get(selected))
    }

    pub fn is_syntax_highlighting_selected(&self) -> bool {
        self.selected == self.syntax_highlighting_index()
    }

    pub fn is_copy_full_response_selected(&self) -> bool {
        self.selected == self.copy_full_response_index()
    }

    pub fn theme_item_count(&self) -> usize {
        self.themes.len() + 2
    }

    fn syntax_highlighting_index(&self) -> i32 {
        self.themes.len() as i32
    }

    fn copy_full_response_index(&self) -> i32 {
        self.themes.len() as i32 + 1
    }

    pub fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            SettingsTab::Theme => SettingsTab::OutputStyle,
            SettingsTab::OutputStyle => SettingsTab::Permissions,
            SettingsTab::Permissions => SettingsTab::About,
            SettingsTab::About => SettingsTab::Theme,
        };
        self.selected = 0;
    }

    pub fn prev_tab(&mut self) {
        self.active_tab = match self.active_tab {
            SettingsTab::Theme => SettingsTab::About,
            SettingsTab::OutputStyle => SettingsTab::Theme,
            SettingsTab::Permissions => SettingsTab::OutputStyle,
            SettingsTab::About => SettingsTab::Permissions,
        };
        self.selected = 0;
    }
}

impl Default for SettingsPanelState {
    fn default() -> Self {
        Self::new(&ThemeRuntimeState::default(), DisplaySettings::default())
    }
}

fn selected_theme_index(themes: &[ThemeChoice], active_theme: &ThemeSetting) -> i32 {
    themes
        .iter()
        .position(|choice| &choice.setting == active_theme)
        .unwrap_or(0) as i32
}

pub(crate) fn syntax_highlighting_status(syntax_highlighting: SyntaxHighlighting) -> String {
    match syntax_highlighting {
        SyntaxHighlighting::Enabled => t!("settings.enabled").to_string(),
        SyntaxHighlighting::Disabled => t!("settings.disabled").to_string(),
    }
}

pub(crate) fn syntax_highlighting_status_for_display(settings: DisplaySettings) -> String {
    let status = syntax_highlighting_status(settings.syntax_highlighting);
    if let Some(source) = settings.syntax_highlighting_editability.overriding_source() {
        format!(
            "{status} ({})",
            t!("settings.overridden_by", source = source.as_str())
        )
    } else {
        status
    }
}
