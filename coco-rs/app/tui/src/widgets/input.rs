//! Input widget with syntax highlighting.
//!
//! Highlights @mentions (cyan), /commands (magenta), @agent-* (red),
//! @#symbol (green), and paste pills (green/italic).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

use crate::i18n::t;
use crate::state::ui::InputState;
use crate::theme::Theme;

/// Token type for syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenType {
    Text,
    AtMention,
    AgentMention,
    SymbolMention,
    SlashCommand,
    PastePill,
}

/// Input widget with syntax highlighting and cursor.
pub struct InputWidget<'a> {
    input: &'a InputState,
    theme: &'a Theme,
    focused: bool,
    plan_mode: bool,
    is_streaming: bool,
}

impl<'a> InputWidget<'a> {
    pub fn new(input: &'a InputState, theme: &'a Theme) -> Self {
        Self {
            input,
            theme,
            focused: true,
            plan_mode: false,
            is_streaming: false,
        }
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn plan_mode(mut self, plan_mode: bool) -> Self {
        self.plan_mode = plan_mode;
        self
    }

    pub fn is_streaming(mut self, streaming: bool) -> Self {
        self.is_streaming = streaming;
        self
    }

    /// Tokenize input text for syntax highlighting.
    fn tokenize(text: &str) -> Vec<(String, TokenType)> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        let mut chars = text.chars().peekable();

        while let Some(c) = chars.next() {
            match c {
                '@' if current.is_empty() || current.ends_with(char::is_whitespace) => {
                    if !current.is_empty() {
                        tokens.push((current.clone(), TokenType::Text));
                        current.clear();
                    }
                    let mut mention = String::from('@');
                    // Handle @"quoted path" syntax
                    if chars.peek() == Some(&'"') {
                        mention.push('"');
                        chars.next(); // consume opening "
                        while let Some(&next) = chars.peek() {
                            mention.push(next);
                            chars.next();
                            if next == '"' {
                                break;
                            }
                        }
                    } else {
                        while let Some(&next) = chars.peek() {
                            if next.is_whitespace() {
                                break;
                            }
                            mention.push(next);
                            chars.next();
                        }
                    }
                    let token_type =
                        if mention.starts_with("@agent-") || mention.contains("(agent)") {
                            TokenType::AgentMention
                        } else if mention.starts_with("@#") {
                            TokenType::SymbolMention
                        } else {
                            TokenType::AtMention
                        };
                    tokens.push((mention, token_type));
                }
                '/' if current.is_empty() => {
                    let mut cmd = String::from('/');
                    while let Some(&next) = chars.peek() {
                        if next.is_whitespace() {
                            break;
                        }
                        cmd.push(next);
                        chars.next();
                    }
                    tokens.push((cmd, TokenType::SlashCommand));
                }
                '[' => {
                    if !current.is_empty() {
                        tokens.push((current.clone(), TokenType::Text));
                        current.clear();
                    }
                    let mut pill = String::from('[');
                    let mut found_close = false;
                    let mut count = 0;
                    while let Some(&next) = chars.peek() {
                        pill.push(next);
                        chars.next();
                        count += 1;
                        if next == ']' {
                            found_close = true;
                            break;
                        }
                        if count > 50 {
                            break;
                        }
                    }
                    let token_type = if found_close
                        && (pill.starts_with("[Pasted") || pill.starts_with("[Image"))
                    {
                        TokenType::PastePill
                    } else {
                        TokenType::Text
                    };
                    tokens.push((pill, token_type));
                }
                _ => {
                    current.push(c);
                }
            }
        }

        if !current.is_empty() {
            tokens.push((current, TokenType::Text));
        }

        tokens
    }
}

impl Widget for InputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_color = if self.focused {
            self.theme.border_focused
        } else {
            self.theme.border
        };

        let title_text = if self.plan_mode {
            t!("input.title_plan_mode")
        } else if self.is_streaming {
            t!("input.title_queue")
        } else {
            t!("input.title")
        };
        let title = format!(" {title_text} ");

        let block = Block::default()
            .borders(Borders::TOP)
            .title(title)
            .border_style(Style::default().fg(border_color));

        if self.input.is_empty() {
            let placeholder = Paragraph::new(
                Span::raw(t!("input.placeholder").to_string()).fg(self.theme.text_dim),
            )
            .block(block);
            placeholder.render(area, buf);
            return;
        }

        // Tokenize and style
        let tokens = Self::tokenize(&self.input.text);
        let spans: Vec<Span> = tokens
            .into_iter()
            .map(|(text, token_type)| match token_type {
                TokenType::Text => Span::raw(text),
                TokenType::AtMention => Span::raw(text).fg(self.theme.primary),
                TokenType::AgentMention => Span::raw(text).fg(self.theme.error).bold(),
                TokenType::SymbolMention => Span::raw(text).fg(self.theme.success).bold(),
                TokenType::SlashCommand => Span::raw(text).fg(self.theme.accent),
                TokenType::PastePill => Span::raw(text).fg(self.theme.success).italic(),
            })
            .collect();

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line).block(block).wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
