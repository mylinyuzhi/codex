//! Queued-commands footer strip.
//!
//! Renders the contents of `SessionState::queued_commands` — commands the
//! user typed while the agent was busy that will process once the current
//! turn ends. Displays as a compact single-row footer showing the count
//! and a preview of the first queued command.
//!
//! TS reference: src/components/QueueIndicator.tsx /
//! PromptInputQueuedCommands.tsx.

use std::collections::VecDeque;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::presentation::styles::UiStyles;
use crate::state::session::QueuedCommandDisplay;

pub struct QueueStatusWidget<'a> {
    queued: &'a VecDeque<QueuedCommandDisplay>,
    styles: UiStyles<'a>,
}

impl<'a> QueueStatusWidget<'a> {
    pub fn new(queued: &'a VecDeque<QueuedCommandDisplay>, styles: UiStyles<'a>) -> Self {
        Self { queued, styles }
    }

    pub fn should_display(queued: &VecDeque<QueuedCommandDisplay>) -> bool {
        !queued.is_empty()
    }
}

impl Widget for QueueStatusWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || self.queued.is_empty() {
            return;
        }
        let count = self.queued.len();
        let mut parts = vec![Span::styled(
            t!("queue_status.count", count = count).to_string(),
            Style::default().fg(self.styles.accent()).bold(),
        )];
        if let Some(first) = self.queued.front() {
            let preview: String = first.preview.chars().take(48).collect();
            parts.push(Span::styled(
                t!("queue_status.next_preview", preview = preview).to_string(),
                Style::default().fg(self.styles.dim()),
            ));
            if count > 1 {
                parts.push(Span::styled(
                    t!("queue_status.more", count = count - 1).to_string(),
                    Style::default().fg(self.styles.dim()),
                ));
            }
        }
        Paragraph::new(Line::from(parts))
            .style(Style::default().bg(self.styles.border()))
            .render(area, buf);
    }
}

#[cfg(test)]
#[path = "queue_status_widget.test.rs"]
mod tests;
