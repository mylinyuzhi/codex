//! Plugin management widget — enable/disable/install plugins from TUI.
//!
//! TS: src/hooks/useManagePlugins.ts (11KB), src/components/PluginManager/

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

use crate::theme::Theme;

/// Plugin entry for the manager display.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub enabled: bool,
    pub source: PluginSource,
    pub tool_count: i32,
    pub error: Option<String>,
}

/// Where the plugin came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSource {
    Managed,
    User,
    Project,
    Builtin,
}

/// Plugin manager tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginTab {
    Installed,
    Available,
}

/// Plugin manager state.
#[derive(Debug, Clone)]
pub struct PluginManagerState {
    pub tab: PluginTab,
    pub plugins: Vec<PluginEntry>,
    pub selected: i32,
    pub filter: String,
}

impl PluginManagerState {
    pub fn new(plugins: Vec<PluginEntry>) -> Self {
        Self {
            tab: PluginTab::Installed,
            plugins,
            selected: 0,
            filter: String::new(),
        }
    }

    /// Get filtered plugins based on current tab and filter.
    pub fn filtered(&self) -> Vec<&PluginEntry> {
        let filter_lower = self.filter.to_lowercase();
        self.plugins
            .iter()
            .filter(|p| {
                if !filter_lower.is_empty() && !p.name.to_lowercase().contains(&filter_lower) {
                    return false;
                }
                true
            })
            .collect()
    }

    pub fn toggle_tab(&mut self) {
        self.tab = match self.tab {
            PluginTab::Installed => PluginTab::Available,
            PluginTab::Available => PluginTab::Installed,
        };
        self.selected = 0;
    }
}

impl Default for PluginManagerState {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// Plugin manager widget.
pub struct PluginManagerWidget<'a> {
    state: &'a PluginManagerState,
    theme: &'a Theme,
}

impl<'a> PluginManagerWidget<'a> {
    pub fn new(state: &'a PluginManagerState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }
}

impl Widget for PluginManagerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        // Tab bar
        let installed_style = if self.state.tab == PluginTab::Installed {
            ratatui::style::Style::default()
                .fg(self.theme.primary)
                .bold()
                .underlined()
        } else {
            ratatui::style::Style::default().fg(self.theme.text_dim)
        };
        let available_style = if self.state.tab == PluginTab::Available {
            ratatui::style::Style::default()
                .fg(self.theme.primary)
                .bold()
                .underlined()
        } else {
            ratatui::style::Style::default().fg(self.theme.text_dim)
        };

        lines.push(Line::from(vec![
            Span::styled(" Installed ", installed_style),
            Span::raw("│").fg(self.theme.border),
            Span::styled(" Available ", available_style),
        ]));

        // Filter
        if !self.state.filter.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  🔍 ").fg(self.theme.accent),
                Span::raw(&self.state.filter).fg(self.theme.text),
            ]));
        }
        lines.push(Line::default());

        // Plugin list
        let filtered = self.state.filtered();
        if filtered.is_empty() {
            lines.push(Line::from(
                Span::raw("  No plugins found").fg(self.theme.text_dim),
            ));
        }

        for (i, plugin) in filtered.iter().enumerate().take(15) {
            let is_selected = i as i32 == self.state.selected;
            let marker = if is_selected { "▸ " } else { "  " };

            let status_icon = if plugin.error.is_some() {
                ("✗", self.theme.error)
            } else if plugin.enabled {
                ("●", self.theme.success)
            } else {
                ("○", self.theme.text_dim)
            };

            let source_label = match plugin.source {
                PluginSource::Managed => "managed",
                PluginSource::User => "user",
                PluginSource::Project => "project",
                PluginSource::Builtin => "builtin",
            };

            let mut spans = vec![
                Span::raw(marker),
                Span::raw(format!("{} ", status_icon.0)).fg(status_icon.1),
                Span::raw(&plugin.name).fg(self.theme.text),
            ];

            if let Some(ref ver) = plugin.version {
                spans.push(Span::raw(format!(" v{ver}")).fg(self.theme.text_dim));
            }

            spans.push(Span::raw(format!(" [{source_label}]")).fg(self.theme.text_dim));

            if plugin.tool_count > 0 {
                spans.push(
                    Span::raw(format!(" ({} tools)", plugin.tool_count)).fg(self.theme.text_dim),
                );
            }

            if let Some(ref err) = plugin.error {
                spans.push(Span::raw(format!(" ⚠ {err}")).fg(self.theme.error));
            }

            lines.push(Line::from(spans));

            if is_selected && let Some(ref desc) = plugin.description {
                lines.push(Line::from(
                    Span::raw(format!("    {desc}"))
                        .fg(self.theme.text_dim)
                        .italic(),
                ));
            }
        }

        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::raw("  [Tab] Switch tab  ").fg(self.theme.text_dim),
            Span::raw("[Enter] Toggle  ").fg(self.theme.text_dim),
            Span::raw("[Esc] Close").fg(self.theme.text_dim),
        ]));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Plugins ")
            .border_style(ratatui::style::Style::default().fg(self.theme.border_focused));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
