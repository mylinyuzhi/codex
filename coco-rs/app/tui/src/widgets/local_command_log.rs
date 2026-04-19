//! Local-command echo log.
//!
//! Renders the most recent `LocalCommandOutput` entries in a scrollable
//! panel. The handler keeps the last 50 entries in
//! `SessionState::local_command_output`; this widget presents them as a
//! tail-first REPL echo so users can see what external commands (slash
//! commands, shell pipelines executed by the host) produced.
//!
//! TS reference: src/components/LocalCommandOutput.tsx.

use std::collections::VecDeque;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

use crate::i18n::t;
use crate::theme::Theme;

pub struct LocalCommandLog<'a> {
    entries: &'a VecDeque<String>,
    theme: &'a Theme,
    max_rows: u16,
}

impl<'a> LocalCommandLog<'a> {
    pub fn new(entries: &'a VecDeque<String>, theme: &'a Theme) -> Self {
        Self {
            entries,
            theme,
            max_rows: 8,
        }
    }

    pub fn max_rows(mut self, max_rows: u16) -> Self {
        self.max_rows = max_rows;
        self
    }

    pub fn should_display(entries: &VecDeque<String>) -> bool {
        !entries.is_empty()
    }
}

impl Widget for LocalCommandLog<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        // Keep the most recent entries — drop the head if we overflow
        // the panel so the log reads tail-first.
        let available = area.height.saturating_sub(2).min(self.max_rows) as usize;
        let start = self.entries.len().saturating_sub(available);
        let lines: Vec<Line> = self
            .entries
            .iter()
            .skip(start)
            .map(|entry| {
                Line::from(vec![
                    Span::styled("$ ", Style::default().fg(self.theme.accent).bold()),
                    Span::styled(entry.as_str(), Style::default().fg(self.theme.text)),
                ])
            })
            .collect();

        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(t!("local_command_log.panel_title").to_string())
                    .border_style(Style::default().fg(self.theme.border)),
            )
            .render(area, buf);
    }
}

#[cfg(test)]
#[path = "local_command_log.test.rs"]
mod tests;
