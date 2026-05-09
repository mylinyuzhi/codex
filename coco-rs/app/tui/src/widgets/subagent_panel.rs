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
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;
use crate::state::session::SubagentInstance;
use crate::state::session::SubagentStatus;
use crate::theme::Theme;

/// Number of recent message lines per teammate when preview is on.
/// TS uses 3 (`getMessagePreview` in `TeammateSpinnerLine.tsx`); coco-rs
/// matches.
const PREVIEW_LINES_PER_TEAMMATE: usize = 3;

/// Side panel showing subagent status.
pub struct SubagentPanel<'a> {
    subagents: &'a [SubagentInstance],
    focused_index: Option<i32>,
    theme: &'a Theme,
    /// When set + non-empty, the panel renders up to
    /// [`PREVIEW_LINES_PER_TEAMMATE`] recent message lines per agent.
    /// TS `showTeammateMessagePreview` (`TeammateSpinnerTree`).
    messages_for_preview: Option<&'a [ChatMessage]>,
}

impl<'a> SubagentPanel<'a> {
    pub fn new(subagents: &'a [SubagentInstance], theme: &'a Theme) -> Self {
        Self {
            subagents,
            focused_index: None,
            theme,
            messages_for_preview: None,
        }
    }

    pub fn focused_index(mut self, index: Option<i32>) -> Self {
        self.focused_index = index;
        self
    }

    /// Enable per-teammate message preview lines (TS
    /// `showTeammateMessagePreview`). Pass the full session message
    /// list — the panel filters per teammate.
    pub fn message_preview(mut self, messages: &'a [ChatMessage]) -> Self {
        self.messages_for_preview = Some(messages);
        self
    }
}

/// Last `n` lines from `teammate_id`'s recent messages in this
/// session. Walks newest-first so the most recent activity wins, then
/// reverses so the rendered lines read top-to-bottom in chronological
/// order. Mirrors TS `getMessagePreview` (`TeammateSpinnerLine.tsx`).
fn last_preview_lines<'a>(
    messages: &'a [ChatMessage],
    teammate_id: &str,
    n: usize,
) -> Vec<&'a str> {
    let mut lines: Vec<&str> = Vec::new();
    for msg in messages.iter().rev() {
        let MessageContent::TeammateMessage { teammate, content } = &msg.content else {
            continue;
        };
        if teammate != teammate_id {
            continue;
        }
        for line in content.lines().rev() {
            if lines.len() >= n {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            lines.push(trimmed);
        }
        if lines.len() >= n {
            break;
        }
    }
    lines.reverse();
    lines
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

            // TS-parity: when `showTeammateMessagePreview` is on,
            // each spinner line is followed by up to N indented
            // recent-activity lines from this teammate's messages.
            if let Some(msgs) = self.messages_for_preview {
                for preview in last_preview_lines(msgs, &agent.agent_id, PREVIEW_LINES_PER_TEAMMATE)
                {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::raw(preview).fg(self.theme.text_dim),
                    ]));
                }
            }
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
