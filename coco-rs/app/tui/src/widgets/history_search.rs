//! History search widget — search through conversation history.
//!
//! TS: src/hooks/useHistorySearch.ts (19KB)
//! Fuzzy/regex search with match highlighting and jump-to navigation.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

use crate::state::session::ChatMessage;
use crate::state::session::ChatRole;
use crate::theme::Theme;

/// A search match in the message history.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// Index into the messages array.
    pub message_index: i32,
    /// Preview of the matching content.
    pub preview: String,
    /// Role label for display.
    pub role_label: String,
}

/// History search state.
#[derive(Debug, Clone)]
pub struct HistorySearchState {
    /// Current search query.
    pub query: String,
    /// Matching results.
    pub matches: Vec<SearchMatch>,
    /// Currently selected match index.
    pub selected: i32,
}

impl HistorySearchState {
    /// Create a new search state.
    pub fn new() -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            selected: 0,
        }
    }

    /// Execute search against messages.
    pub fn search(&mut self, messages: &[ChatMessage]) {
        let query_lower = self.query.to_lowercase();
        self.matches.clear();
        self.selected = 0;

        if query_lower.is_empty() {
            return;
        }

        for (i, msg) in messages.iter().enumerate() {
            let text = msg.text_content();
            if text.to_lowercase().contains(&query_lower) {
                // Extract preview around match
                let preview = if text.len() > 80 {
                    if let Some(pos) = text.to_lowercase().find(&query_lower) {
                        let start = pos.saturating_sub(20);
                        let end = (pos + query_lower.len() + 40).min(text.len());
                        format!("...{}...", &text[start..end])
                    } else {
                        text[..80].to_string()
                    }
                } else {
                    text.to_string()
                };

                let role_label = match msg.role {
                    ChatRole::User => "you",
                    ChatRole::Assistant => "assistant",
                    ChatRole::System => "system",
                    ChatRole::Tool => "tool",
                }
                .to_string();

                self.matches.push(SearchMatch {
                    message_index: i as i32,
                    preview,
                    role_label,
                });
            }
        }
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if self.selected < self.matches.len() as i32 - 1 {
            self.selected += 1;
        }
    }

    /// Get the selected match's message index.
    pub fn selected_message_index(&self) -> Option<i32> {
        self.matches
            .get(self.selected as usize)
            .map(|m| m.message_index)
    }
}

impl Default for HistorySearchState {
    fn default() -> Self {
        Self::new()
    }
}

/// History search overlay widget.
pub struct HistorySearchWidget<'a> {
    state: &'a HistorySearchState,
    theme: &'a Theme,
}

impl<'a> HistorySearchWidget<'a> {
    pub fn new(state: &'a HistorySearchState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }
}

impl Widget for HistorySearchWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        // Search input
        lines.push(Line::from(vec![
            Span::raw("  🔍 ").fg(self.theme.accent),
            if self.state.query.is_empty() {
                Span::raw("Type to search...").fg(self.theme.text_dim)
            } else {
                Span::raw(&self.state.query).fg(self.theme.text)
            },
            Span::raw("▌").fg(self.theme.accent),
        ]));
        lines.push(Line::default());

        // Results
        if self.state.matches.is_empty() && !self.state.query.is_empty() {
            lines.push(Line::from(
                Span::raw("  No matches found").fg(self.theme.text_dim),
            ));
        }

        for (i, m) in self.state.matches.iter().enumerate().take(15) {
            let is_selected = i as i32 == self.state.selected;
            let marker = if is_selected { "▸ " } else { "  " };
            let role_color = match m.role_label.as_str() {
                "you" => self.theme.user_message,
                "assistant" => self.theme.assistant_message,
                _ => self.theme.text_dim,
            };

            lines.push(Line::from(vec![
                Span::raw(marker),
                Span::raw(format!("[{}] ", m.role_label)).fg(role_color),
                Span::raw(&m.preview).fg(self.theme.text),
            ]));
        }

        if self.state.matches.len() > 15 {
            lines.push(Line::from(
                Span::raw(format!(
                    "  ... {} more matches",
                    self.state.matches.len() - 15
                ))
                .fg(self.theme.text_dim),
            ));
        }

        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::raw("  [Enter] Jump  ").fg(self.theme.text_dim),
            Span::raw("[↑↓] Navigate  ").fg(self.theme.text_dim),
            Span::raw("[Esc] Close").fg(self.theme.text_dim),
        ]));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Search History ")
            .border_style(ratatui::style::Style::default().fg(self.theme.border_focused));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
