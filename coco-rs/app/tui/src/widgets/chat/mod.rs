//! Chat history widget — renders the engine-authoritative
//! `&[RenderedCell]` slice via per-category renderer submodules.
//!
//! The widget and its renderers dispatch on `cell.kind` + `cell.source:
//! Arc<Message>` directly — engine `MessageHistory` is the only source
//! of truth, with no parallel TUI-side projection.

mod render_assistant;
mod render_system;
mod render_tool;
mod render_user;

use std::collections::HashMap;
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
use crate::presentation::thinking::ThinkingDisplay;
use crate::presentation::thinking::ThinkingRenderInput;
use crate::presentation::thinking::format_duration_seconds;
use crate::presentation::thinking::render_thinking_block;
use crate::presentation::transcript::ActiveTranscriptCell;
use crate::presentation::transcript::TRANSCRIPT_LINE_CHAR_CAP;
use crate::presentation::transcript::ToolOutputPreview;
use crate::presentation::transcript::TranscriptCell;
use crate::presentation::transcript::TranscriptPresentationInput;
use crate::presentation::transcript::TranscriptProjectionOptions;
use crate::presentation::transcript::TranscriptSourceCell;
use crate::presentation::transcript::tool_output_preview;
use crate::presentation::transcript::transcript_presentation;
use crate::state::session::ToolExecution;
use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;
use crate::state::transcript_view::SystemCellKind;
use crate::state::ui::StreamingState;
use crate::tool_display::ToolNameTone;
use crate::tool_display::tool_name_tone;

pub(crate) const TOOL_OUTPUT_PREVIEW_ROWS: usize = 5;

/// Chat history widget.
///
/// Phase 3d (§6): consumes the engine-authoritative `&[RenderedCell]`
/// slice from `session.transcript.cells()` end-to-end. The per-category
/// renderers (`render_user/_assistant/_tool/_system`) dispatch on
/// `cell.kind` + `cell.source` directly.
pub struct ChatWidget<'a> {
    cells: &'a [RenderedCell],
    scroll_offset: i32,
    streaming: Option<&'a StreamingState>,
    show_thinking: bool,
    show_system_reminders: bool,
    pub(crate) tool_executions: &'a [ToolExecution],
    collapsed_tools: Option<&'a HashSet<String>>,
    /// Side-cache lookup for `AssistantThinking` cells.
    /// `None` ⇒ no reasoning badges (renderer falls back to header without metrics).
    pub(crate) reasoning_metadata:
        Option<&'a HashMap<uuid::Uuid, crate::state::session::ReasoningMetadata>>,
    pub(crate) styles: UiStyles<'a>,
    pub(crate) syntax_highlighting: SyntaxHighlighting,
    pub(crate) width: u16,
    /// Keybinding handle for rendering live shortcuts (e.g. the
    /// `…(<chord> to see full summary)` hint). `None` falls back to
    /// the default literal — used in tests that build a ChatWidget
    /// without an `AppState`.
    pub(crate) kb_handle: Option<&'a crate::keybinding_resolver::KeybindingHandle>,
    pub(crate) show_thinking_internal: bool,
}

impl<'a> ChatWidget<'a> {
    pub fn new(cells: &'a [RenderedCell], styles: UiStyles<'a>) -> Self {
        Self {
            cells,
            scroll_offset: 0,
            streaming: None,
            show_thinking: false,
            show_system_reminders: false,
            tool_executions: &[],
            collapsed_tools: None,
            reasoning_metadata: None,
            styles,
            syntax_highlighting: SyntaxHighlighting::Enabled,
            width: 80,
            kb_handle: None,
            show_thinking_internal: false,
        }
    }

    pub fn reasoning_metadata(
        mut self,
        meta: &'a HashMap<uuid::Uuid, crate::state::session::ReasoningMetadata>,
    ) -> Self {
        self.reasoning_metadata = Some(meta);
        self
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
        self.show_thinking_internal = show;
        self
    }
    pub fn show_system_reminders(mut self, show: bool) -> Self {
        self.show_system_reminders = show;
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
    /// Build lines that own their text for native history emission.
    pub fn build_lines_owned(&self) -> Vec<Line<'static>> {
        self.build_lines()
    }

    fn build_lines(&self) -> Vec<Line<'static>> {
        let presentation = transcript_presentation(TranscriptPresentationInput {
            cells: self.cells,
            options: TranscriptProjectionOptions {
                show_system_reminders: self.show_system_reminders,
            },
            streaming: self.streaming,
            show_thinking: self.show_thinking,
            tool_executions: self.tool_executions,
        });
        let mut lines: Vec<Line<'static>> = Vec::new();

        for cell in presentation.cells {
            self.render_transcript_cell(&cell, false, false, &mut lines);
        }

        lines
    }

    fn render_transcript_cell(
        &self,
        cell: &TranscriptSourceCell<'_>,
        expanded: bool,
        selected: bool,
        lines: &mut Vec<Line<'static>>,
    ) {
        let start_line = lines.len();
        match cell {
            TranscriptSourceCell::Committed(TranscriptCell::MetaPreview { index }) => {
                if let Some(c) = self.cells.get(*index) {
                    self.render_meta_preview(c, lines);
                }
            }
            TranscriptSourceCell::Committed(TranscriptCell::Cell { index }) => {
                if let Some(c) = self.cells.get(*index) {
                    self.render_cell_with_expansion(c, expanded, lines);
                    lines.push(Line::default());
                }
            }
            TranscriptSourceCell::Committed(TranscriptCell::ToolCall {
                invocation,
                result,
                ..
            }) => {
                self.render_tool_call(*invocation, *result, expanded, lines);
                lines.push(Line::default());
            }
            TranscriptSourceCell::Committed(TranscriptCell::ToolBatch { count, .. }) => {
                lines.push(Line::from(
                    Span::raw(format!(
                        "  ‖ {}",
                        t!("chat.tools_in_parallel", count = count)
                    ))
                    .fg(self.styles.secondary())
                    .dim(),
                ));
                lines.push(Line::default());
            }
            TranscriptSourceCell::Active(ActiveTranscriptCell::Streaming(view)) => {
                self.render_streaming(view.clone(), lines);
            }
            TranscriptSourceCell::Active(ActiveTranscriptCell::BusySpinner) => {
                /// Static fallback glyph for the chat-cell busy spinner.
                /// The animated status-indicator spinner lives in
                /// [`crate::widgets::status_indicator`]; this widget
                /// just needs a single character to anchor the
                /// "processing…" line and never re-renders fast
                /// enough to animate.
                const BUSY_GLYPH: &str = "⠋";
                lines.push(Line::from(vec![
                    Span::raw(format!("{BUSY_GLYPH} ")).fg(self.styles.thinking()),
                    Span::raw(t!("chat.processing").to_string()).fg(self.styles.thinking()),
                ]));
            }
        }
        if selected {
            if start_line < lines.len() {
                if let Some(first) = lines.get_mut(start_line) {
                    first
                        .spans
                        .insert(0, Span::raw("▶ ").fg(self.styles.primary()));
                }
            } else {
                lines.push(Line::from(Span::raw("▶").fg(self.styles.primary())));
            }
        }
    }

    fn render_tool_call(
        &self,
        invocation: Option<usize>,
        result: Option<usize>,
        expanded: bool,
        lines: &mut Vec<Line<'static>>,
    ) {
        if expanded {
            if let Some(index) = invocation
                && let Some(c) = self.cells.get(index)
            {
                self.render_cell(c, lines);
            }
            if let Some(index) = result
                && let Some(c) = self.cells.get(index)
            {
                self.render_cell(c, lines);
            }
            return;
        }

        let invocation_cell = invocation.and_then(|index| self.cells.get(index));
        let result_cell = result.and_then(|index| self.cells.get(index));

        if let Some(cell) = invocation_cell
            && let CellKind::ToolUse { tool_name, call_id } = &cell.kind
        {
            self.render_tool_call_header(tool_name, call_id, &cell.source, lines);
            if let Some(rc) = result_cell {
                self.render_tool_result_summary(rc, lines);
            }
            return;
        }

        if let Some(rc) = result_cell {
            self.render_tool_result_summary(rc, lines);
        }
    }

    fn render_cell_with_expansion(
        &self,
        cell: &RenderedCell,
        expanded: bool,
        lines: &mut Vec<Line<'static>>,
    ) {
        if !expanded {
            match &cell.kind {
                CellKind::AssistantThinking { .. } => {
                    if !self.show_thinking {
                        self.render_cell(cell, lines);
                        return;
                    }
                }
                CellKind::ToolResult { .. } => {
                    self.render_tool_result_summary(cell, lines);
                    return;
                }
                _ => {}
            }
        }
        self.render_cell(cell, lines);
    }

    fn render_tool_call_header(
        &self,
        tool_name: &str,
        call_id: &str,
        source: &std::sync::Arc<coco_messages::Message>,
        lines: &mut Vec<Line<'static>>,
    ) {
        let execution = self
            .tool_executions
            .iter()
            .find(|tool| tool.call_id == call_id);
        let input_preview = crate::state::derive::extract_tool_call_input_preview(source, call_id);
        let preview = single_line_capped(&input_preview, 96);
        let elapsed = execution
            .map(|tool| format!(" ({})", format_duration_seconds(tool.elapsed())))
            .unwrap_or_default();
        let mut spans = vec![
            Span::raw("🔧 ").fg(self.styles.dim()),
            Span::raw(tool_name.to_string())
                .fg(tool_tone_color(tool_name_tone(tool_name), self.styles))
                .bold(),
        ];
        if !preview.is_empty() {
            spans.push(Span::raw(format!("({preview})")).fg(self.styles.text()));
        }
        spans.push(Span::raw(elapsed).fg(self.styles.dim()).dim());
        lines.push(Line::from(spans));
    }

    fn render_tool_result_summary(&self, cell: &RenderedCell, lines: &mut Vec<Line<'static>>) {
        let CellKind::ToolResult { .. } = &cell.kind else {
            return;
        };
        let coco_messages::Message::ToolResult(tr) = cell.source.as_ref() else {
            return;
        };
        let Some((_tool_name, output)) =
            crate::state::derive::tool_result_output(cell.source.as_ref())
        else {
            return;
        };
        if tr.is_error {
            lines.push(result_line(
                format!(
                    "error: {}",
                    single_line_capped(&output, TRANSCRIPT_LINE_CHAR_CAP)
                ),
                self.styles.error(),
            ));
        } else {
            self.render_output_preview(&output, lines);
        }
    }

    pub(crate) fn render_output_preview(&self, output: &str, lines: &mut Vec<Line<'static>>) {
        match tool_output_preview(output, TOOL_OUTPUT_PREVIEW_ROWS) {
            ToolOutputPreview::Empty => {
                lines.push(result_line("(no output)".to_string(), self.styles.dim()));
            }
            ToolOutputPreview::Full(output_lines) => {
                for (index, line) in output_lines.into_iter().enumerate() {
                    lines.push(output_result_line(
                        transcript_safe_line(line),
                        self.styles.text(),
                        index == 0,
                    ));
                }
            }
            ToolOutputPreview::Truncated {
                head,
                omitted,
                tail,
            } => {
                let mut rendered = 0usize;
                for line in head {
                    lines.push(output_result_line(
                        transcript_safe_line(line),
                        self.styles.text(),
                        rendered == 0,
                    ));
                    rendered += 1;
                }
                lines.push(output_result_line(
                    format!("… +{omitted} lines {}", self.expand_hint()),
                    self.styles.dim(),
                    rendered == 0,
                ));
                for line in tail {
                    lines.push(output_result_line(
                        transcript_safe_line(line),
                        self.styles.text(),
                        false,
                    ));
                }
            }
        }
    }

    fn expand_hint(&self) -> String {
        let chord = self
            .kb_handle
            .and_then(|handle| {
                handle.display_for(
                    &coco_keybindings::KeybindingAction::AppToggleTranscript,
                    crate::keybinding_bridge::KeybindingContext::Chat,
                )
            })
            .unwrap_or_else(|| "ctrl+o".to_string());
        format!("({chord} to expand)")
    }

    pub(crate) fn thinking_toggle_hint(&self) -> String {
        let chord = self
            .kb_handle
            .and_then(|handle| {
                handle.display_for(
                    &coco_keybindings::KeybindingAction::ChatThinkingToggle,
                    crate::keybinding_bridge::KeybindingContext::Chat,
                )
            })
            .unwrap_or_else(|| "F2".to_string());
        let key = if self.show_thinking_internal {
            "status.thinking_toggle_collapse"
        } else {
            "status.thinking_toggle_expand"
        };
        t!(key, shortcut = chord.as_str()).to_string()
    }

    /// Render a single-line collapsed preview for a meta (system reminder)
    /// cell. Keeps the user aware that system content exists without
    /// taking vertical space.
    fn render_meta_preview(&self, cell: &RenderedCell, lines: &mut Vec<Line<'static>>) {
        const PREVIEW_CHARS: usize = 50;
        let category = meta_category(&cell.kind);
        let raw = meta_preview_text(cell);
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

    fn render_cell(&self, cell: &RenderedCell, lines: &mut Vec<Line<'static>>) {
        // Dispatch to the first category whose renderer handles the variant.
        // Each submodule returns None when the variant is outside its scope,
        // keeping the individual match statements exhaustive-by-category.
        render_user::try_render(self, cell, lines)
            .or_else(|| render_assistant::try_render(self, cell, lines))
            .or_else(|| render_tool::try_render(self, cell, lines))
            .or_else(|| render_system::try_render(self, cell, lines));
    }

    fn render_streaming(&self, view: StreamingTailView<'_>, lines: &mut Vec<Line<'static>>) {
        for block in view.blocks {
            match block {
                StreamingTailBlock::AssistantText(content) => {
                    self.render_streaming_text(content, lines);
                }
                StreamingTailBlock::Cursor => {
                    lines.push(Line::from(Span::raw("▌").fg(self.styles.accent())));
                }
                StreamingTailBlock::ThinkingTokens { count } => {
                    lines.extend(render_thinking_block(
                        ThinkingRenderInput {
                            content: "",
                            duration_ms: None,
                            reasoning_tokens: Some(count),
                            toggle_hint: Some(&self.thinking_toggle_hint()),
                            display: ThinkingDisplay::Collapsed,
                        },
                        self.styles,
                    ));
                }
            }
        }
    }

    fn render_streaming_text(&self, content: &str, lines: &mut Vec<Line<'static>>) {
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

/// Short category label for a meta-preview row. Categorizes by
/// `CellKind` / system-cell sub-variant so users can identify what's
/// hidden by the system-reminder collapse.
fn meta_category(kind: &CellKind) -> &'static str {
    match kind {
        CellKind::System(SystemCellKind::Informational) => "system",
        CellKind::System(SystemCellKind::ApiError) => "api",
        CellKind::System(SystemCellKind::CompactBoundary) => "compact",
        CellKind::System(SystemCellKind::PermissionRetry) => "permission",
        CellKind::System(SystemCellKind::BridgeStatus) => "bridge",
        CellKind::System(SystemCellKind::MemorySaved) => "memory",
        CellKind::System(SystemCellKind::AwaySummary) => "away",
        CellKind::System(SystemCellKind::AgentsKilled) => "agents",
        CellKind::System(SystemCellKind::ApiMetrics) => "metrics",
        CellKind::System(SystemCellKind::StopHookSummary) => "hook",
        CellKind::System(SystemCellKind::TurnDuration) => "turn",
        CellKind::System(SystemCellKind::ScheduledTaskFire) => "schedule",
        _ => "meta",
    }
}

/// Best-effort short text for the meta-preview row. Walks
/// `cell.source` for the human-readable payload of each
/// `SystemMessage` sub-variant.
fn meta_preview_text(cell: &RenderedCell) -> String {
    use coco_messages::Message;
    use coco_messages::SystemMessage as SM;
    let Message::System(sm) = cell.source.as_ref() else {
        return String::new();
    };
    match sm {
        SM::Informational(info) => {
            if info.title.is_empty() {
                info.message.clone()
            } else {
                format!("{}: {}", info.title, info.message)
            }
        }
        SM::ApiError(e) => e.error.clone(),
        SM::CompactBoundary(_) => String::new(),
        SM::PermissionRetry(m) => format!("{} · {}", m.tool_name, m.message),
        SM::BridgeStatus(m) => m.message.clone().unwrap_or_default(),
        SM::LocalCommand(lc) => lc.command.clone(),
        SM::UserInterruption(_)
        | SM::MicrocompactBoundary(_)
        | SM::MemorySaved(_)
        | SM::AwaySummary(_)
        | SM::AgentsKilled(_)
        | SM::ApiMetrics(_)
        | SM::StopHookSummary(_)
        | SM::TurnDuration(_)
        | SM::ScheduledTaskFire(_) => String::new(),
    }
}

fn result_line(text: String, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![Span::raw("  └ ").fg(color), Span::raw(text).fg(color)])
}

fn tool_tone_color(
    tone: ToolNameTone,
    styles: crate::presentation::styles::UiStyles<'_>,
) -> ratatui::style::Color {
    match tone {
        ToolNameTone::ReadOnly => styles.success(),
        ToolNameTone::Shell => styles.primary(),
        ToolNameTone::Write => styles.warning(),
        ToolNameTone::Agent => styles.accent(),
        ToolNameTone::Plan => styles.plan(),
        ToolNameTone::Utility => styles.secondary(),
    }
}

fn output_result_line(text: String, color: ratatui::style::Color, first: bool) -> Line<'static> {
    let prefix = if first { "  └ " } else { "    " };
    Line::from(vec![Span::raw(prefix).fg(color), Span::raw(text).fg(color)])
}

fn single_line_capped(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut count = 0usize;
    let mut first = true;
    let mut truncated = false;

    'words: for word in text.split_whitespace() {
        if !first {
            if count + 1 >= max_chars {
                truncated = true;
                break;
            }
            out.push(' ');
            count += 1;
        }
        first = false;
        for ch in word.chars() {
            if count + 1 >= max_chars {
                truncated = true;
                break 'words;
            }
            out.push(ch);
            count += 1;
        }
    }

    if truncated {
        out.push('…');
    }
    out
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out = text.chars().take(max.saturating_sub(1)).collect::<String>();
    out.push('…');
    out
}

pub(super) fn transcript_safe_line(line: &str) -> String {
    truncate_chars(line, TRANSCRIPT_LINE_CHAR_CAP)
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
