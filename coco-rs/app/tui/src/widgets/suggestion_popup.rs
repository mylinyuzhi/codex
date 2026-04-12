//! Shared suggestion popup widget for autocomplete systems.
//!
//! Used by file (@path), skill (/command), agent (@agent-*),
//! and symbol (@#symbol) autocomplete systems.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::theme::Theme;

/// A suggestion item for the popup.
#[derive(Debug, Clone)]
pub struct SuggestionItem {
    /// Display text.
    pub label: String,
    /// Optional description (shown dimmed).
    pub description: Option<String>,
}

/// Suggestion popup widget.
pub struct SuggestionPopup<'a> {
    items: &'a [SuggestionItem],
    selected: i32,
    title: &'a str,
    theme: &'a Theme,
    max_visible: i32,
}

impl<'a> SuggestionPopup<'a> {
    pub fn new(items: &'a [SuggestionItem], title: &'a str, theme: &'a Theme) -> Self {
        Self {
            items,
            selected: 0,
            title,
            theme,
            max_visible: 10,
        }
    }

    pub fn selected(mut self, index: i32) -> Self {
        self.selected = index;
        self
    }

    pub fn max_visible(mut self, max: i32) -> Self {
        self.max_visible = max;
        self
    }
}

impl Widget for SuggestionPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.items.is_empty() {
            return;
        }

        let visible_count = (self.items.len() as i32).min(self.max_visible);
        let popup_height = (visible_count + 2) as u16; // +2 for borders
        let popup_width = area.width.min(50);

        // Position above the input area
        let y = area.y.saturating_sub(popup_height);
        let popup_area = Rect::new(area.x, y, popup_width, popup_height);

        Clear.render(popup_area, buf);

        let mut lines: Vec<Line> = Vec::new();

        // Calculate scroll window
        let start = if self.selected >= visible_count {
            (self.selected - visible_count + 1) as usize
        } else {
            0
        };
        let end = (start + visible_count as usize).min(self.items.len());

        for (i, item) in self.items[start..end].iter().enumerate() {
            let actual_idx = (start + i) as i32;
            let is_selected = actual_idx == self.selected;

            let style = if is_selected {
                Style::default().fg(self.theme.primary).bold()
            } else {
                Style::default().fg(self.theme.text)
            };

            let marker = if is_selected { "▸ " } else { "  " };

            let mut spans = vec![Span::raw(marker), Span::styled(&item.label, style)];

            if let Some(ref desc) = item.description {
                spans.push(Span::raw(" — ").fg(self.theme.text_dim));
                spans.push(Span::raw(desc.as_str()).fg(self.theme.text_dim));
            }

            lines.push(Line::from(spans));
        }

        let popup = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", self.title))
                .border_style(Style::default().fg(self.theme.border_focused)),
        );
        popup.render(popup_area, buf);
    }
}
