//! Model-fallback persistent banner.
//!
//! TS reference: src/components/ModelBanner.tsx / BridgeDialog.tsx — shows
//! prominently at the top of the screen when the session falls back from a
//! preferred model to a lighter one (typically due to overload or rate
//! limiting). Coupled with `SessionState::model_fallback_banner`, which
//! holds the "{from} → {to}" string while the fallback is in effect.
//!
//! Rendered as a single row below the header, next to or above the
//! rate-limit banner if both are active.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::theme::Theme;
use crate::widgets::lifecycle_banner::render_banner_row;

/// Model-fallback persistent banner. Occupies a single row.
pub struct ModelFallbackBanner<'a> {
    /// The `"from → to"` summary provided by the protocol handler.
    description: &'a str,
    theme: &'a Theme,
}

impl<'a> ModelFallbackBanner<'a> {
    pub fn new(description: &'a str, theme: &'a Theme) -> Self {
        Self { description, theme }
    }

    /// Whether to allocate a row for this banner. Handlers keep
    /// `model_fallback_banner` populated only while the fallback is active
    /// (cleared on `ModelFallbackCompleted`), so presence == display.
    pub fn should_display(banner: Option<&str>) -> bool {
        banner.is_some_and(|s| !s.is_empty())
    }
}

impl Widget for ModelFallbackBanner<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let parts = vec![
            Span::styled(
                t!("model_fallback.label").to_string(),
                Style::default().fg(self.theme.warning).bold(),
            ),
            Span::styled(
                self.description,
                Style::default().fg(self.theme.text).bold(),
            ),
        ];
        render_banner_row(parts, self.theme, area, buf);
    }
}

#[cfg(test)]
#[path = "model_fallback_banner.test.rs"]
mod tests;
