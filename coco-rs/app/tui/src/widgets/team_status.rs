//! Team status indicator — shows teammate count in status bar.
//!
//! TS: components/teams/TeamStatus.tsx
//!
//! Displays "X teammate(s)" with optional selection hint.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::theme::Theme;

/// Team status indicator for the status bar.
pub struct TeamStatusWidget<'a> {
    teammate_count: i32,
    selected: bool,
    show_hint: bool,
    theme: &'a Theme,
}

impl<'a> TeamStatusWidget<'a> {
    pub fn new(teammate_count: i32, theme: &'a Theme) -> Self {
        Self {
            teammate_count,
            selected: false,
            show_hint: false,
            theme,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn show_hint(mut self, show: bool) -> Self {
        self.show_hint = show;
        self
    }
}

impl Widget for TeamStatusWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.teammate_count == 0 {
            return;
        }

        let label = if self.teammate_count == 1 {
            "1 teammate".to_string()
        } else {
            format!("{} teammates", self.teammate_count)
        };

        let mut spans = vec![Span::raw(label).fg(self.theme.accent)];

        if self.selected && self.show_hint {
            spans.push(Span::raw(" · Enter to view").dim());
        }

        let line = Line::from(spans);
        Paragraph::new(line).render(area, buf);
    }
}
