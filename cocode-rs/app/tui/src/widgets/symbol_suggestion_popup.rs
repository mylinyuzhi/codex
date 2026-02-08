//! Symbol suggestion popup widget.
//!
//! Displays a dropdown list of symbol suggestions for @# autocomplete.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

use cocode_symbol_search::SymbolKind;

use crate::state::SymbolSuggestionState;

/// Maximum number of visible suggestions in the popup.
const MAX_VISIBLE: i32 = 8;

/// Symbol suggestion popup widget.
///
/// Renders a dropdown list of symbol suggestions above the input area.
/// Uses green as the accent color to differentiate from file (cyan),
/// agent (yellow), and skill (magenta) suggestions.
pub struct SymbolSuggestionPopup<'a> {
    state: &'a SymbolSuggestionState,
}

impl<'a> SymbolSuggestionPopup<'a> {
    /// Create a new symbol suggestion popup.
    pub fn new(state: &'a SymbolSuggestionState) -> Self {
        Self { state }
    }

    /// Calculate the area for the popup based on input position.
    ///
    /// The popup appears above the input widget, anchored to the left,
    /// with enough width to show symbol names and file locations.
    pub fn calculate_area(&self, input_area: Rect, terminal_height: u16) -> Rect {
        let suggestion_count = self.state.suggestions.len() as i32;
        let visible_count = suggestion_count.min(MAX_VISIBLE);

        // Height: suggestions + 2 for border + 1 for hint line
        let height = (visible_count as u16 + 3).min(terminal_height / 3);

        // Width: Use most of the input area width (symbols need more room for file paths)
        let width = input_area.width.min(70).max(30);

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

impl Widget for SymbolSuggestionPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 10 {
            return;
        }

        // Clear the popup area
        Clear.render(area, buf);

        // Create border with query in title
        let title = format!(" @#{} ", self.state.query);
        let block = Block::default()
            .title(title.bold())
            .borders(Borders::ALL)
            .border_style(Style::default().green());

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
                Style::default().bg(Color::Green).fg(Color::Black)
            } else {
                Style::default()
            };

            // Clear line with background
            buf.set_style(item_area, style);

            // Build the display: "▸ kind  Name      file_path:line"
            let prefix = if is_selected { "▸ " } else { "  " };
            let kind_label = suggestion.kind.label();

            // Render prefix
            buf.set_string(inner.x, y, prefix, style);

            // Render kind badge (colored)
            let kind_x = inner.x + 2;
            let badge_style = if is_selected {
                style
            } else {
                Style::default().fg(kind_color(suggestion.kind))
            };
            let padded_kind = format!("{kind_label:<8}");
            buf.set_string(kind_x, y, &padded_kind, badge_style);

            // Render symbol name with match highlighting
            let name_x = kind_x + 8;
            if is_selected {
                // When selected, render without match highlight (selection bg is enough)
                buf.set_string(name_x, y, &suggestion.name, style.bold());
            } else {
                // Apply match highlighting
                for (char_idx, c) in suggestion.name.chars().enumerate() {
                    let is_match = suggestion.match_indices.contains(&char_idx);
                    let char_style = if is_match {
                        style.bold().green()
                    } else {
                        style
                    };
                    let x = name_x + char_idx as u16;
                    if x < inner.x + inner.width {
                        buf.set_string(x, y, c.to_string(), char_style);
                    }
                }
            }

            // Render file location (right-aligned, dim)
            let location = format!("{}:{}", suggestion.file_path, suggestion.line);
            let loc_width = location.len() as u16;
            let name_end = name_x + suggestion.name.len() as u16 + 2;
            let loc_x = (inner.x + inner.width).saturating_sub(loc_width + 1);
            if loc_x > name_end {
                let loc_style = if is_selected { style } else { style.dim() };
                buf.set_string(loc_x, y, &location, loc_style);
            }

            y += 1;
        }

        // Render hint line at bottom
        if inner.height > 1 {
            let hint_y = inner.y + inner.height - 1;
            let hint = if self.state.loading {
                "Indexing symbols..."
            } else if self.state.suggestions.is_empty() {
                "No matching symbols"
            } else {
                "Tab/Enter: Select  Esc: Dismiss"
            };
            buf.set_string(inner.x, hint_y, hint, Style::default().dim());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SymbolSuggestionItem;

    fn create_test_state() -> SymbolSuggestionState {
        let mut state = SymbolSuggestionState::new("ModelInfo".to_string(), 0);
        state.update_suggestions(vec![
            SymbolSuggestionItem {
                name: "ModelInfo".to_string(),
                kind: SymbolKind::Struct,
                file_path: "protocol/src/model.rs".to_string(),
                line: 42,
                score: 100,
                match_indices: vec![0, 1, 2, 3, 4, 5, 6, 7, 8],
            },
            SymbolSuggestionItem {
                name: "model_info_new".to_string(),
                kind: SymbolKind::Function,
                file_path: "api/src/client.rs".to_string(),
                line: 156,
                score: 80,
                match_indices: vec![0, 1, 2, 3, 4],
            },
        ]);
        state
    }

    #[test]
    fn test_popup_creation() {
        let state = create_test_state();
        let popup = SymbolSuggestionPopup::new(&state);

        let input_area = Rect::new(0, 20, 80, 3);
        let area = popup.calculate_area(input_area, 24);

        assert!(area.width >= 30);
        assert!(area.height >= 3);
    }

    #[test]
    fn test_popup_render() {
        let state = create_test_state();
        let popup = SymbolSuggestionPopup::new(&state);

        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);

        popup.render(area, &mut buf);

        // Should contain the query
        let content: String = buf.content.iter().map(|c| c.symbol()).collect();
        assert!(content.contains("ModelInfo"));
    }

    #[test]
    fn test_empty_suggestions() {
        let mut state = SymbolSuggestionState::new("xyz".to_string(), 0);
        state.update_suggestions(vec![]);
        let popup = SymbolSuggestionPopup::new(&state);

        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);

        popup.render(area, &mut buf);

        let content: String = buf.content.iter().map(|c| c.symbol()).collect();
        assert!(content.contains("No matching"));
    }

    #[test]
    fn test_loading_state() {
        let state = SymbolSuggestionState::new("test".to_string(), 0);
        // state.loading is true by default
        let popup = SymbolSuggestionPopup::new(&state);

        let area = Rect::new(0, 0, 60, 10);
        let mut buf = Buffer::empty(area);

        popup.render(area, &mut buf);

        let content: String = buf.content.iter().map(|c| c.symbol()).collect();
        assert!(content.contains("Indexing"));
    }
}
