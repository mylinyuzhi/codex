//! File suggestion popup widget.
//!
//! Displays a dropdown list of file suggestions for @mention autocomplete.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::widgets::Widget;

use crate::state::FileSuggestionItem;
use crate::state::FileSuggestionState;
use crate::theme::Theme;
use crate::widgets::suggestion_popup::SuggestionPopup;
use crate::widgets::suggestion_popup::SuggestionRenderer;

/// File-specific suggestion renderer.
struct FileRenderer;

impl SuggestionRenderer for FileRenderer {
    type Item = FileSuggestionItem;

    fn border_color(&self, theme: &Theme) -> Color {
        theme.primary
    }

    fn title_prefix(&self) -> &str {
        "@"
    }

    fn popup_width(&self) -> u16 {
        60
    }

    fn render_item(
        &self,
        buf: &mut Buffer,
        item: &FileSuggestionItem,
        is_selected: bool,
        x: u16,
        y: u16,
        _width: u16,
        style: Style,
        theme: &Theme,
    ) {
        if is_selected {
            let suffix = if item.is_directory { "/" } else { "" };
            let display = format!("{}{suffix}", item.display_text);
            buf.set_string(x, y, &display, style);
        } else {
            // Apply match highlighting
            let mut cx = x;
            for (char_idx, c) in item.display_text.chars().enumerate() {
                let is_match = item.match_indices.contains(&(char_idx as i32));
                let char_style = if is_match {
                    style.bold().fg(theme.primary)
                } else {
                    style
                };
                buf.set_string(cx, y, c.to_string(), char_style);
                cx += 1;
            }

            if item.is_directory {
                buf.set_string(cx, y, "/", style.dim());
            }
        }
    }

    fn hint_loading(&self) -> &str {
        "Searching..."
    }

    fn hint_empty(&self) -> &str {
        "No matches"
    }
}

/// File suggestion popup widget.
///
/// Renders a dropdown list of file suggestions below the input area.
pub struct FileSuggestionPopup<'a> {
    inner: SuggestionPopup<'a, FileRenderer>,
}

impl<'a> FileSuggestionPopup<'a> {
    /// Create a new file suggestion popup.
    pub fn new(state: &'a FileSuggestionState, theme: &'a Theme) -> Self {
        Self {
            inner: SuggestionPopup::new(state, theme, FileRenderer),
        }
    }

    /// Calculate the area for the popup based on input position.
    pub fn calculate_area(&self, input_area: Rect, terminal_height: u16) -> Rect {
        self.inner.calculate_area(input_area, terminal_height)
    }
}

impl Widget for FileSuggestionPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.inner.render(area, buf);
    }
}

#[cfg(test)]
#[path = "file_suggestion_popup.test.rs"]
mod tests;
