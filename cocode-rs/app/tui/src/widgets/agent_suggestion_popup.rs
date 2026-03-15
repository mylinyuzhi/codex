//! Agent suggestion popup widget.
//!
//! Displays a dropdown list of agent suggestions for @agent-* autocomplete.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::widgets::Widget;

use crate::state::AgentSuggestionItem;
use crate::state::AgentSuggestionState;
use crate::theme::Theme;
use crate::widgets::suggestion_popup::SuggestionPopup;
use crate::widgets::suggestion_popup::SuggestionRenderer;

/// Agent-specific suggestion renderer.
struct AgentRenderer;

impl SuggestionRenderer for AgentRenderer {
    type Item = AgentSuggestionItem;

    fn border_color(&self, theme: &Theme) -> Color {
        theme.warning
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
        item: &AgentSuggestionItem,
        is_selected: bool,
        x: u16,
        y: u16,
        width: u16,
        style: Style,
        theme: &Theme,
    ) {
        let agent_label = format!("agent-{}", item.agent_type);

        // Render agent type with highlighting
        if is_selected {
            buf.set_string(x, y, &agent_label, style);
        } else {
            let agent_prefix = "agent-";
            buf.set_string(x, y, agent_prefix, style);
            let mut cx = x + agent_prefix.len() as u16;
            for (char_idx, c) in item.agent_type.chars().enumerate() {
                let is_match = item.match_indices.contains(&char_idx);
                let char_style = if is_match {
                    style.bold().fg(theme.warning)
                } else {
                    style
                };
                buf.set_string(cx, y, c.to_string(), char_style);
                cx += 1;
            }
        }

        // Render description (truncated if needed)
        let name_width = agent_label.len().min(25);
        let desc_start = name_width + 4;
        let desc_x = x + desc_start as u16;
        let right_edge = x + width;
        if desc_x < right_edge.saturating_sub(3) {
            let available_width = (right_edge - desc_x - 1) as usize;
            let desc = if item.description.len() > available_width {
                format!(
                    " - {}...",
                    &item.description[..available_width.saturating_sub(4)]
                )
            } else {
                format!(" - {}", item.description)
            };
            buf.set_string(desc_x, y, desc, style.dim());
        }
    }

    fn hint_loading(&self) -> &str {
        "Loading..."
    }

    fn hint_empty(&self) -> &str {
        "No matching agents"
    }
}

/// Agent suggestion popup widget.
///
/// Renders a dropdown list of agent suggestions above the input area.
pub struct AgentSuggestionPopup<'a> {
    inner: SuggestionPopup<'a, AgentRenderer>,
}

impl<'a> AgentSuggestionPopup<'a> {
    /// Create a new agent suggestion popup.
    pub fn new(state: &'a AgentSuggestionState, theme: &'a Theme) -> Self {
        Self {
            inner: SuggestionPopup::new(state, theme, AgentRenderer),
        }
    }

    /// Calculate the area for the popup based on input position.
    pub fn calculate_area(&self, input_area: Rect, terminal_height: u16) -> Rect {
        self.inner.calculate_area(input_area, terminal_height)
    }
}

impl Widget for AgentSuggestionPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.inner.render(area, buf);
    }
}

#[cfg(test)]
#[path = "agent_suggestion_popup.test.rs"]
mod tests;
