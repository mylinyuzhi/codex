//! Shared helper for single-row "lifecycle" banners that sit between the
//! header bar and the main area.
//!
//! All banners share the same frame — a single row painted with the theme's
//! border background. Individual banners (context warning, rate limit,
//! interrupt, stream stall, model fallback, permission mode) differ only in
//! the spans they compose. Centralizing the frame keeps the styling rule in
//! one place so every banner looks consistent.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::theme::Theme;

/// Render a banner row: early-return on zero height, wrap spans in a paragraph
/// styled with the theme's border background.
pub(crate) fn render_banner_row(parts: Vec<Span<'_>>, theme: &Theme, area: Rect, buf: &mut Buffer) {
    if area.height == 0 {
        return;
    }
    Paragraph::new(Line::from(parts))
        .style(Style::default().bg(theme.border))
        .render(area, buf);
}
