//! Status bar widget — model, tokens, mode, MCP info.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::state::AppState;
use crate::theme::Theme;

/// Bottom status bar.
pub struct StatusBar<'a> {
    state: &'a AppState,
    theme: &'a Theme,
}

impl<'a> StatusBar<'a> {
    pub fn new(state: &'a AppState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut parts = Vec::new();

        // Model
        parts.push(
            Span::raw(format!(" {}", self.state.session.model))
                .fg(self.theme.primary)
                .bold(),
        );

        // Fast mode
        if self.state.session.fast_mode {
            parts.push(Span::raw(" ⚡").fg(self.theme.warning));
        }

        // Permission mode
        parts.push(Span::raw(" | ").fg(self.theme.border));
        parts.push(
            Span::raw(format!("{:?}", self.state.session.permission_mode)).fg(self.theme.text_dim),
        );

        // Token usage
        let tokens = &self.state.session.token_usage;
        let total = tokens.input_tokens + tokens.output_tokens;
        if total > 0 {
            parts.push(Span::raw(" | ").fg(self.theme.border));
            if total >= 1000 {
                parts.push(
                    Span::raw(format!("{:.1}k tok", total as f64 / 1000.0)).fg(self.theme.text_dim),
                );
            } else {
                parts.push(Span::raw(format!("{total} tok")).fg(self.theme.text_dim));
            }
        }

        // Cache read tokens
        if tokens.cache_read_tokens > 0 {
            parts.push(
                Span::raw(format!(" ({}cr)", tokens.cache_read_tokens)).fg(self.theme.text_dim),
            );
        }

        // MCP servers
        let mcp_count = self.state.session.connected_mcp_count();
        if mcp_count > 0 {
            parts.push(Span::raw(" | ").fg(self.theme.border));
            parts.push(Span::raw(format!("{mcp_count} MCP")).fg(self.theme.text_dim));
        }

        // Cost
        if self.state.session.estimated_cost_cents > 0 {
            parts.push(Span::raw(" | ").fg(self.theme.border));
            let cost = self.state.session.estimated_cost_cents as f64 / 100.0;
            parts.push(Span::raw(format!("${cost:.2}")).fg(self.theme.text_dim));
        }

        // Message count
        parts.push(Span::raw(" | ").fg(self.theme.border));
        parts.push(
            Span::raw(format!("{} msgs", self.state.session.messages.len()))
                .fg(self.theme.text_dim),
        );

        let line = Line::from(parts);
        let bar = Paragraph::new(line).style(Style::default().bg(self.theme.border));
        bar.render(area, buf);
    }
}
