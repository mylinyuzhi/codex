//! Chat history widget.
//!
//! Displays the conversation messages with support for:
//! - Message role indicators (user/assistant)
//! - Streaming content with markdown rendering
//! - Thinking content (collapsed by default)
//! - Animated thinking block with duration
//! - Inline tool call display
//! - Scroll position

use std::collections::HashSet;
use std::time::Duration;

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
use crate::state::ChatMessage;
use crate::state::MessageRole;
use crate::state::ToolStatus;
use crate::theme::Theme;
use crate::widgets::markdown::markdown_to_lines;

/// Chat history widget.
pub struct ChatWidget<'a> {
    messages: &'a [ChatMessage],
    scroll_offset: i32,
    streaming_content: Option<&'a str>,
    streaming_thinking: Option<&'a str>,
    show_thinking: bool,
    /// Whether currently thinking (for animation).
    is_thinking: bool,
    /// Current animation frame (0-7).
    animation_frame: u8,
    /// Duration of current or last thinking phase.
    thinking_duration: Option<Duration>,
    /// Theme for styling.
    theme: &'a Theme,
    /// Set of collapsed tool call IDs.
    collapsed_tools: &'a HashSet<String>,
    /// Available width for markdown rendering.
    width: u16,
}

impl<'a> ChatWidget<'a> {
    /// Create a new chat widget.
    pub fn new(messages: &'a [ChatMessage], theme: &'a Theme) -> Self {
        Self {
            messages,
            scroll_offset: 0,
            streaming_content: None,
            streaming_thinking: None,
            show_thinking: false,
            is_thinking: false,
            animation_frame: 0,
            thinking_duration: None,
            theme,
            collapsed_tools: &EMPTY_SET,
            width: 80,
        }
    }

    /// Set the scroll offset.
    pub fn scroll(mut self, offset: i32) -> Self {
        self.scroll_offset = offset;
        self
    }

    /// Set the streaming content.
    pub fn streaming(mut self, content: Option<&'a str>) -> Self {
        self.streaming_content = content;
        self
    }

    /// Set the streaming thinking content.
    pub fn streaming_thinking(mut self, thinking: Option<&'a str>) -> Self {
        self.streaming_thinking = thinking;
        self
    }

    /// Set whether to show thinking content.
    pub fn show_thinking(mut self, show: bool) -> Self {
        self.show_thinking = show;
        self
    }

    /// Set whether currently thinking (for animation).
    pub fn is_thinking(mut self, thinking: bool) -> Self {
        self.is_thinking = thinking;
        self
    }

    /// Set the animation frame.
    pub fn animation_frame(mut self, frame: u8) -> Self {
        self.animation_frame = frame;
        self
    }

    /// Set the thinking duration.
    pub fn thinking_duration(mut self, duration: Option<Duration>) -> Self {
        self.thinking_duration = duration;
        self
    }

    /// Set the collapsed tools set.
    pub fn collapsed_tools(mut self, collapsed: &'a HashSet<String>) -> Self {
        self.collapsed_tools = collapsed;
        self
    }

    /// Set the available width.
    pub fn width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    /// Format duration for display (e.g., "2.3s").
    fn format_duration(duration: Duration) -> String {
        let secs = duration.as_secs_f64();
        if secs < 1.0 {
            format!("{:.0}ms", secs * 1000.0)
        } else if secs < 60.0 {
            format!("{:.1}s", secs)
        } else {
            let mins = secs / 60.0;
            format!("{:.1}m", mins)
        }
    }

    /// Get the animation character for the thinking indicator.
    fn thinking_animation_char(&self) -> char {
        const SPINNER: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];
        SPINNER[self.animation_frame as usize % SPINNER.len()]
    }

    /// Render inline tool calls for a message.
    fn render_tool_calls(&self, message: &ChatMessage) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for tool in &message.tool_calls {
            let status_icon = match tool.status {
                ToolStatus::Running => Span::raw(" ⏳").fg(self.theme.tool_running),
                ToolStatus::Completed => Span::raw(" ✓").fg(self.theme.tool_completed),
                ToolStatus::Failed => Span::raw(" ✗").fg(self.theme.tool_error),
            };

            // Top border with tool name and status
            lines.push(Line::from(vec![
                Span::raw("  ┌ ").fg(self.theme.border),
                Span::raw(tool.tool_name.clone())
                    .bold()
                    .fg(self.theme.primary),
                Span::raw(" ─").fg(self.theme.border),
                status_icon,
            ]));

            // Description line
            if !tool.description.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("  │ ").fg(self.theme.border),
                    Span::raw(tool.description.clone()).fg(self.theme.text_dim),
                ]));
            }

            // Bottom border
            lines.push(Line::from(
                Span::raw("  └─────────────────────────────").fg(self.theme.border),
            ));
        }
        lines
    }

    /// Format a message for display.
    fn format_message(&self, message: &ChatMessage) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // Role indicator
        let role_span = match message.role {
            MessageRole::User => Span::raw(format!("▶ {}", t!("chat.you")))
                .bold()
                .fg(self.theme.user_message),
            MessageRole::Assistant => Span::raw(format!("◀ {}", t!("chat.assistant")))
                .bold()
                .fg(self.theme.assistant_message),
            MessageRole::System => Span::raw(format!("⚙ {}", t!("chat.system")))
                .bold()
                .fg(self.theme.system_message),
        };
        lines.push(Line::from(role_span));

        // Thinking content (if any)
        if let Some(ref thinking) = message.thinking {
            if !thinking.is_empty() {
                let word_count = thinking.split_whitespace().count();
                let tokens = (word_count as f64 * 1.3) as i32;

                if self.show_thinking {
                    // Show expanded thinking content with styled header
                    let header = format!("  💭 {}", t!("chat.thinking_tokens", tokens = tokens));
                    lines.push(Line::from(
                        Span::raw(header).italic().fg(self.theme.thinking),
                    ));
                    for line in thinking.lines() {
                        lines.push(Line::from(
                            Span::raw(format!("    {line}")).fg(self.theme.text_dim),
                        ));
                    }
                    lines.push(Line::from("")); // Separator
                } else {
                    // Show collapsed indicator
                    let indicator =
                        format!("  ▸ {}", t!("chat.thinking_collapsed", tokens = tokens));
                    lines.push(Line::from(Span::raw(indicator).fg(self.theme.thinking)));
                }
            }
        }

        // Inline tool calls (between thinking and content)
        if !message.tool_calls.is_empty() {
            lines.extend(self.render_tool_calls(message));
        }

        // Message content - use markdown rendering for assistant messages
        if message.role == MessageRole::Assistant && !message.content.is_empty() {
            let md_lines = markdown_to_lines(&message.content, self.theme, self.width);
            lines.extend(md_lines);
        } else {
            for line in message.content.lines() {
                lines.push(Line::from(Span::raw(format!("  {line}"))));
            }
        }

        // Streaming indicator
        if message.streaming {
            lines.push(Line::from(Span::raw("  ▌").slow_blink()));
        }

        // Empty line after message
        lines.push(Line::from(""));

        lines
    }
}

/// Empty set for default collapsed_tools reference.
static EMPTY_SET: std::sync::LazyLock<HashSet<String>> = std::sync::LazyLock::new(HashSet::new);

impl Widget for ChatWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 {
            return;
        }

        // Build all lines
        let mut all_lines: Vec<Line> = Vec::new();

        for message in self.messages {
            all_lines.extend(self.format_message(message));
        }

        // Add streaming content if present
        let has_streaming = self.streaming_content.is_some()
            || self
                .streaming_thinking
                .as_ref()
                .is_some_and(|t| !t.is_empty());

        if has_streaming {
            all_lines.push(Line::from(
                Span::raw(format!("◀ {}", t!("chat.assistant")))
                    .bold()
                    .fg(self.theme.assistant_message),
            ));

            // Show thinking content (collapsed indicator or expanded)
            if let Some(thinking) = self.streaming_thinking {
                if !thinking.is_empty() {
                    // Build duration string
                    let duration_str = self
                        .thinking_duration
                        .map(Self::format_duration)
                        .unwrap_or_default();

                    if self.show_thinking {
                        // Show expanded thinking content with animated header
                        let header = if self.is_thinking {
                            let spinner = self.thinking_animation_char();
                            format!(
                                "  {spinner} {}",
                                t!("chat.thinking_active", duration = duration_str)
                            )
                        } else {
                            format!(
                                "  💭 {}",
                                t!("chat.thinking_active", duration = duration_str)
                            )
                        };
                        all_lines.push(Line::from(
                            Span::raw(header).italic().fg(self.theme.thinking),
                        ));

                        for line in thinking.lines() {
                            all_lines.push(Line::from(
                                Span::raw(format!("    {line}")).fg(self.theme.text_dim),
                            ));
                        }
                        if self.is_thinking {
                            all_lines.push(Line::from(
                                Span::raw("    ▌").fg(self.theme.text_dim).slow_blink(),
                            ));
                        }
                    } else {
                        // Show collapsed indicator with word count estimate and animation
                        let word_count = thinking.split_whitespace().count();
                        let tokens = (word_count as f64 * 1.3) as i32;
                        let indicator = if self.is_thinking {
                            let spinner = self.thinking_animation_char();
                            format!(
                                "  {spinner} {}",
                                t!(
                                    "chat.thinking_active_collapsed",
                                    tokens = tokens,
                                    duration = duration_str
                                )
                            )
                        } else {
                            format!(
                                "  ▸ {}",
                                t!(
                                    "chat.thinking_active_collapsed",
                                    tokens = tokens,
                                    duration = duration_str
                                )
                            )
                        };
                        all_lines.push(Line::from(Span::raw(indicator).fg(self.theme.thinking)));
                    }
                }
            }

            // Show main streaming content with markdown rendering
            if let Some(content) = self.streaming_content {
                if !content.is_empty() {
                    let md_lines = markdown_to_lines(content, self.theme, self.width);
                    all_lines.extend(md_lines);
                }
                all_lines.push(Line::from(Span::raw("  ▌").slow_blink()));
            }
        }

        // Create the block
        let block = Block::default().borders(Borders::NONE).title_bottom(
            format!(
                " {} ",
                t!("chat.messages_count", count = self.messages.len())
            )
            .dim(),
        );

        // Calculate scroll
        let total_lines = all_lines.len();
        let visible_lines = (area.height - 2) as usize; // Account for borders
        let max_scroll = total_lines.saturating_sub(visible_lines);
        let scroll = (self.scroll_offset as usize).min(max_scroll);

        // Create paragraph with scroll
        let paragraph = Paragraph::new(all_lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll as u16, 0));

        paragraph.render(area, buf);
    }
}

#[cfg(test)]
#[path = "chat.test.rs"]
mod tests;
