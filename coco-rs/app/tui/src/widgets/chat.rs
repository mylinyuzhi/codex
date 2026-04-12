//! Chat history widget — renders all 30+ message types.
//!
//! TS: src/components/messages/ (41 files, 6K LOC)
//! Each message variant has dedicated rendering logic.

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
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;
use crate::state::session::PlanAction;
use crate::state::session::ToolExecution;
use crate::state::session::ToolStatus;
use crate::state::session::ToolUseStatus;
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

        for msg in self.messages {
            if msg.is_meta && !self.show_system_reminders {
                continue;
            }
            self.render_message(msg, &mut lines);
            lines.push(Line::default());
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
                .any(|t| t.status == ToolStatus::Running);
            if running {
                lines.push(Line::from(vec![
                    Span::raw(format!("{} ", self.spinner_frame)).fg(self.theme.thinking),
                    Span::raw("Processing...").fg(self.theme.thinking),
                ]));
            }
        }

        lines
    }

    fn render_message(&self, msg: &'a ChatMessage, lines: &mut Vec<Line<'a>>) {
        match &msg.content {
            // ── User messages ──
            MessageContent::Text(text) => {
                for line in text.lines() {
                    lines.push(Line::from(
                        Span::raw(format!("> {line}")).fg(self.theme.user_message),
                    ));
                }
            }
            MessageContent::Image { path } => {
                lines.push(Line::from(vec![
                    Span::raw("> ").fg(self.theme.user_message),
                    Span::raw("📎 ").fg(self.theme.accent),
                    Span::raw(path.as_str()).fg(self.theme.primary).underlined(),
                ]));
            }
            MessageContent::BashInput { command } => {
                lines.push(Line::from(vec![
                    Span::raw("> $ ").fg(self.theme.user_message),
                    Span::raw(command.as_str()).fg(self.theme.accent),
                ]));
            }
            MessageContent::BashOutput { output, exit_code } => {
                let color = if *exit_code == 0 {
                    self.theme.text_dim
                } else {
                    self.theme.error
                };
                for line in output.lines().take(20) {
                    lines.push(Line::from(Span::raw(format!("  {line}")).fg(color)));
                }
                if output.lines().count() > 20 {
                    lines.push(Line::from(
                        Span::raw("  ... (truncated)")
                            .fg(self.theme.text_dim)
                            .italic(),
                    ));
                }
                if *exit_code != 0 {
                    lines.push(Line::from(
                        Span::raw(format!("  exit code: {exit_code}")).fg(self.theme.error),
                    ));
                }
            }
            MessageContent::PlanMarker { action } => {
                let text = match action {
                    PlanAction::Enter => "── Entered plan mode ──",
                    PlanAction::Exit => "── Exited plan mode ──",
                };
                lines.push(Line::from(
                    Span::raw(format!("  {text}"))
                        .fg(self.theme.plan_mode)
                        .italic(),
                ));
            }
            MessageContent::MemoryInput { content } => {
                lines.push(Line::from(vec![
                    Span::raw("> ").fg(self.theme.user_message),
                    Span::raw("💾 ").fg(self.theme.accent),
                    Span::raw(content.as_str()).fg(self.theme.text_dim),
                ]));
            }
            MessageContent::AgentNotification { agent_id, summary } => {
                lines.push(Line::from(vec![
                    Span::raw("  🤖 ").fg(self.theme.accent),
                    Span::raw(format!("[{agent_id}] ")).fg(self.theme.text_dim),
                    Span::raw(summary.as_str()).fg(self.theme.text),
                ]));
            }
            MessageContent::TeammateMessage { teammate, content } => {
                // Parse XML-tagged teammate messages if present
                let parsed = parse_teammate_xml(content);
                if parsed.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw(format!("  @{teammate}: ")).fg(self.theme.primary),
                        Span::raw(content.clone()).fg(self.theme.text),
                    ]));
                } else {
                    for part in parsed {
                        let name_color = part
                            .color
                            .as_deref()
                            .map(teammate_color_to_ratatui)
                            .unwrap_or(self.theme.primary);
                        let mut header =
                            vec![Span::raw(format!("  @{}: ", part.teammate_id)).fg(name_color)];
                        if let Some(summary) = part.summary {
                            header.push(Span::raw(format!("({summary}) ")).dim());
                        }
                        lines.push(Line::from(header));
                        for line in part.content.lines() {
                            lines.push(Line::from(vec![
                                Span::raw("    ".to_string()),
                                Span::raw(line.to_string()).fg(self.theme.text),
                            ]));
                        }
                    }
                }
            }
            MessageContent::Attachment {
                attachment_type,
                preview,
            } => {
                lines.push(Line::from(vec![
                    Span::raw("> ").fg(self.theme.user_message),
                    Span::raw(format!("📎 [{attachment_type}] ")).fg(self.theme.accent),
                    Span::raw(preview.as_str()).fg(self.theme.text_dim),
                ]));
            }

            // ── Assistant messages ──
            MessageContent::AssistantText(text) => {
                let md_lines =
                    crate::widgets::markdown::markdown_to_lines(text, self.theme, self.width);
                lines.extend(md_lines);
            }
            MessageContent::Thinking {
                content,
                duration_ms,
            } => {
                if self.show_thinking {
                    let token_est = (content.split_whitespace().count() as f64
                        * constants::THINKING_TOKEN_MULTIPLIER)
                        as i64;
                    let dur = duration_ms
                        .map(|ms| format!(" ({ms}ms)"))
                        .unwrap_or_default();
                    lines.push(Line::from(
                        Span::raw(format!("  💭 ~{token_est} tokens{dur}"))
                            .fg(self.theme.thinking)
                            .italic(),
                    ));
                    for line in content.lines().take(5) {
                        lines.push(Line::from(
                            Span::raw(format!("  │ {line}"))
                                .fg(self.theme.thinking)
                                .dim(),
                        ));
                    }
                    if content.lines().count() > 5 {
                        lines.push(Line::from(
                            Span::raw("  │ ...").fg(self.theme.thinking).dim(),
                        ));
                    }
                } else {
                    let token_est = (content.split_whitespace().count() as f64
                        * constants::THINKING_TOKEN_MULTIPLIER)
                        as i64;
                    lines.push(Line::from(
                        Span::raw(format!("  ▸ 💭 {token_est} tokens"))
                            .fg(self.theme.thinking)
                            .dim(),
                    ));
                }
            }
            MessageContent::RedactedThinking => {
                lines.push(Line::from(
                    Span::raw("  ▸ 💭 [redacted thinking]")
                        .fg(self.theme.thinking)
                        .dim(),
                ));
            }
            MessageContent::ToolUse {
                tool_name,
                call_id: _,
                input_preview,
                status,
            } => {
                let (icon, color) = match status {
                    ToolUseStatus::Queued => ("◌", self.theme.text_dim),
                    ToolUseStatus::Running => ("⏳", self.theme.tool_running),
                    ToolUseStatus::Completed => ("✓", self.theme.tool_completed),
                    ToolUseStatus::Failed => ("✗", self.theme.tool_error),
                };
                let preview =
                    if input_preview.len() > constants::TOOL_DESCRIPTION_MAX_CHARS as usize {
                        format!(
                            "{}...",
                            &input_preview[..constants::TOOL_DESCRIPTION_MAX_CHARS as usize - 3]
                        )
                    } else {
                        input_preview.clone()
                    };
                lines.push(Line::from(vec![
                    Span::raw(format!("  {icon} ")).fg(color),
                    Span::raw(format!("{tool_name}: "))
                        .fg(self.theme.text_dim)
                        .bold(),
                    Span::raw(preview).fg(self.theme.text),
                ]));
            }

            // ── Tool results ──
            MessageContent::ToolSuccess { tool_name, output } => {
                lines.push(Line::from(vec![
                    Span::raw("  ✓ ").fg(self.theme.tool_completed),
                    Span::raw(format!("{tool_name}: ")).fg(self.theme.text_dim),
                ]));
                for line in output.lines().take(15) {
                    lines.push(Line::from(
                        Span::raw(format!("    {line}")).fg(self.theme.text),
                    ));
                }
                if output.lines().count() > 15 {
                    lines.push(Line::from(
                        Span::raw(format!(
                            "    ... ({} more lines)",
                            output.lines().count() - 15
                        ))
                        .fg(self.theme.text_dim),
                    ));
                }
            }
            MessageContent::ToolError { tool_name, error } => {
                lines.push(Line::from(vec![
                    Span::raw("  ✗ ").fg(self.theme.tool_error),
                    Span::raw(format!("{tool_name}: ")).fg(self.theme.text_dim),
                    Span::raw(error.as_str()).fg(self.theme.error),
                ]));
            }
            MessageContent::ToolRejected { tool_name, reason } => {
                lines.push(Line::from(vec![
                    Span::raw("  ⊘ ").fg(self.theme.warning),
                    Span::raw(format!("{tool_name} rejected: ")).fg(self.theme.text_dim),
                    Span::raw(reason.as_str()).fg(self.theme.warning),
                ]));
            }
            MessageContent::ToolCanceled { tool_name } => {
                lines.push(Line::from(vec![
                    Span::raw("  ⊘ ").fg(self.theme.text_dim),
                    Span::raw(format!("{tool_name} canceled"))
                        .fg(self.theme.text_dim)
                        .italic(),
                ]));
            }
            MessageContent::FileEditDiff { path, diff, .. } => {
                lines.push(Line::from(vec![
                    Span::raw("  📝 ").fg(self.theme.accent),
                    Span::raw(path.as_str()).fg(self.theme.primary).underlined(),
                ]));
                let diff_lines =
                    crate::widgets::diff_display::render_diff_lines(diff, self.theme, self.width);
                lines.extend(diff_lines);
            }
            MessageContent::FileWriteResult {
                path,
                bytes_written,
            } => {
                lines.push(Line::from(vec![
                    Span::raw("  ✓ ").fg(self.theme.tool_completed),
                    Span::raw("wrote ").fg(self.theme.text_dim),
                    Span::raw(path.as_str()).fg(self.theme.primary),
                    Span::raw(format!(" ({bytes_written} bytes)")).fg(self.theme.text_dim),
                ]));
            }

            // ── System messages ──
            MessageContent::SystemText(text) => {
                for line in text.lines() {
                    lines.push(Line::from(
                        Span::raw(format!("  # {line}")).fg(self.theme.system_message),
                    ));
                }
            }
            MessageContent::ApiError {
                error,
                retryable,
                status_code,
            } => {
                let status = status_code.map(|c| format!(" [{c}]")).unwrap_or_default();
                let retry = if *retryable { " (retrying...)" } else { "" };
                lines.push(Line::from(
                    Span::raw(format!("  ⚠ API error{status}: {error}{retry}"))
                        .fg(self.theme.error),
                ));
            }
            MessageContent::RateLimit { message, resets_at } => {
                let reset = resets_at
                    .map(|t| format!(" (resets at {t})"))
                    .unwrap_or_default();
                lines.push(Line::from(
                    Span::raw(format!("  ⏱ {message}{reset}")).fg(self.theme.warning),
                ));
            }
            MessageContent::Shutdown { reason } => {
                lines.push(Line::from(
                    Span::raw(format!("  ■ Session ended: {reason}"))
                        .fg(self.theme.text_dim)
                        .italic(),
                ));
            }
            MessageContent::ShutdownRequest { from, reason } => {
                let reason_text = reason
                    .as_deref()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default();
                lines.push(Line::from(
                    Span::raw(format!("  ⛔ Shutdown requested by {from}{reason_text}"))
                        .fg(self.theme.error),
                ));
            }
            MessageContent::ShutdownRejected { from, reason } => {
                lines.push(Line::from(
                    Span::raw(format!("  ✗ Shutdown rejected by {from}: {reason}"))
                        .fg(self.theme.text_dim),
                ));
            }
            MessageContent::HookSuccess { hook_name, output } => {
                lines.push(Line::from(vec![
                    Span::raw("  ⚙ ").fg(self.theme.accent),
                    Span::raw(format!("{hook_name}: ")).dim(),
                    Span::raw(output.clone()).green(),
                ]));
            }
            MessageContent::HookNonBlockingError { hook_name, error } => {
                lines.push(Line::from(vec![
                    Span::raw("  ⚠ ").fg(self.theme.warning),
                    Span::raw(format!("{hook_name}: ")).fg(self.theme.text_dim),
                    Span::raw(error.clone()).yellow(),
                ]));
            }
            MessageContent::HookBlockingError {
                hook_name,
                error,
                command,
            } => {
                lines.push(Line::from(vec![
                    Span::raw("  ✗ ").fg(self.theme.error),
                    Span::raw(format!("{hook_name}: ")).fg(self.theme.text_dim),
                    Span::raw(error.clone()).red(),
                ]));
                lines.push(Line::from(
                    Span::raw(format!("    command: {command}")).dim(),
                ));
            }
            MessageContent::HookCancelled { hook_name } => {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {hook_name}: ")).dim(),
                    Span::raw("cancelled").dim(),
                ]));
            }
            MessageContent::HookSystemMessage { hook_name, message } => {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {hook_name}: ")).fg(self.theme.text_dim),
                    Span::raw(message.clone()).cyan(),
                ]));
            }
            MessageContent::HookAdditionalContext { hook_name, context } => {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {hook_name}: ")).dim(),
                    Span::raw(context.clone()).fg(self.theme.text),
                ]));
            }
            MessageContent::HookStoppedContinuation { hook_name, reason } => {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {hook_name}: ")).fg(self.theme.text_dim),
                    Span::raw(reason.clone()).yellow(),
                ]));
            }
            MessageContent::HookAsyncResponse { hook_name, output } => {
                lines.push(Line::from(vec![
                    Span::raw("  ⚙ ").fg(self.theme.accent),
                    Span::raw(format!("{hook_name}: ")).dim(),
                    Span::raw(output.clone()).fg(self.theme.text),
                ]));
            }
            MessageContent::PlanApproval { plan, .. } => {
                lines.push(Line::from(
                    Span::raw("  📋 Plan for review:")
                        .fg(self.theme.plan_mode)
                        .bold(),
                ));
                for line in plan.lines().take(20) {
                    lines.push(Line::from(
                        Span::raw(format!("  │ {line}")).fg(self.theme.text),
                    ));
                }
            }
            MessageContent::CompactBoundary => {
                let border = "─".repeat(40);
                lines.push(Line::from(
                    Span::raw(format!("  {border}")).fg(self.theme.border).dim(),
                ));
            }
            MessageContent::Advisor {
                advisor_id,
                content,
            } => {
                lines.push(Line::from(vec![
                    Span::raw("  📋 ").fg(self.theme.accent),
                    Span::raw(format!("[advisor:{advisor_id}] "))
                        .fg(self.theme.text_dim)
                        .bold(),
                ]));
                let md_lines =
                    crate::widgets::markdown::markdown_to_lines(content, self.theme, self.width);
                lines.extend(md_lines);
            }
            MessageContent::TaskAssignment {
                task_id,
                assignee,
                description,
            } => {
                lines.push(Line::from(vec![
                    Span::raw("  📌 ").fg(self.theme.accent),
                    Span::raw(format!("Task {task_id} → @{assignee}: "))
                        .fg(self.theme.primary)
                        .bold(),
                    Span::raw(description.clone()).fg(self.theme.text),
                ]));
            }
        }
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
                Span::raw(format!("  💭 ~{token_est} thinking tokens"))
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

// ── Teammate message XML parsing ──

/// Parsed teammate message from XML tags.
///
/// TS: `parseTeammateMessages(text)` in UserTeammateMessage.tsx
struct ParsedTeammateMessage {
    teammate_id: String,
    color: Option<String>,
    summary: Option<String>,
    content: String,
}

/// Parse XML-tagged teammate messages.
///
/// Format: `<teammate_message teammate_id="..." color="..." summary="...">content</teammate_message>`
fn parse_teammate_xml(text: &str) -> Vec<ParsedTeammateMessage> {
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

/// Map agent color name to ratatui Color.
fn teammate_color_to_ratatui(color_name: &str) -> ratatui::style::Color {
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
