//! Symbol suggestion popup widget.
//!
//! Displays a dropdown list of symbol suggestions for @# autocomplete.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::widgets::Widget;

use cocode_symbol_search::SymbolKind;

use crate::state::SymbolSuggestionItem;
use crate::state::SymbolSuggestionState;
use crate::theme::Theme;
use crate::widgets::suggestion_popup::SuggestionPopup;
use crate::widgets::suggestion_popup::SuggestionRenderer;

/// Symbol-specific suggestion renderer.
struct SymbolRenderer;

/// Get the color for a symbol kind badge.
fn kind_color(kind: SymbolKind) -> Color {
    match kind {
        SymbolKind::Function | SymbolKind::Method => Color::Cyan,
        SymbolKind::Struct | SymbolKind::Class | SymbolKind::Enum => Color::Yellow,
        SymbolKind::Interface | SymbolKind::Type => Color::Magenta,
        SymbolKind::Module => Color::Blue,
        SymbolKind::Constant => Color::DarkGray,
        SymbolKind::Other => Color::DarkGray,
    }
}

impl SuggestionRenderer for SymbolRenderer {
    type Item = SymbolSuggestionItem;

    fn border_color(&self, theme: &Theme) -> Color {
        theme.success
    }

    fn title_prefix(&self) -> &str {
        "@#"
    }

    fn popup_width(&self) -> u16 {
        70
    }

    fn render_item(
        &self,
        buf: &mut Buffer,
        item: &SymbolSuggestionItem,
        is_selected: bool,
        x: u16,
        y: u16,
        width: u16,
        style: Style,
        theme: &Theme,
    ) {
        // Render kind badge (colored)
        let kind_label = item.kind.label();
        let badge_style = if is_selected {
            style
        } else {
            Style::default().fg(kind_color(item.kind))
        };
        let padded_kind = format!("{kind_label:<8}");
        buf.set_string(x, y, &padded_kind, badge_style);

        // Render symbol name with match highlighting
        let name_x = x + 8;
        if is_selected {
            buf.set_string(name_x, y, &item.name, style.bold());
        } else {
            for (char_idx, c) in item.name.chars().enumerate() {
                let is_match = item.match_indices.contains(&char_idx);
                let char_style = if is_match {
                    style.bold().fg(theme.success)
                } else {
                    style
                };
                let cx = name_x + char_idx as u16;
                if cx < x + width {
                    buf.set_string(cx, y, c.to_string(), char_style);
                }
            }
        }

        // Render file location (right-aligned, dim)
        let location = format!("{}:{}", item.file_path, item.line);
        let loc_width = location.len() as u16;
        let name_end = name_x + item.name.len() as u16 + 2;
        let right_edge = x + width;
        let loc_x = right_edge.saturating_sub(loc_width + 1);
        if loc_x > name_end {
            let loc_style = if is_selected { style } else { style.dim() };
            buf.set_string(loc_x, y, &location, loc_style);
        }
    }

    fn hint_loading(&self) -> &str {
        "Indexing symbols..."
    }

    fn hint_empty(&self) -> &str {
        "No matching symbols"
    }
}

/// Symbol suggestion popup widget.
///
/// Renders a dropdown list of symbol suggestions above the input area.
pub struct SymbolSuggestionPopup<'a> {
    inner: SuggestionPopup<'a, SymbolRenderer>,
}

impl<'a> SymbolSuggestionPopup<'a> {
    /// Create a new symbol suggestion popup.
    pub fn new(state: &'a SymbolSuggestionState, theme: &'a Theme) -> Self {
        Self {
            inner: SuggestionPopup::new(state, theme, SymbolRenderer),
        }
    }

    /// Calculate the area for the popup based on input position.
    pub fn calculate_area(&self, input_area: Rect, terminal_height: u16) -> Rect {
        self.inner.calculate_area(input_area, terminal_height)
    }
}

impl Widget for SymbolSuggestionPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.inner.render(area, buf);
    }
}

#[cfg(test)]
#[path = "symbol_suggestion_popup.test.rs"]
mod tests;
