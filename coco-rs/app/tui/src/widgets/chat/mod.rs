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

use crate::display_settings::SyntaxHighlighting;
use crate::i18n::t;
use crate::presentation::streaming::StreamingTailBlock;
use crate::presentation::streaming::StreamingTailView;
use crate::presentation::styles::UiStyles;
use crate::presentation::transcript::ActiveTranscriptCell;
use crate::presentation::transcript::TaskNotificationBatchKind;
use crate::presentation::transcript::TaskNotificationTone;
use crate::presentation::transcript::TranscriptCell;
use crate::presentation::transcript::TranscriptPresentationInput;
use crate::presentation::transcript::TranscriptProjectionOptions;
use crate::presentation::transcript::TranscriptSourceCell;
use crate::presentation::transcript::transcript_presentation;
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;
use crate::state::session::ToolExecution;
use crate::state::ui::StreamingState;

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
    styles: UiStyles<'a>,
    syntax_highlighting: SyntaxHighlighting,
    width: u16,
    /// Keybinding handle for rendering live shortcuts (e.g. the
    /// `…(<chord> to see full summary)` hint). `None` falls back to
    /// the default literal — used in tests that build a ChatWidget
    /// without an `AppState`.
    pub(crate) kb_handle: Option<&'a crate::keybinding_resolver::KeybindingHandle>,
}

impl<'a> ChatWidget<'a> {
    pub fn new(messages: &'a [ChatMessage], styles: UiStyles<'a>) -> Self {
        Self {
            messages,
            scroll_offset: 0,
            streaming: None,
            show_thinking: true,
            show_system_reminders: false,
            spinner_frame: "⠋",
            tool_executions: &[],
            collapsed_tools: None,
            styles,
            syntax_highlighting: SyntaxHighlighting::Enabled,
            width: 80,
            kb_handle: None,
        }
    }

    pub fn kb_handle(mut self, handle: &'a crate::keybinding_resolver::KeybindingHandle) -> Self {
        self.kb_handle = Some(handle);
        self
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
    pub fn syntax_highlighting(mut self, syntax_highlighting: SyntaxHighlighting) -> Self {
        self.syntax_highlighting = syntax_highlighting;
        self
    }

    /// Build lines that own their text — needed by the transcript
    /// overlay which can't borrow from the widget across the
    /// `(title, body, color)` return tuple. Cloned `Line`s are cheap
    /// here because the transcript only renders on Esc / Ctrl+O, not
    /// every frame.
    pub fn build_lines_owned(&self) -> Vec<Line<'static>> {
        self.build_lines()
            .into_iter()
            .map(|line| {
                let spans: Vec<Span<'static>> = line
                    .spans
                    .into_iter()
                    .map(|s| Span::styled(s.content.into_owned(), s.style))
                    .collect();
                Line::from(spans)
                    .style(line.style)
                    .alignment(line.alignment.unwrap_or_default())
            })
            .collect()
    }

    fn build_lines(&self) -> Vec<Line<'a>> {
        let presentation = transcript_presentation(TranscriptPresentationInput {
            messages: self.messages,
            options: TranscriptProjectionOptions {
                show_system_reminders: self.show_system_reminders,
            },
            streaming: self.streaming,
            show_thinking: self.show_thinking,
            tool_executions: self.tool_executions,
        });
        let mut lines: Vec<Line> = Vec::new();

        for cell in presentation.cells {
            match cell {
                TranscriptSourceCell::Committed(TranscriptCell::MetaPreview { index }) => {
                    self.render_meta_preview(&self.messages[index], &mut lines);
                }
                TranscriptSourceCell::Committed(TranscriptCell::Message { index }) => {
                    self.render_message(&self.messages[index], &mut lines);
                    lines.push(Line::default());
                }
                TranscriptSourceCell::Committed(TranscriptCell::ToolBatch {
                    start,
                    end,
                    count,
                }) => {
                    lines.push(Line::from(
                        Span::raw(format!(
                            "  ‖ {}",
                            t!("chat.tools_in_parallel", count = count)
                        ))
                        .fg(self.styles.secondary())
                        .dim(),
                    ));
                    for j in start..end {
                        self.render_message(&self.messages[j], &mut lines);
                    }
                    lines.push(Line::default());
                }
                TranscriptSourceCell::Committed(TranscriptCell::HookBatch {
                    count,
                    hook_name,
                    has_error,
                    ..
                }) => {
                    self.render_hook_batch(count, &hook_name, has_error, &mut lines);
                }
                TranscriptSourceCell::Committed(TranscriptCell::TaskNotification {
                    summary,
                    tone,
                    ..
                }) => {
                    self.render_task_notification(&summary, tone, &mut lines);
                }
                TranscriptSourceCell::Committed(TranscriptCell::TaskNotificationBatch {
                    count,
                    kind,
                    ..
                }) => {
                    self.render_task_notification_batch(count, kind, &mut lines);
                }
                TranscriptSourceCell::Active(ActiveTranscriptCell::Streaming(view)) => {
                    self.render_streaming(view, &mut lines);
                }
                TranscriptSourceCell::Active(ActiveTranscriptCell::BusySpinner) => {
                    lines.push(Line::from(vec![
                        Span::raw(format!("{} ", self.spinner_frame)).fg(self.styles.thinking()),
                        Span::raw(t!("chat.processing").to_string()).fg(self.styles.thinking()),
                    ]));
                }
            }
        }

        lines
    }

    fn render_hook_batch(
        &self,
        count: usize,
        hook_name: &str,
        has_error: bool,
        lines: &mut Vec<Line<'a>>,
    ) {
        let color = if has_error {
            self.styles.warning()
        } else {
            self.styles.accent()
        };
        lines.push(Line::from(vec![
            Span::raw("  ⚙ ").fg(color),
            Span::raw(hook_name.to_string()).fg(self.styles.dim()),
            Span::raw(": ").fg(self.styles.dim()),
            Span::raw(t!("chat.hook_batch", count = count).to_string()).fg(color),
        ]));
        lines.push(Line::default());
    }

    fn render_task_notification(
        &self,
        summary: &str,
        tone: TaskNotificationTone,
        lines: &mut Vec<Line<'a>>,
    ) {
        let color = match tone {
            TaskNotificationTone::Completed => self.styles.success(),
            TaskNotificationTone::Failed => self.styles.error(),
            TaskNotificationTone::Killed => self.styles.warning(),
            TaskNotificationTone::Unknown => self.styles.dim(),
        };
        lines.push(Line::from(vec![
            Span::raw("  ● ").fg(color),
            Span::raw(summary.to_string()).fg(color),
        ]));
        lines.push(Line::default());
    }

    fn render_task_notification_batch(
        &self,
        count: usize,
        kind: TaskNotificationBatchKind,
        lines: &mut Vec<Line<'a>>,
    ) {
        let label = match kind {
            TaskNotificationBatchKind::BackgroundBashCompleted => {
                t!("chat.background_bash_batch", count = count).to_string()
            }
            TaskNotificationBatchKind::TeammateShutdown => {
                t!("chat.teammate_shutdown_batch", count = count).to_string()
            }
        };
        lines.push(Line::from(vec![
            Span::raw("  ● ").fg(self.styles.success()),
            Span::raw(label).fg(self.styles.dim()),
        ]));
        lines.push(Line::default());
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
            Span::raw(format!("  # [{category}] ")).fg(self.styles.system_message()),
            Span::raw(preview).fg(self.styles.dim()).italic(),
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

    fn render_streaming(&self, view: StreamingTailView<'_>, lines: &mut Vec<Line<'a>>) {
        for block in view.blocks {
            match block {
                StreamingTailBlock::AssistantText(content) => {
                    self.render_streaming_text(content, lines);
                }
                StreamingTailBlock::Cursor => {
                    lines.push(Line::from(Span::raw("▌").fg(self.styles.accent())));
                }
                StreamingTailBlock::ThinkingTokens { count } => {
                    lines.push(Line::from(
                        Span::raw(format!(
                            "  💭 {}",
                            t!("chat.thinking_tokens", count = count)
                        ))
                        .fg(self.styles.thinking())
                        .italic(),
                    ));
                }
            }
        }
    }

    fn render_streaming_text(&self, content: &str, lines: &mut Vec<Line<'a>>) {
        let mut md_lines = crate::widgets::markdown::markdown_to_lines_with_syntax(
            content,
            self.styles,
            self.width,
            self.syntax_highlighting,
        );
        // Match `render_assistant::try_render`'s leading dot so the
        // partial response and the finalised response share the same
        // marker — otherwise the row jumps when streaming finishes
        // and the assistant text replaces the live buffer.
        if let Some(first) = md_lines.first_mut() {
            let dot_span = Span::styled(
                "⏺ ".to_string(),
                ratatui::style::Style::default().fg(self.styles.assistant_message()),
            );
            let leading_is_indent = first
                .spans
                .first()
                .map(|s| s.content.as_ref() == "  ")
                .unwrap_or(false);
            if leading_is_indent {
                first.spans[0] = dot_span;
            } else {
                first.spans.insert(0, dot_span);
            }
        }
        lines.extend(md_lines);
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
        MessageContent::CompactSummary { .. } => "compact",
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
