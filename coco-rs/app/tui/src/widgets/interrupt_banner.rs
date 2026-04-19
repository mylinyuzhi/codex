//! Turn-interrupt indicator.
//!
//! Shows a single-row banner while `SessionState::was_interrupted` is true,
//! reminding the user that the last turn was cancelled before completion.
//! Clears automatically when a new turn starts (the session handler resets
//! `was_interrupted` on `TurnStarted`).
//!
//! TS reference: src/components/InterruptBanner.tsx — a top-of-screen
//! indicator during an interrupted state.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::theme::Theme;
use crate::widgets::lifecycle_banner::render_banner_row;

pub struct InterruptBanner<'a> {
    theme: &'a Theme,
}

impl<'a> InterruptBanner<'a> {
    pub fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    pub fn should_display(was_interrupted: bool) -> bool {
        was_interrupted
    }
}

impl Widget for InterruptBanner<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let parts = vec![
            Span::styled(
                t!("interrupt_banner.label").to_string(),
                Style::default().fg(self.theme.warning).bold(),
            ),
            Span::styled(
                t!("interrupt_banner.message").to_string(),
                Style::default().fg(self.theme.text_dim),
            ),
        ];
        render_banner_row(parts, self.theme, area, buf);
    }
}

#[cfg(test)]
#[path = "interrupt_banner.test.rs"]
mod tests;
