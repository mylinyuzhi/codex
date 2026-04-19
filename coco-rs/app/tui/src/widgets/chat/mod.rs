//! Chat history widget — renders all 30+ message types.
//!
//! TS: src/components/messages/ (41 files, 6K LOC) — each React component
//! is replaced by a match arm in one of the `render_*` submodules.

mod render_assistant;
mod render_system;
mod render_tool;
mod render_user;

use std::collections::HashSet;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

use crate::constants;
use crate::i18n::t;
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;
use crate::state::session::ToolExecution;
use crate::state::session::ToolStatus;
use crate::state::ui::StreamingState;
use crate::theme::Theme;

/// Chat history widget.
pub struct ChatWidget<'a> {
    messages: &'a [ChatMessage],
    scroll_offset: i32,
    streaming: Option<&'a StreamingState>,
    show_thinking: bool,
    show_system_reminders: bool,
    spinner_frame: &'a str,
    tool_executions: &'a [ToolExecution],
    collapsed_tools: Option<&'a HashSet<String>>,
    theme: &'a Theme,
    width: u16,
}

impl<'a> ChatWidget<'a> {
    pub fn new(messages: &'a [ChatMessage], theme: &'a Theme) -> Self {
        Self {
            messages,
            scroll_offset: 0,
            streaming: None,
            show_thinking: true,
            show_system_reminders: false,
            spinner_frame: "⠋",
            tool_executions: &[],
            collapsed_tools: None,
            theme,
            width: 80,
        }
    }

    pub fn scroll(mut self, offset: i32) -> Self {
        self.scroll_offset = offset;
        self
    }
    pub fn streaming(mut self, state: Option<&'a StreamingState>) -> Self {
        self.streaming = state;
        self
    }
    pub fn show_thinking(mut self, show: bool) -> Self {
        self.show_thinking = show;
        self
    }
    pub fn show_system_reminders(mut self, show: bool) -> Self {
        self.show_system_reminders = show;
        self
    }
    pub fn spinner_frame(mut self, frame: &'a str) -> Self {
        self.spinner_frame = frame;
        self
    }
    pub fn tool_executions(mut self, tools: &'a [ToolExecution]) -> Self {
        self.tool_executions = tools;
        self
    }
    pub fn collapsed_tools(mut self, collapsed: &'a HashSet<String>) -> Self {
        self.collapsed_tools = Some(collapsed);
        self
    }
    pub fn width(mut self, w: u16) -> Self {
        self.width = w;
        self
    }

    fn build_lines(&self) -> Vec<Line<'a>> {
        let mut lines: Vec<Line> = Vec::new();

        let mut i = 0;
        while i < self.messages.len() {
            let msg = &self.messages[i];
            if msg.is_meta && !self.show_system_reminders {
                // Collapsed: one-line `# [category] truncated preview` so
                // users can tell *something* was hidden without flooding the
                // transcript. TS: SystemTextMessage preview behaviour.
                self.render_meta_preview(msg, &mut lines);
                i += 1;
                continue;
            }

            // Parallel tool-call grouping: consecutive ToolUse messages are
            // rendered as a batch with a `‖ N in parallel` header and no
            // inter-tool blank lines. Matches TS's batch display. A single
            // ToolUse is not treated as a batch.
            let batch_end = self.tool_batch_end(i);
            if batch_end > i + 1 {
                let count = batch_end - i;
                lines.push(Line::from(
                    Span::raw(format!(
                        "  ‖ {}",
                        t!("chat.tools_in_parallel", count = count)
                    ))
                    .fg(self.theme.secondary)
                    .dim(),
                ));
                for j in i..batch_end {
                    self.render_message(&self.messages[j], &mut lines);
                }
                lines.push(Line::default());
                i = batch_end;
                continue;
            }

            self.render_message(msg, &mut lines);
            lines.push(Line::default());
            i += 1;
        }

        // Streaming content
        if let Some(streaming) = self.streaming {
            self.render_streaming(streaming, &mut lines);
        }

        // Spinner when busy but not streaming
        if self.streaming.is_none() && !self.tool_executions.is_empty() {
            let running = self
                .tool_executions
                .iter()
                .any(|t| matches!(t.status, ToolStatus::Queued | ToolStatus::Running));
            if running {
                lines.push(Line::from(vec![
                    Span::raw(format!("{} ", self.spinner_frame)).fg(self.theme.thinking),
                    Span::raw(t!("chat.processing").to_string()).fg(self.theme.thinking),
                ]));
            }
        }

        lines
    }

    /// Exclusive end index of the consecutive ToolUse run starting at `start`.
    /// Returns `start + 1` when the message at `start` is not a ToolUse.
    /// Meta messages inside the run are skipped (they're hidden collapsed
    /// previews); any non-ToolUse non-meta message terminates the run.
    fn tool_batch_end(&self, start: usize) -> usize {
        let is_tool_use = |m: &ChatMessage| matches!(m.content, MessageContent::ToolUse { .. });
        if !is_tool_use(&self.messages[start]) {
            return start + 1;
        }
        let mut end = start + 1;
        while end < self.messages.len() {
            let next = &self.messages[end];
            if is_tool_use(next) {
                end += 1;
            } else if next.is_meta {
                // Skip collapsed meta previews inside a batch.
                end += 1;
            } else {
                break;
            }
        }
        end
    }

    /// Render a single-line collapsed preview for a meta (system reminder)
    /// message. Keeps the user aware that system content exists without
    /// taking vertical space.
    fn render_meta_preview(&self, msg: &'a ChatMessage, lines: &mut Vec<Line<'a>>) {
        const PREVIEW_CHARS: usize = 50;
        let category = meta_category(&msg.content);
        let raw = msg.text_content();
        let single_line: String = raw.lines().next().unwrap_or("").to_string();
        let trimmed: String = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
        let preview = if trimmed.chars().count() > PREVIEW_CHARS {
            let mut s = trimmed.chars().take(PREVIEW_CHARS - 1).collect::<String>();
            s.push('\u{2026}');
            s
        } else {
            trimmed
        };
        lines.push(Line::from(vec![
            Span::raw(format!("  # [{category}] ")).fg(self.theme.system_message),
            Span::raw(preview).fg(self.theme.text_dim).italic(),
        ]));
    }

    fn render_message(&self, msg: &'a ChatMessage, lines: &mut Vec<Line<'a>>) {
        // Dispatch to the first category whose renderer handles the variant.
        // Each submodule returns None when the variant is outside its scope,
        // keeping the individual match statements exhaustive-by-category.
        render_user::try_render(self, &msg.content, lines)
            .or_else(|| render_assistant::try_render(self, &msg.content, lines))
            .or_else(|| render_tool::try_render(self, &msg.content, lines))
            .or_else(|| render_system::try_render(self, &msg.content, lines));
    }

    fn render_streaming(&self, streaming: &StreamingState, lines: &mut Vec<Line<'a>>) {
        let content = streaming.visible_content();
        if !content.is_empty() {
            let md_lines =
                crate::widgets::markdown::markdown_to_lines(content, self.theme, self.width);
            lines.extend(md_lines);
            lines.push(Line::from(Span::raw("▌").fg(self.theme.accent)));
        }

        if self.show_thinking && !streaming.thinking.is_empty() {
            let token_est = (streaming.thinking.split_whitespace().count() as f64
                * constants::THINKING_TOKEN_MULTIPLIER) as i64;
            lines.push(Line::from(
                Span::raw(format!(
                    "  💭 {}",
                    t!("chat.thinking_tokens", count = token_est)
                ))
                .fg(self.theme.thinking)
                .italic(),
            ));
        }
    }
}

impl Widget for ChatWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines = self.build_lines();
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset.max(0) as u16, 0));
        paragraph.render(area, buf);
    }
}

// ── Shared helpers ──

/// Parsed teammate message from XML tags.
///
/// TS: `parseTeammateMessages(text)` in UserTeammateMessage.tsx
pub(super) struct ParsedTeammateMessage {
    pub(super) teammate_id: String,
    pub(super) color: Option<String>,
    pub(super) summary: Option<String>,
    pub(super) content: String,
}

/// Parse XML-tagged teammate messages.
///
/// Format: `<teammate_message teammate_id="..." color="..." summary="...">content</teammate_message>`
pub(super) fn parse_teammate_xml(text: &str) -> Vec<ParsedTeammateMessage> {
    let mut results = Vec::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("<teammate_message ") {
        let after_start = &remaining[start..];
        let Some(tag_end) = after_start.find('>') else {
            break;
        };
        let tag = &after_start[..tag_end];

        let teammate_id = extract_attr(tag, "teammate_id").unwrap_or_default();
        let color = extract_attr(tag, "color");
        let summary = extract_attr(tag, "summary");

        let content_start = start + tag_end + 1;
        let close_tag = "</teammate_message>";
        let content_end = remaining[content_start..]
            .find(close_tag)
            .map(|pos| content_start + pos)
            .unwrap_or(remaining.len());

        let content = remaining[content_start..content_end].trim().to_string();

        results.push(ParsedTeammateMessage {
            teammate_id,
            color,
            summary,
            content,
        });

        remaining = if content_end + close_tag.len() <= remaining.len() {
            &remaining[content_end + close_tag.len()..]
        } else {
            ""
        };
    }

    results
}

/// Extract an attribute value from an XML-like tag.
fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let pattern = format!("{attr_name}=\"");
    let start = tag.find(&pattern)?;
    let value_start = start + pattern.len();
    let value_end = tag[value_start..].find('"')? + value_start;
    Some(tag[value_start..value_end].to_string())
}

/// Short category label for a collapsed meta preview. Mirrors the
/// bracketed prefix TS uses so users can identify what they hid (e.g.
/// `[api]`, `[hook]`, `[system]`).
fn meta_category(content: &MessageContent) -> &'static str {
    match content {
        MessageContent::SystemText(_) => "system",
        MessageContent::ApiError { .. } => "api",
        MessageContent::RateLimit { .. } => "rate-limit",
        MessageContent::Shutdown { .. }
        | MessageContent::ShutdownRequest { .. }
        | MessageContent::ShutdownRejected { .. } => "shutdown",
        MessageContent::HookSuccess { .. }
        | MessageContent::HookNonBlockingError { .. }
        | MessageContent::HookBlockingError { .. }
        | MessageContent::HookCancelled { .. }
        | MessageContent::HookSystemMessage { .. }
        | MessageContent::HookAdditionalContext { .. }
        | MessageContent::HookStoppedContinuation { .. }
        | MessageContent::HookAsyncResponse { .. } => "hook",
        MessageContent::PlanApproval { .. } => "plan",
        MessageContent::CompactBoundary => "compact",
        MessageContent::Advisor { .. } => "advisor",
        MessageContent::TaskAssignment { .. } => "task",
        MessageContent::ResourceUpdate { .. } => "mcp",
        _ => "meta",
    }
}

/// Format a resource URI for display.
///
/// TS `formatUri()`: file:// URIs show just the filename; other URIs
/// render truncated at 40 chars with a horizontal-ellipsis suffix.
pub(super) fn format_resource_target(uri: &str) -> String {
    if let Some(path) = uri.strip_prefix("file://") {
        return path
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or(path)
            .to_string();
    }
    if uri.chars().count() > 40 {
        let mut s = uri.chars().take(39).collect::<String>();
        s.push('\u{2026}');
        s
    } else {
        uri.to_string()
    }
}

/// Map agent color name to ratatui Color.
pub(super) fn teammate_color_to_ratatui(color_name: &str) -> ratatui::style::Color {
    match color_name {
        "red" => ratatui::style::Color::Red,
        "blue" => ratatui::style::Color::Blue,
        "green" => ratatui::style::Color::Green,
        "yellow" => ratatui::style::Color::Yellow,
        "purple" | "magenta" => ratatui::style::Color::Magenta,
        "orange" => ratatui::style::Color::LightRed,
        "pink" => ratatui::style::Color::LightMagenta,
        "cyan" => ratatui::style::Color::Cyan,
        _ => ratatui::style::Color::Reset,
    }
}
