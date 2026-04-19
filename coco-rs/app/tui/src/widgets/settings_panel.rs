//! Settings panel widget — in-TUI configuration.
//!
//! TS: src/components/Settings/ (4 files, 2.5K LOC)
//! Tabs: Theme, Output Style, Permission Rules, Model.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

use crate::i18n::t;
use crate::theme::Theme;
use crate::theme::ThemeName;

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
    pub themes: Vec<ThemeName>,
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
    pub fn new() -> Self {
        Self {
            active_tab: SettingsTab::Theme,
            selected: 0,
            themes: vec![
                ThemeName::Default,
                ThemeName::Dark,
                ThemeName::Light,
                ThemeName::Dracula,
                ThemeName::Nord,
            ],
            output_styles: Vec::new(),
            permission_rules: Vec::new(),
        }
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
        Self::new()
    }
}

/// Settings panel widget.
pub struct SettingsPanelWidget<'a> {
    state: &'a SettingsPanelState,
    theme: &'a Theme,
}

impl<'a> SettingsPanelWidget<'a> {
    pub fn new(state: &'a SettingsPanelState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }
}

impl Widget for SettingsPanelWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        // Tab bar
        let tabs: Vec<(String, bool)> = vec![
            (
                t!("settings.tab_theme").to_string(),
                self.state.active_tab == SettingsTab::Theme,
            ),
            (
                t!("settings.tab_output").to_string(),
                self.state.active_tab == SettingsTab::OutputStyle,
            ),
            (
                t!("settings.tab_permissions").to_string(),
                self.state.active_tab == SettingsTab::Permissions,
            ),
            (
                t!("settings.tab_about").to_string(),
                self.state.active_tab == SettingsTab::About,
            ),
        ];

        let tab_spans: Vec<Span> = tabs
            .iter()
            .flat_map(|(name, active)| {
                let style = if *active {
                    ratatui::style::Style::default()
                        .fg(self.theme.primary)
                        .bold()
                        .underlined()
                } else {
                    ratatui::style::Style::default().fg(self.theme.text_dim)
                };
                vec![
                    Span::styled(format!(" {name} "), style),
                    Span::raw("│").fg(self.theme.border),
                ]
            })
            .collect();
        lines.push(Line::from(tab_spans));
        lines.push(Line::default());

        // Tab content
        match self.state.active_tab {
            SettingsTab::Theme => {
                for (i, theme_name) in self.state.themes.iter().enumerate() {
                    let marker = if i as i32 == self.state.selected {
                        "▸ "
                    } else {
                        "  "
                    };
                    let name = format!("{theme_name:?}");
                    lines.push(Line::from(vec![
                        Span::raw(marker),
                        Span::raw(name).fg(self.theme.text),
                    ]));
                }
            }
            SettingsTab::OutputStyle => {
                if self.state.output_styles.is_empty() {
                    lines.push(Line::from(
                        Span::raw(format!("  {}", t!("settings.no_output_styles")))
                            .fg(self.theme.text_dim),
                    ));
                    lines.push(Line::from(
                        Span::raw(format!("  {}", t!("settings.add_output_styles_hint")))
                            .fg(self.theme.text_dim),
                    ));
                } else {
                    for (i, style) in self.state.output_styles.iter().enumerate() {
                        let marker = if i as i32 == self.state.selected {
                            "▸ "
                        } else {
                            "  "
                        };
                        lines.push(Line::from(vec![
                            Span::raw(marker),
                            Span::raw(style.as_str()).fg(self.theme.text),
                        ]));
                    }
                }
            }
            SettingsTab::Permissions => {
                if self.state.permission_rules.is_empty() {
                    lines.push(Line::from(
                        Span::raw(format!("  {}", t!("settings.no_permission_rules")))
                            .fg(self.theme.text_dim),
                    ));
                } else {
                    for rule in &self.state.permission_rules {
                        lines.push(Line::from(vec![
                            Span::raw(format!("  {} ", rule.tool)).fg(self.theme.text),
                            Span::raw(format!("→ {} ", rule.behavior)).fg(self.theme.accent),
                            Span::raw(format!("({})", rule.source)).fg(self.theme.text_dim),
                        ]));
                    }
                }
            }
            SettingsTab::About => {
                lines.push(Line::from(
                    Span::raw(t!("dialog.settings_about_title").to_string())
                        .fg(self.theme.primary)
                        .bold(),
                ));
                lines.push(Line::from(
                    Span::raw(t!("dialog.settings_about_built").to_string())
                        .fg(self.theme.text_dim),
                ));
                lines.push(Line::from(
                    Span::raw(t!("dialog.settings_about_arch").to_string()).fg(self.theme.text_dim),
                ));
            }
        }

        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::raw(format!("  {}", t!("settings.hint_switch_tab"))).fg(self.theme.text_dim),
            Span::raw(t!("settings.hint_navigate").to_string()).fg(self.theme.text_dim),
            Span::raw(t!("settings.hint_select").to_string()).fg(self.theme.text_dim),
            Span::raw(t!("settings.hint_close").to_string()).fg(self.theme.text_dim),
        ]));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(t!("dialog.title_settings").to_string())
            .border_style(ratatui::style::Style::default().fg(self.theme.border_focused));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
