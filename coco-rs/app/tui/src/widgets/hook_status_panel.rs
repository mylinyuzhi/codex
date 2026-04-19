//! Active hook executions panel.
//!
//! Renders running/completed hooks from `SessionState::active_hooks`.
//! Populated by `HookStarted`/`HookProgress`/`HookResponse` events.
//!
//! TS reference: src/components/HookPanel.tsx / HookStreamView.tsx.

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
use crate::state::session::HookEntry;
use crate::state::session::HookEntryStatus;
use crate::theme::Theme;

pub struct HookStatusPanel<'a> {
    hooks: &'a [HookEntry],
    theme: &'a Theme,
}

impl<'a> HookStatusPanel<'a> {
    pub fn new(hooks: &'a [HookEntry], theme: &'a Theme) -> Self {
        Self { hooks, theme }
    }

    pub fn should_display(hooks: &[HookEntry]) -> bool {
        !hooks.is_empty()
    }
}

impl Widget for HookStatusPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let lines: Vec<Line> = self
            .hooks
            .iter()
            .map(|h| {
                let (glyph, color) = match h.status {
                    HookEntryStatus::Running => ("◌", self.theme.warning),
                    HookEntryStatus::Completed => ("✓", self.theme.success),
                    HookEntryStatus::Failed => ("✗", self.theme.error),
                };
                let mut spans = vec![
                    Span::styled(format!(" {glyph} "), Style::default().fg(color)),
                    Span::styled(
                        h.hook_name.as_str(),
                        Style::default().fg(self.theme.text).bold(),
                    ),
                ];
                if let Some(out) = h.output.as_deref().filter(|s| !s.is_empty()) {
                    // Trim to one visual line so the panel doesn't go
                    // jagged; the full hook output stays in history.
                    let snippet: String =
                        out.lines().next().unwrap_or("").chars().take(60).collect();
                    spans.push(Span::styled(
                        format!("  {snippet}"),
                        Style::default().fg(self.theme.text_dim),
                    ));
                }
                Line::from(spans)
            })
            .collect();

        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", t!("hook.panel_title")))
                    .border_style(Style::default().fg(self.theme.border)),
            )
            .render(area, buf);
    }
}

#[cfg(test)]
#[path = "hook_status_panel.test.rs"]
mod tests;
