//! Rate-limit persistent banner.
//!
//! TS reference: src/components/RateLimitBanner.tsx — a top-of-screen
//! persistent banner shown while the session is rate-limited. Unlike a
//! transient toast (which auto-expires after a few seconds), this banner
//! stays visible until the rate limit clears so users can't miss the
//! blocking condition.
//!
//! Rendered as a single line between the header bar and main area when
//! `SessionState::rate_limit_info` is populated and `remaining == Some(0)`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::state::session::RateLimitInfo;
use crate::theme::Theme;
use crate::widgets::lifecycle_banner::render_banner_row;

/// Rate-limit persistent banner. Occupies a single row.
pub struct RateLimitPanel<'a> {
    info: &'a RateLimitInfo,
    theme: &'a Theme,
    now_unix_secs: i64,
}

impl<'a> RateLimitPanel<'a> {
    pub fn new(info: &'a RateLimitInfo, theme: &'a Theme) -> Self {
        let now_unix_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Self {
            info,
            theme,
            now_unix_secs,
        }
    }

    /// Override "now" for deterministic snapshot tests.
    #[cfg(test)]
    pub fn with_now(mut self, now_unix_secs: i64) -> Self {
        self.now_unix_secs = now_unix_secs;
        self
    }

    /// Whether this panel should be shown at all. The handler populates
    /// `rate_limit_info` on every `RateLimit` event (not just zeros) so the
    /// banner self-filters: only show when `remaining == Some(0)` (true
    /// blocking state). Renderers elsewhere can read
    /// `should_display(&session.rate_limit_info)` to decide on allocating
    /// the row.
    pub fn should_display(info: Option<&RateLimitInfo>) -> bool {
        matches!(info, Some(i) if i.remaining == Some(0))
    }
}

impl Widget for RateLimitPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut parts = Vec::new();
        parts.push(Span::styled(
            t!("rate_limit.label").to_string(),
            Style::default().fg(self.theme.error).bold(),
        ));

        if let Some(provider) = self.info.provider.as_deref() {
            parts.push(Span::raw(" "));
            parts.push(Span::styled(
                format!("[{provider}]"),
                Style::default().fg(self.theme.text_dim),
            ));
        }

        if let Some(reset_at) = self.info.reset_at {
            let remaining_secs = reset_at.saturating_sub(self.now_unix_secs).max(0);
            parts.push(Span::styled(
                t!("rate_limit.reset_in").to_string(),
                Style::default().fg(self.theme.text_dim),
            ));
            parts.push(Span::styled(
                format_duration(remaining_secs),
                Style::default().fg(self.theme.warning).bold(),
            ));
        }

        render_banner_row(parts, self.theme, area, buf);
    }
}

fn format_duration(seconds: i64) -> String {
    if seconds <= 0 {
        return t!("rate_limit.now").to_string();
    }
    let mins = seconds / 60;
    let secs = seconds % 60;
    if mins >= 60 {
        let hours = mins / 60;
        let rem_mins = mins % 60;
        format!("{hours}h {rem_mins}m")
    } else if mins > 0 {
        format!("{mins}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
#[path = "rate_limit_panel.test.rs"]
mod tests;
