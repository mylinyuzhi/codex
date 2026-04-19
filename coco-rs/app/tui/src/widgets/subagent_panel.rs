//! Subagent panel widget — displays spawned agent instances.

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

use crate::i18n::t;
use crate::state::session::SubagentInstance;
use crate::state::session::SubagentStatus;
use crate::theme::Theme;

/// Side panel showing subagent status.
pub struct SubagentPanel<'a> {
    subagents: &'a [SubagentInstance],
    focused_index: Option<i32>,
    theme: &'a Theme,
}

impl<'a> SubagentPanel<'a> {
    pub fn new(subagents: &'a [SubagentInstance], theme: &'a Theme) -> Self {
        Self {
            subagents,
            focused_index: None,
            theme,
        }
    }

    pub fn focused_index(mut self, index: Option<i32>) -> Self {
        self.focused_index = index;
        self
    }
}

impl Widget for SubagentPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        for (i, agent) in self.subagents.iter().enumerate() {
            let is_focused = self.focused_index == Some(i as i32);

            let (icon, color) = match agent.status {
                SubagentStatus::Running => ("●", self.theme.tool_running),
                SubagentStatus::Completed => ("✓", self.theme.tool_completed),
                SubagentStatus::Backgrounded => ("◐", self.theme.text_dim),
                SubagentStatus::Failed => ("✗", self.theme.tool_error),
            };

            let focus_marker = if is_focused { "▸ " } else { "  " };

            lines.push(Line::from(vec![
                Span::raw(focus_marker),
                Span::raw(format!("{icon} ")).fg(color),
                Span::raw(agent.description.as_str()).fg(self.theme.text),
                Span::raw(format!(" ({})", agent.agent_type)).fg(self.theme.text_dim),
            ]));
        }

        if lines.is_empty() {
            lines.push(Line::from(
                Span::raw(format!("  {}", t!("subagent.no_agents"))).fg(self.theme.text_dim),
            ));
        }

        let panel = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::LEFT | Borders::TOP)
                .title(format!(" {} ", t!("subagent.title")))
                .border_style(Style::default().fg(self.theme.border)),
        );
        panel.render(area, buf);
    }
}
