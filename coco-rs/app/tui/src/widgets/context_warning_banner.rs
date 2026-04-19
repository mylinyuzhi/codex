//! Context-usage warning banner.
//!
//! Shows a single-row banner when `SessionState::context_usage_percent`
//! crosses the warning threshold (default 80%). Escalates to a stronger
//! color above the critical threshold (95%).
//!
//! TS reference: src/components/TokenWarning.tsx — live bar that reminds
//! users their context window is filling up so they can checkpoint or let
//! autocompact engage.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::theme::Theme;
use crate::widgets::lifecycle_banner::render_banner_row;

/// Percentage (inclusive) at which we show the warning banner.
pub const CONTEXT_WARNING_THRESHOLD: f64 = 80.0;
/// Percentage (inclusive) at which we escalate to the critical color.
pub const CONTEXT_CRITICAL_THRESHOLD: f64 = 95.0;

pub struct ContextWarningBanner<'a> {
    percent: f64,
    theme: &'a Theme,
}

impl<'a> ContextWarningBanner<'a> {
    pub fn new(percent: f64, theme: &'a Theme) -> Self {
        Self { percent, theme }
    }

    pub fn should_display(percent: Option<f64>) -> bool {
        percent.is_some_and(|p| p >= CONTEXT_WARNING_THRESHOLD)
    }
}

impl Widget for ContextWarningBanner<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let color = if self.percent >= CONTEXT_CRITICAL_THRESHOLD {
            self.theme.error
        } else {
            self.theme.warning
        };
        let parts = vec![
            Span::styled(
                t!("context_warning.label").to_string(),
                Style::default().fg(color).bold(),
            ),
            Span::styled(
                t!(
                    "context_warning.percent",
                    percent = format!("{:.0}", self.percent)
                )
                .to_string(),
                Style::default().fg(color).bold(),
            ),
            Span::styled(
                t!("context_warning.message").to_string(),
                Style::default().fg(self.theme.text_dim),
            ),
        ];
        render_banner_row(parts, self.theme, area, buf);
    }
}

#[cfg(test)]
#[path = "context_warning_banner.test.rs"]
mod tests;
