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

use unicode_width::UnicodeWidthStr;

use crate::i18n::t;
use crate::state::ChatMessage;
use crate::state::MessageRole;
use crate::state::StreamingToolUse;
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
    /// Current spinner frame string (time-based).
    spinner_frame: &'a str,
    /// Duration of current or last thinking phase.
    thinking_duration: Option<Duration>,
    /// Theme for styling.
    theme: &'a Theme,
    /// Set of collapsed tool call IDs.
    collapsed_tools: &'a HashSet<String>,
    /// Available width for markdown rendering.
    width: u16,
    /// Whether user has manually scrolled (auto-scroll disabled).
    user_scrolled: bool,
    /// Streaming tool uses (partial tool JSON being built).
    streaming_tool_uses: &'a [StreamingToolUse],
    /// Whether to show system reminders (meta messages) in the chat.
    show_system_reminders: bool,
    /// Whether transcript mode is active (only show last N messages).
    transcript_mode: bool,
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
            spinner_frame: "⠋",
            thinking_duration: None,
            theme,
            collapsed_tools: &EMPTY_SET,
            width: 80,
            user_scrolled: false,
            streaming_tool_uses: &[],
            show_system_reminders: false,
            transcript_mode: false,
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

    /// Set the spinner frame string (from `Animation::current_frame()`).
    pub fn spinner_frame(mut self, frame: &'a str) -> Self {
        self.spinner_frame = frame;
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

    /// Set whether user has manually scrolled up.
    pub fn user_scrolled(mut self, scrolled: bool) -> Self {
        self.user_scrolled = scrolled;
        self
    }

    /// Set the streaming tool uses (partial tool calls being built).
    pub fn streaming_tool_uses(mut self, tool_uses: &'a [StreamingToolUse]) -> Self {
        self.streaming_tool_uses = tool_uses;
        self
    }

    /// Set whether system reminders (meta messages) should be visible.
    pub fn show_system_reminders(mut self, show: bool) -> Self {
        self.show_system_reminders = show;
        self
    }

    /// Set transcript mode (only show last 10 messages).
    pub fn transcript_mode(mut self, active: bool) -> Self {
        self.transcript_mode = active;
        self
    }

    /// Format duration for display (e.g., "2.3s").
    fn format_duration(duration: Duration) -> String {
        let secs = duration.as_secs_f64();
        if secs < 1.0 {
            format!("{:.0}ms", secs * 1000.0)
        } else if secs < 60.0 {
            format!("{secs:.1}s")
        } else {
            let mins = secs / 60.0;
            format!("{mins:.1}m")
        }
    }

    /// Get the spinner frame for the thinking indicator.
    fn spinner(&self) -> &str {
        self.spinner_frame
    }

    /// Render a single inline tool call line (collapsed style).
    fn render_tool_line(
        tool: &crate::state::InlineToolCall,
        theme: &Theme,
        indent: &str,
    ) -> Line<'static> {
        let status_icon = match tool.status {
            ToolStatus::Running => Span::raw("⏳").fg(theme.tool_running),
            ToolStatus::Completed => Span::raw("✓").fg(theme.tool_completed),
            ToolStatus::Failed => Span::raw("✗").fg(theme.tool_error),
        };

        let elapsed_span = tool
            .elapsed
            .map(|d| {
                let text = Self::format_duration(d);
                Span::raw(format!(" {text}")).fg(theme.text_dim)
            })
            .unwrap_or_default();

        let desc_preview = if tool.description.is_empty() {
            String::new()
        } else {
            let max = 40;
            if UnicodeWidthStr::width(tool.description.as_str()) > max {
                let boundary = tool.description.floor_char_boundary(max);
                format!(" {}…", &tool.description[..boundary])
            } else {
                format!(" {}", tool.description)
            }
        };

        Line::from(vec![
            Span::raw(indent.to_string()).fg(theme.border),
            status_icon,
            Span::raw(format!(" {}", tool.tool_name))
                .bold()
                .fg(theme.primary),
            Span::raw(desc_preview).fg(theme.text_dim),
            elapsed_span,
        ])
    }

    /// Render inline tool calls for a message, grouping parallel batches.
    fn render_tool_calls(&self, message: &ChatMessage) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let tools = &message.tool_calls;
        let mut i = 0;

        while i < tools.len() {
            // Check if this tool starts a parallel batch (batch_id is Some and
            // at least one more consecutive tool shares the same batch_id).
            if let Some(ref bid) = tools[i].batch_id {
                let batch_start = i;
                let mut batch_end = i + 1;
                while batch_end < tools.len() && tools[batch_end].batch_id.as_deref() == Some(bid) {
                    batch_end += 1;
                }
                let batch_size = batch_end - batch_start;

                if batch_size >= 2 {
                    // Render grouped parallel batch
                    lines.push(Line::from(vec![
                        Span::raw("  ").fg(self.theme.border),
                        Span::raw("‖").bold().fg(self.theme.secondary),
                        Span::raw(format!(
                            " {}",
                            t!("chat.parallel_tools", count = batch_size)
                        ))
                        .fg(self.theme.text_dim),
                    ]));
                    for tool in &tools[batch_start..batch_end] {
                        lines.push(Self::render_tool_line(tool, self.theme, "    "));
                    }
                    i = batch_end;
                    continue;
                }
            }

            // Single tool (no batch or batch of 1)
            lines.push(Self::render_tool_line(&tools[i], self.theme, "  "));
            i += 1;
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
        if let Some(ref thinking) = message.thinking
            && !thinking.is_empty()
        {
            let word_count = thinking.split_whitespace().count();
            let tokens = (word_count as f64 * crate::constants::THINKING_TOKEN_MULTIPLIER) as i32;

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
                let indicator = format!("  ▸ {}", t!("chat.thinking_collapsed", tokens = tokens));
                lines.push(Line::from(Span::raw(indicator).fg(self.theme.thinking)));
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

        // Apply subtle background tint to user messages for visual separation
        if message.role == MessageRole::User
            && let Some(bg) = self.theme.user_message_bg
        {
            for line in &mut lines {
                line.style.bg = Some(bg);
            }
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

        let show_reminders = self.show_system_reminders;
        let mut visible_messages: Vec<_> = self
            .messages
            .iter()
            .filter(|msg| !msg.is_meta || show_reminders)
            .collect();

        // Transcript mode: only show last N messages
        let limit = if self.transcript_mode {
            crate::constants::TRANSCRIPT_MODE_MESSAGE_LIMIT
        } else {
            crate::constants::MAX_MESSAGE_DISPLAY as usize
        };
        if visible_messages.len() > limit {
            visible_messages = visible_messages.split_off(visible_messages.len() - limit);
        }

        let has_streaming = self.streaming_content.is_some()
            || self
                .streaming_thinking
                .as_ref()
                .is_some_and(|t| !t.is_empty());

        // Show welcome hint when chat is empty and not streaming
        if visible_messages.is_empty() && !has_streaming {
            all_lines.push(Line::from(""));
            all_lines.push(Line::from(
                Span::raw(format!("  {}", t!("chat.welcome")))
                    .fg(self.theme.text_dim)
                    .italic(),
            ));
            all_lines.push(Line::from(""));
            all_lines.push(Line::from(vec![
                Span::raw(format!("  {} ", t!("chat.welcome_shortcuts"))).fg(self.theme.text_dim),
                Span::raw("? ").bold(),
                Span::raw("Help  ").fg(self.theme.text_dim),
                Span::raw("Ctrl+M ").bold(),
                Span::raw("Model  ").fg(self.theme.text_dim),
                Span::raw("Ctrl+P ").bold(),
                Span::raw("Commands  ").fg(self.theme.text_dim),
                Span::raw("Tab ").bold(),
                Span::raw("Plan mode").fg(self.theme.text_dim),
            ]));
            all_lines.push(Line::from(""));
        }

        for message in &visible_messages {
            if message.is_meta {
                // Render meta (system reminder) messages as collapsed dim lines
                let flat = message.content.replace('\n', " ");
                let (preview, suffix) = if UnicodeWidthStr::width(flat.as_str()) > 60 {
                    let boundary = flat.floor_char_boundary(60);
                    (&flat[..boundary], "...")
                } else {
                    (flat.as_str(), "")
                };
                all_lines.push(
                    Line::from(format!(
                        "  [{sr}] {preview}{suffix}",
                        sr = t!("chat.system_reminder")
                    ))
                    .fg(self.theme.text_dim)
                    .italic(),
                );
            } else {
                all_lines.extend(self.format_message(message));
            }
        }

        // Add streaming content if present
        if has_streaming {
            all_lines.push(Line::from(
                Span::raw(format!("◀ {}", t!("chat.assistant")))
                    .bold()
                    .fg(self.theme.assistant_message),
            ));

            // Show "waiting for response" when streaming started but no content yet
            let has_content = self.streaming_content.is_some_and(|c| !c.is_empty())
                || self.streaming_thinking.is_some_and(|t| !t.is_empty());
            if !has_content {
                let waiting_text = format!("  {} ", t!("chat.waiting_for_response"));
                let mut spans = crate::shimmer::shimmer_spans(&waiting_text);
                // Apply italic to all shimmer spans
                for span in &mut spans {
                    span.style = span.style.add_modifier(ratatui::style::Modifier::ITALIC);
                }
                all_lines.push(Line::from(spans));
            }

            // Show thinking content (collapsed indicator or expanded)
            if let Some(thinking) = self.streaming_thinking
                && !thinking.is_empty()
            {
                // Build duration string
                let duration_str = self
                    .thinking_duration
                    .map(Self::format_duration)
                    .unwrap_or_default();

                if self.show_thinking {
                    // Show expanded thinking content with animated header
                    let header = if self.is_thinking {
                        let spinner = self.spinner();
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
                    let tokens =
                        (word_count as f64 * crate::constants::THINKING_TOKEN_MULTIPLIER) as i32;
                    let indicator = if self.is_thinking {
                        let spinner = self.spinner();
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

            // Show main streaming content with markdown rendering
            if let Some(content) = self.streaming_content {
                if !content.is_empty() {
                    let md_lines = markdown_to_lines(content, self.theme, self.width);
                    all_lines.extend(md_lines);
                }
                all_lines.push(Line::from(Span::raw("  ▌").slow_blink()));
            }

            // Show streaming tool uses (partial tool JSON being built)
            for tool in self.streaming_tool_uses {
                let spinner = self.spinner();
                all_lines.push(Line::from(vec![
                    Span::raw(format!("  {spinner} ")).fg(self.theme.tool_running),
                    Span::raw(tool.name.clone()).bold().fg(self.theme.primary),
                    "...".dim(),
                ]));
                if !tool.accumulated_input.is_empty() {
                    let max_preview = 120;
                    let preview =
                        if UnicodeWidthStr::width(tool.accumulated_input.as_str()) > max_preview {
                            format!(
                                "    {}…",
                                &tool.accumulated_input[..tool
                                    .accumulated_input
                                    .char_indices()
                                    .take_while(|(i, _)| *i < max_preview)
                                    .last()
                                    .map_or(0, |(i, c)| i + c.len_utf8())]
                            )
                        } else {
                            format!("    {}", tool.accumulated_input)
                        };
                    all_lines.push(Line::from(Span::raw(preview).dim()));
                }
            }
        }

        // Count visible messages (meta messages visible only when toggled on)
        let visible_count = self
            .messages
            .iter()
            .filter(|m| !m.is_meta || show_reminders)
            .count();
        let bottom_title = if self.user_scrolled {
            format!(
                " {} | {} ",
                t!("chat.messages_count", count = visible_count),
                t!("chat.scroll_to_bottom")
            )
        } else {
            format!(" {} ", t!("chat.messages_count", count = visible_count))
        };
        let block = Block::default()
            .borders(Borders::NONE)
            .title_bottom(bottom_title.dim());

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
