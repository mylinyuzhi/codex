//! Tool panel widget — running/completed tool executions in a compact activity surface.

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

use coco_tui_ui::constants;
use crate::i18n::t;
use coco_tui_ui::style::UiStyles;
use crate::state::session::ToolExecution;
use crate::state::session::ToolStatus;

/// Side panel showing tool execution status.
pub struct ToolPanel<'a> {
    tools: &'a [ToolExecution],
    styles: UiStyles<'a>,
}

impl<'a> ToolPanel<'a> {
    pub fn new(tools: &'a [ToolExecution], styles: UiStyles<'a>) -> Self {
        Self { tools, styles }
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
                ToolStatus::Queued => ("◦", self.styles.tool_running()),
                ToolStatus::Running => ("⏳", self.styles.tool_running()),
                ToolStatus::Completed => ("✓", self.styles.tool_completed()),
                ToolStatus::Failed => ("✗", self.styles.tool_error()),
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
            // Width-safe truncation: a raw `&desc[..max_chars-3]` byte slice
            // panics when the cut lands mid-codepoint (CJK/emoji descriptions).
            let max_chars = constants::TOOL_DESCRIPTION_MAX_CHARS as usize;
            let truncated = coco_tui_ui::truncate::truncate_to_width(desc, max_chars);

            lines.push(Line::from(vec![
                Span::raw(format!("{icon} ")).fg(color),
                Span::raw(truncated).fg(self.styles.text()),
                Span::raw(format!(" ({elapsed_str})")).fg(self.styles.dim()),
            ]));
        }

        if lines.is_empty() {
            lines.push(Line::from(
                Span::raw(format!("  {}", t!("tool.no_active"))).fg(self.styles.dim()),
            ));
        }

        let panel = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::LEFT)
                .title(format!(" {} ", t!("tool.title")))
                .border_style(Style::default().fg(self.styles.border())),
        );
        panel.render(area, buf);
    }
}
