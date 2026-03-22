//! Generic suggestion popup widget.
//!
//! Provides shared rendering logic (border, scroll, selection, hints) for all
//! autocomplete popup types. Each autocomplete system implements
//! [`SuggestionRenderer`] to define its item rendering and colors.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

use crate::state::SuggestionState;
use crate::theme::Theme;

/// Maximum number of visible suggestions in the popup.
const MAX_VISIBLE: i32 = 8;

/// Trait for rendering individual suggestion items.
///
/// Each autocomplete type (file, skill, agent, symbol) implements this to
/// define its border color, title prefix, popup width, item rendering, and
/// hint messages.
pub trait SuggestionRenderer {
    /// The suggestion item type.
    type Item;

    /// Border color for the popup.
    fn border_color(&self, theme: &Theme) -> Color;

    /// Title prefix (e.g., "@", "/", "@#").
    fn title_prefix(&self) -> &str;

    /// Popup width (e.g., 60 or 70).
    fn popup_width(&self) -> u16;

    /// Render a single suggestion item to the buffer.
    #[allow(clippy::too_many_arguments)]
    fn render_item(
        &self,
        buf: &mut Buffer,
        item: &Self::Item,
        is_selected: bool,
        x: u16,
        y: u16,
        width: u16,
        style: Style,
        theme: &Theme,
    );

    /// Hint text when loading.
    fn hint_loading(&self) -> &str;

    /// Hint text when no results.
    fn hint_empty(&self) -> &str;
}

/// Generic suggestion popup widget.
///
/// Renders a dropdown list of suggestions with shared calculate_area, scroll
/// offset, selection highlight, and hint line logic.
pub struct SuggestionPopup<'a, R: SuggestionRenderer> {
    state: &'a SuggestionState<R::Item>,
    theme: &'a Theme,
    renderer: R,
}

impl<'a, R: SuggestionRenderer> SuggestionPopup<'a, R> {
    /// Create a new suggestion popup.
    pub fn new(state: &'a SuggestionState<R::Item>, theme: &'a Theme, renderer: R) -> Self {
        Self {
            state,
            theme,
            renderer,
        }
    }

    /// Calculate the area for the popup based on input position.
    pub fn calculate_area(&self, input_area: Rect, terminal_height: u16) -> Rect {
        let suggestion_count = self.state.suggestions.len() as i32;
        let visible_count = suggestion_count.min(MAX_VISIBLE);

        // Height: suggestions + 2 for border + 1 for hint line
        let height = (visible_count as u16 + 3).min(terminal_height / 3);

        // Width: bounded by renderer's preferred width
        let width = input_area.width.min(self.renderer.popup_width()).max(30);

        // Position: above input area
        let x = input_area.x;
        let y = input_area.y.saturating_sub(height);

        // Ensure we don't go off-screen
        let y = if y + height > terminal_height {
            terminal_height.saturating_sub(height)
        } else {
            y
        };

        Rect::new(x, y, width, height)
    }
}

impl<R: SuggestionRenderer> Widget for SuggestionPopup<'_, R> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 10 {
            return;
        }

        // Clear the popup area
        Clear.render(area, buf);

        // Create border with query in title
        let title = format!(" {}{} ", self.renderer.title_prefix(), self.state.query);
        let block = Block::default()
            .title(title.bold())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.renderer.border_color(self.theme)));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 1 {
            return;
        }

        // Calculate visible range (scrolling)
        let total = self.state.suggestions.len() as i32;
        let selected = self.state.selected;
        let visible = (inner.height as i32 - 1).max(1); // -1 for hint line

        // Calculate scroll offset to keep selected item visible
        let scroll_offset = if selected < visible / 2 {
            0
        } else if selected > total - (visible + 1) / 2 {
            (total - visible).max(0)
        } else {
            selected - visible / 2
        };

        // Render suggestions
        let mut y = inner.y;
        for (i, suggestion) in self
            .state
            .suggestions
            .iter()
            .skip(scroll_offset as usize)
            .take(visible as usize)
            .enumerate()
        {
            if y >= inner.y + inner.height - 1 {
                break;
            }

            let global_idx = scroll_offset + i as i32;
            let is_selected = global_idx == selected;

            // Calculate display area for this item
            let item_area = Rect::new(inner.x, y, inner.width, 1);

            // Style based on selection
            let style = if is_selected {
                Style::default()
                    .bg(self.theme.bg_selected)
                    .fg(self.theme.text)
            } else {
                Style::default()
            };

            // Clear line with background
            buf.set_style(item_area, style);

            // Render prefix
            let prefix = if is_selected { "▸ " } else { "  " };
            buf.set_string(inner.x, y, prefix, style);

            // Delegate item rendering to the renderer
            self.renderer.render_item(
                buf,
                suggestion,
                is_selected,
                inner.x + 2,
                y,
                inner.width.saturating_sub(2),
                style,
                self.theme,
            );

            y += 1;
        }

        // Render hint line at bottom
        if inner.height > 1 {
            let hint_y = inner.y + inner.height - 1;
            let hint = if self.state.loading {
                self.renderer.hint_loading()
            } else if self.state.suggestions.is_empty() {
                self.renderer.hint_empty()
            } else {
                "Tab/Enter: Select  Esc: Dismiss"
            };
            buf.set_string(inner.x, hint_y, hint, Style::default().dim());
        }
    }
}
