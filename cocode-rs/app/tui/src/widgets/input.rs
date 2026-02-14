//! Input widget.
//!
//! Multi-line input field with cursor support and syntax highlighting
//! for @mentions (cyan), /commands (magenta), and paste pills (green).

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

use crate::i18n::t;
use crate::paste::is_paste_pill;
use crate::state::InputState;
use crate::theme::Theme;

/// Token type for syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenType {
    /// Plain text.
    Text,
    /// @mention (file path).
    AtMention,
    /// @agent-* mention.
    AgentMention,
    /// @#symbol mention.
    SymbolMention,
    /// /command (skill).
    SlashCommand,
    /// Paste pill ([Pasted text #1], [Image #1]).
    PastePill,
}

/// A token in the input text.
#[derive(Debug, Clone)]
struct Token {
    /// Token text.
    text: String,
    /// Token type.
    token_type: TokenType,
}

impl Token {
    fn new(text: impl Into<String>, token_type: TokenType) -> Self {
        Self {
            text: text.into(),
            token_type,
        }
    }
}

/// Tokenize input text for syntax highlighting.
fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current_text = String::new();
    let chars = text.chars().peekable();
    let mut in_mention = false;
    let mut in_command = false;
    let mut in_pill = false;
    let mut pill_buffer = String::new();

    for c in chars {
        match c {
            '[' if !in_mention && !in_command && !in_pill => {
                // Potential start of a paste pill
                // Flush current text
                if !current_text.is_empty() {
                    tokens.push(Token::new(&current_text, TokenType::Text));
                    current_text.clear();
                }
                pill_buffer.push(c);
                in_pill = true;
            }
            ']' if in_pill => {
                // End of potential pill
                pill_buffer.push(c);
                if is_paste_pill(&pill_buffer) {
                    tokens.push(Token::new(&pill_buffer, TokenType::PastePill));
                } else {
                    // Not a valid pill, treat as regular text
                    tokens.push(Token::new(&pill_buffer, TokenType::Text));
                }
                pill_buffer.clear();
                in_pill = false;
            }
            _ if in_pill => {
                pill_buffer.push(c);
                // Safety limit: pills shouldn't be too long
                if pill_buffer.len() > 50 {
                    // Not a pill, flush as text
                    current_text.push_str(&pill_buffer);
                    pill_buffer.clear();
                    in_pill = false;
                }
            }
            '@' if !in_mention && !in_command => {
                // Check if this is a valid @mention start (at start or after whitespace)
                let is_valid_start =
                    current_text.is_empty() || current_text.ends_with(char::is_whitespace);

                if is_valid_start {
                    // Flush current text
                    if !current_text.is_empty() {
                        tokens.push(Token::new(&current_text, TokenType::Text));
                        current_text.clear();
                    }
                    current_text.push(c);
                    in_mention = true;
                } else {
                    current_text.push(c);
                }
            }
            '/' if !in_mention && !in_command => {
                // Check if this is a valid /command start (at start or after whitespace)
                let is_valid_start =
                    current_text.is_empty() || current_text.ends_with(char::is_whitespace);

                if is_valid_start {
                    // Flush current text
                    if !current_text.is_empty() {
                        tokens.push(Token::new(&current_text, TokenType::Text));
                        current_text.clear();
                    }
                    current_text.push(c);
                    in_command = true;
                } else {
                    current_text.push(c);
                }
            }
            ' ' | '\t' | '\n' => {
                // Whitespace ends mentions/commands
                if in_mention {
                    // Check if this is an @agent-* or @#symbol mention
                    let token_type = if current_text
                        .strip_prefix('@')
                        .is_some_and(|rest| rest.starts_with("agent-") || rest == "agent")
                    {
                        TokenType::AgentMention
                    } else if current_text
                        .strip_prefix('@')
                        .is_some_and(|rest| rest.starts_with('#'))
                    {
                        TokenType::SymbolMention
                    } else {
                        TokenType::AtMention
                    };
                    tokens.push(Token::new(&current_text, token_type));
                    current_text.clear();
                    in_mention = false;
                } else if in_command {
                    tokens.push(Token::new(&current_text, TokenType::SlashCommand));
                    current_text.clear();
                    in_command = false;
                }
                current_text.push(c);
            }
            _ => {
                current_text.push(c);
            }
        }
    }

    // Flush any remaining pill buffer as text
    if !pill_buffer.is_empty() {
        current_text.push_str(&pill_buffer);
    }

    // Flush remaining text
    if !current_text.is_empty() {
        let token_type = if in_mention {
            if current_text
                .strip_prefix('@')
                .is_some_and(|rest| rest.starts_with("agent-") || rest == "agent")
            {
                TokenType::AgentMention
            } else if current_text
                .strip_prefix('@')
                .is_some_and(|rest| rest.starts_with('#'))
            {
                TokenType::SymbolMention
            } else {
                TokenType::AtMention
            }
        } else if in_command {
            TokenType::SlashCommand
        } else {
            TokenType::Text
        };
        tokens.push(Token::new(&current_text, token_type));
    }

    tokens
}

/// Input widget for user text entry.
pub struct InputWidget<'a> {
    input: &'a InputState,
    theme: &'a Theme,
    focused: bool,
    plan_mode: bool,
    queued_count: i32,
    placeholder: Option<&'a str>,
}

impl<'a> InputWidget<'a> {
    /// Create a new input widget.
    pub fn new(input: &'a InputState, theme: &'a Theme) -> Self {
        Self {
            input,
            theme,
            focused: true,
            plan_mode: false,
            queued_count: 0,
            placeholder: None,
        }
    }

    /// Set whether the input is focused.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set whether plan mode is active.
    pub fn plan_mode(mut self, plan_mode: bool) -> Self {
        self.plan_mode = plan_mode;
        self
    }

    /// Set the number of queued commands.
    pub fn queued_count(mut self, count: i32) -> Self {
        self.queued_count = count;
        self
    }

    /// Set the placeholder text.
    pub fn placeholder(mut self, text: &'a str) -> Self {
        self.placeholder = Some(text);
        self
    }

    /// Get the display lines with cursor and syntax highlighting.
    fn get_lines(&self) -> Vec<Line<'static>> {
        let text = self.input.text();

        // Show placeholder if empty
        if text.is_empty() {
            if let Some(placeholder) = self.placeholder {
                return vec![Line::from(
                    Span::raw(placeholder.to_string()).dim().italic(),
                )];
            }
            // Just show cursor
            if self.focused {
                return vec![Line::from(Span::raw("▌").slow_blink())];
            }
            return vec![Line::from("")];
        }

        // Build highlighted spans with cursor
        let cursor_pos = self.input.cursor as usize;
        let tokens = tokenize(text);

        // Build a flat list of styled characters with cursor position
        let mut styled_chars: Vec<(char, TokenType)> = Vec::new();
        for token in &tokens {
            for c in token.text.chars() {
                styled_chars.push((c, token.token_type));
            }
        }

        // Now build lines, inserting cursor at the right position
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut current_line_spans: Vec<Span<'static>> = Vec::new();
        let mut current_span_text = String::new();
        let mut current_token_type: Option<TokenType> = None;
        let mut char_pos = 0_usize;

        for (c, token_type) in &styled_chars {
            // Insert cursor if this is the position
            if self.focused && char_pos == cursor_pos {
                // Flush current span
                if !current_span_text.is_empty() {
                    current_line_spans.push(Self::styled_span(
                        &current_span_text,
                        current_token_type.unwrap_or(TokenType::Text),
                        self.theme,
                    ));
                    current_span_text.clear();
                }
                current_line_spans.push(Span::raw("▌").slow_blink());
            }

            // Handle newlines
            if *c == '\n' {
                // Flush current span and line
                if !current_span_text.is_empty() {
                    current_line_spans.push(Self::styled_span(
                        &current_span_text,
                        current_token_type.unwrap_or(TokenType::Text),
                        self.theme,
                    ));
                    current_span_text.clear();
                }
                lines.push(Line::from(current_line_spans));
                current_line_spans = Vec::new();
                current_token_type = None;
            } else {
                // Continue building span
                if current_token_type != Some(*token_type) {
                    // Token type changed, flush current span
                    if !current_span_text.is_empty() {
                        current_line_spans.push(Self::styled_span(
                            &current_span_text,
                            current_token_type.unwrap_or(TokenType::Text),
                            self.theme,
                        ));
                        current_span_text.clear();
                    }
                    current_token_type = Some(*token_type);
                }
                current_span_text.push(*c);
            }

            char_pos += 1;
        }

        // Flush remaining span
        if !current_span_text.is_empty() {
            current_line_spans.push(Self::styled_span(
                &current_span_text,
                current_token_type.unwrap_or(TokenType::Text),
                self.theme,
            ));
        }

        // Insert cursor at end if needed
        if self.focused && char_pos <= cursor_pos {
            current_line_spans.push(Span::raw("▌").slow_blink());
        }

        // Flush remaining line
        if !current_line_spans.is_empty() {
            lines.push(Line::from(current_line_spans));
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::raw("▌").slow_blink()));
        }

        lines
    }

    /// Create a styled span based on token type.
    fn styled_span(text: &str, token_type: TokenType, theme: &Theme) -> Span<'static> {
        let raw = Span::raw(text.to_string());
        match token_type {
            TokenType::Text => raw,
            TokenType::AtMention => raw.fg(theme.primary),
            TokenType::AgentMention => raw.fg(theme.warning).bold(),
            TokenType::SymbolMention => raw.fg(theme.success).bold(),
            TokenType::SlashCommand => raw.fg(theme.accent),
            TokenType::PastePill => raw.fg(theme.success).italic(),
        }
    }
}

impl Widget for InputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 1 {
            return;
        }

        let lines = self.get_lines();

        // Create block
        let border_style = if self.focused {
            ratatui::style::Style::default().fg(self.theme.border_focused)
        } else {
            ratatui::style::Style::default().fg(self.theme.border)
        };

        let line_num = self.input.text().lines().count().max(1);
        let col = self.input.cursor + 1;
        let queue_tag = if self.queued_count > 0 {
            format!(" [Q:{}]", self.queued_count)
        } else {
            String::new()
        };
        let title_text = if self.plan_mode {
            format!(
                " {} [PLAN]{queue_tag} [{line_num}:{col}] ",
                t!("input.title")
            )
        } else {
            format!(" {}{queue_tag} [{line_num}:{col}] ", t!("input.title"))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title_text)
            .title_style(if self.focused {
                ratatui::style::Style::default().bold()
            } else {
                ratatui::style::Style::default().fg(self.theme.text_dim)
            });

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        paragraph.render(area, buf);
    }
}

#[cfg(test)]
#[path = "input.test.rs"]
mod tests;
