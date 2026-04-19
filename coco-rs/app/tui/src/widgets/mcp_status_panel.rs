//! MCP server status panel.
//!
//! Renders a compact list of connected MCP servers with their connection
//! state and tool count. Populated by `McpStartupStatus` events
//! (`SessionState::mcp_servers`).
//!
//! TS reference: src/components/McpStatusBar.tsx / McpServerList.tsx.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::state::session::McpServerStatus;
use crate::theme::Theme;

pub struct McpStatusPanel<'a> {
    servers: &'a [McpServerStatus],
    theme: &'a Theme,
}

impl<'a> McpStatusPanel<'a> {
    pub fn new(servers: &'a [McpServerStatus], theme: &'a Theme) -> Self {
        Self { servers, theme }
    }

    pub fn should_display(servers: &[McpServerStatus]) -> bool {
        !servers.is_empty()
    }
}

impl Widget for McpStatusPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let lines: Vec<Line> = self
            .servers
            .iter()
            .map(|s| {
                let (dot, color) = if s.connected {
                    ("●", self.theme.success)
                } else {
                    ("○", self.theme.text_dim)
                };
                Line::from(vec![
                    Span::styled(format!(" {dot} "), Style::default().fg(color)),
                    Span::styled(s.name.as_str(), Style::default().fg(self.theme.text).bold()),
                    Span::styled(
                        format!("  {}", t!("mcp.tools_count", count = s.tool_count)),
                        Style::default().fg(self.theme.text_dim),
                    ),
                ])
            })
            .collect();

        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", t!("mcp.panel_title")))
                    .border_style(Style::default().fg(self.theme.border)),
            )
            .render(area, buf);
    }
}

#[cfg(test)]
#[path = "mcp_status_panel.test.rs"]
mod tests;
