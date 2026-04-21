//! Verification-nudge banner.
//!
//! Shown above the input area when `SessionState::verification_nudge_pending`
//! is true. Mirrors the 3+ completed-tasks-without-verify check from
//! both `TodoWriteTool` (V1) and `TaskUpdateTool` (V2) — the tool has
//! already flagged the state, so the banner just renders it.
//!
//! TS parity: the V1/V2 tool-result messages embed the nudge inline in
//! the model's tool-result content. The banner surfaces the same
//! signal to the *user* so they're aware the model was asked to spawn
//! a verification agent.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::theme::Theme;
use crate::widgets::lifecycle_banner::render_banner_row;

pub struct VerificationNudgeBanner<'a> {
    theme: &'a Theme,
}

impl<'a> VerificationNudgeBanner<'a> {
    pub fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    pub fn should_display(pending: bool) -> bool {
        pending
    }
}

impl Widget for VerificationNudgeBanner<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let parts = vec![
            Span::styled(
                t!("verification_nudge.label").to_string(),
                Style::default().fg(self.theme.warning).bold(),
            ),
            Span::styled(
                t!("verification_nudge.message").to_string(),
                Style::default().fg(self.theme.text_dim),
            ),
        ];
        render_banner_row(parts, self.theme, area, buf);
    }
}

#[cfg(test)]
#[path = "verification_nudge_banner.test.rs"]
mod tests;
