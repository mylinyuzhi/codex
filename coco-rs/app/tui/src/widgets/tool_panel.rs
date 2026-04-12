//! Tool panel widget — running/completed tool executions in side panel.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::constants;
use crate::state::session::ToolExecution;
use crate::state::session::ToolStatus;
use crate::theme::Theme;

/// Side panel showing tool execution status.
pub struct ToolPanel<'a> {
    tools: &'a [ToolExecution],
    theme: &'a Theme,
}

impl<'a> ToolPanel<'a> {
    pub fn new(tools: &'a [ToolExecution], theme: &'a Theme) -> Self {
        Self { tools, theme }
    }
}

impl Widget for ToolPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        let display_count = self
            .tools
            .len()
            .min(constants::MAX_TOOL_PANEL_DISPLAY as usize);
        let start = self.tools.len().saturating_sub(display_count);

        for tool in &self.tools[start..] {
            let (icon, color) = match tool.status {
                ToolStatus::Running => ("⏳", self.theme.tool_running),
                ToolStatus::Completed => ("✓", self.theme.tool_completed),
                ToolStatus::Failed => ("✗", self.theme.tool_error),
            };

            let elapsed = tool.elapsed();
            let elapsed_str = if elapsed.as_secs() >= 60 {
                format!("{}m{}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
            } else if elapsed.as_secs() > 0 {
                format!("{}s", elapsed.as_secs())
            } else {
                format!("{}ms", elapsed.as_millis())
            };

            let desc = tool.description.as_deref().unwrap_or(&tool.name);
            let max_chars = constants::TOOL_DESCRIPTION_MAX_CHARS as usize;
            let truncated = if desc.len() > max_chars {
                format!("{}...", &desc[..max_chars - 3])
            } else {
                desc.to_string()
            };

            lines.push(Line::from(vec![
                Span::raw(format!("{icon} ")).fg(color),
                Span::raw(truncated).fg(self.theme.text),
                Span::raw(format!(" ({elapsed_str})")).fg(self.theme.text_dim),
            ]));
        }

        if lines.is_empty() {
            lines.push(Line::from(
                Span::raw("  No active tools").fg(self.theme.text_dim),
            ));
        }

        let panel = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::LEFT)
                .title(" Tools ")
                .border_style(Style::default().fg(self.theme.border)),
        );
        panel.render(area, buf);
    }
}
