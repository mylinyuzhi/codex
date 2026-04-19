//! Stream-stall indicator banner.
//!
//! Rendered while `SessionState::stream_stall` is true (set by the
//! `StreamStallDetected` notification and cleared on the next successful
//! chunk or turn start). Signals to the user that the model's stream has
//! paused unexpectedly and the agent is still waiting.
//!
//! TS reference: StreamHealthIndicator variants — either a dedicated
//! indicator or a pause state on the existing spinner.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::theme::Theme;
use crate::widgets::lifecycle_banner::render_banner_row;

pub struct StreamStallIndicator<'a> {
    theme: &'a Theme,
}

impl<'a> StreamStallIndicator<'a> {
    pub fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    pub fn should_display(stream_stall: bool) -> bool {
        stream_stall
    }
}

impl Widget for StreamStallIndicator<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let parts = vec![
            Span::styled(
                t!("stream_stall.label").to_string(),
                Style::default().fg(self.theme.warning).bold(),
            ),
            Span::styled(
                t!("stream_stall.message").to_string(),
                Style::default().fg(self.theme.text_dim),
            ),
        ];
        render_banner_row(parts, self.theme, area, buf);
    }
}

#[cfg(test)]
#[path = "stream_stall_indicator.test.rs"]
mod tests;
