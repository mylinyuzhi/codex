use std::collections::HashMap;
use std::collections::HashSet;

use coco_keybindings::KeybindingAction;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::prelude::Stylize;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

use crate::i18n::t;
use crate::keybinding_bridge::KeybindingContext as TuiContext;
use crate::presentation::layout::text_width;
use crate::presentation::streaming::StreamingTailBlock;
use crate::presentation::styles::UiStyles;
use crate::presentation::thinking::ThinkingDisplay;
use crate::presentation::thinking::ThinkingRenderInput;
use crate::presentation::thinking::format_duration_seconds;
use crate::presentation::thinking::render_thinking_block;
use crate::presentation::transcript::TRANSCRIPT_COLLAPSED_PREVIEW_LINES;
use crate::presentation::transcript::TRANSCRIPT_EXPANDED_CELL_LINE_CAP;
use crate::presentation::transcript::TRANSCRIPT_LINE_CHAR_CAP;
use crate::presentation::transcript::TRANSCRIPT_TRUNCATED_HINT;
use crate::presentation::transcript::TaskNotificationBatchKind;
use crate::presentation::transcript::TaskNotificationTone;
use crate::presentation::transcript::ToolOutputPreview;
use crate::presentation::transcript::TranscriptCell;
use crate::presentation::transcript::TranscriptSourceCell;
use crate::presentation::transcript::tool_output_preview;
use crate::presentation::transcript::transcript_presentation_for_state;
use crate::state::AppState;
use crate::state::session::ChatMessage;
use crate::state::session::MessageContent;
use crate::state::session::ToolExecution;
use crate::state::session::ToolUseStatus;
use crate::state::transcript::TranscriptCellId;
use crate::state::transcript::TranscriptScrollPosition;
use crate::state::transcript::TranscriptState;
use crate::tool_display::ToolNameTone;
use crate::tool_display::tool_name_tone;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TranscriptHeightCacheKey {
    cell_id: TranscriptCellId,
    width: u16,
    expanded: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TranscriptLayoutIndex {
    content_generation: Option<u64>,
    prefix_generation: Option<u64>,
    heights: HashMap<TranscriptHeightCacheKey, usize>,
    prefix: Vec<Option<usize>>,
}

impl TranscriptLayoutIndex {
    pub(crate) fn reset(&mut self) {
        self.content_generation = None;
        self.prefix_generation = None;
        self.heights.clear();
        self.prefix.clear();
    }

    fn begin_frame(&mut self, content_generation: u64, prefix_generation: u64, cell_count: usize) {
        if self.content_generation != Some(content_generation) {
            self.reset();
            self.content_generation = Some(content_generation);
        }
        if self.prefix_generation != Some(prefix_generation) || self.prefix.len() != cell_count + 1
        {
            self.prefix_generation = Some(prefix_generation);
            self.prefix.clear();
            self.prefix.resize(cell_count + 1, None);
        }
        self.prefix[0] = Some(0);
    }

    fn invalidate_prefix_from(&mut self, index: usize) {
        for prefix in self.prefix.iter_mut().skip(index.saturating_add(1)) {
            *prefix = None;
        }
    }
}

pub(crate) struct TranscriptStateWidget<'a> {
    state: &'a AppState,
    transcript: &'a TranscriptState,
    layout_index: &'a mut TranscriptLayoutIndex,
    styles: UiStyles<'a>,
}

impl<'a> TranscriptStateWidget<'a> {
    pub(crate) fn new(
        state: &'a AppState,
        transcript: &'a TranscriptState,
        layout_index: &'a mut TranscriptLayoutIndex,
        styles: UiStyles<'a>,
    ) -> Self {
        Self {
            state,
            transcript,
            layout_index,
            styles,
        }
    }
}

impl Widget for TranscriptStateWidget<'_> {
    fn render(mut self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(t!("transcript.title").to_string())
            .border_style(Style::default().fg(self.styles.primary()));
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.is_empty() {
            return;
        }

        let footer_height = if inner.height > 2 {
            2
        } else {
            u16::from(inner.height > 1)
        };
        let content_area = Rect {
            height: inner.height.saturating_sub(footer_height),
            ..inner
        };
        let footer_area = Rect {
            y: inner.bottom().saturating_sub(footer_height),
            height: footer_height,
            ..inner
        };

        if content_area.height > 0 {
            self.render_cells(content_area, buf);
        }
        if footer_area.height > 0 {
            self.render_footer(footer_area, buf);
        }
    }
}

impl TranscriptStateWidget<'_> {
    fn render_cells(&mut self, area: Rect, buf: &mut Buffer) {
        let presentation = transcript_presentation_for_state(self.state);
        self.layout_index.begin_frame(
            transcript_layout_generation(self.state),
            transcript_prefix_generation(
                self.state,
                &presentation.cells,
                area.width,
                &self.transcript.collapsed_cell_ids,
            ),
            presentation.cells.len(),
        );
        if presentation.cells.is_empty() {
            Line::from(Span::raw(t!("transcript.empty").to_string()).fg(self.styles.dim()))
                .render(Rect { height: 1, ..area }, buf);
            return;
        }

        let mut renderer = TranscriptCellRenderer::new(self.state, self.styles, area.width);
        let visible = {
            let mut pager = TranscriptPager::new(
                &presentation.cells,
                &self.state.session.messages,
                &mut renderer,
                &self.transcript.collapsed_cell_ids,
                self.transcript.selected_cell_id.as_ref(),
                self.layout_index,
            );
            let scroll =
                effective_scroll(&self.transcript.scroll, &mut pager, area.height as usize);
            let visible = pager.visible_cells(scroll, area.height as usize);
            visible.cells
        };

        let mut y = area.y;
        for cell in visible {
            if y >= area.bottom() {
                break;
            }
            let cell_area = Rect {
                x: area.x,
                y,
                width: area.width,
                height: (area.bottom() - y).min(cell.height.saturating_sub(cell.skip) as u16),
            };
            if cell_area.height == 0 {
                continue;
            }
            let source = &presentation.cells[cell.index];
            let id = source.cell_id(&self.state.session.messages);
            let expanded = id
                .as_ref()
                .is_none_or(|id| !self.transcript.collapsed_cell_ids.contains(id));
            let selected = id.as_ref() == self.transcript.selected_cell_id.as_ref();
            renderer.render_window(source, cell_area, cell.skip, expanded, selected, buf);
            y = y.saturating_add(cell_area.height);
        }
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        let toggle_chord = self
            .state
            .ui
            .kb_handle
            .display_for(&KeybindingAction::AppToggleTranscript, TuiContext::Chat)
            .unwrap_or_else(|| "ctrl+o".to_string());
        let nav = t!("transcript.hint_footer_nav", toggle = toggle_chord.as_str()).to_string();
        Line::from(Span::raw(nav).fg(self.styles.dim())).render(Rect { height: 1, ..area }, buf);
        if area.height > 1 {
            let actions = t!("transcript.hint_footer_actions").to_string();
            Line::from(Span::raw(actions).fg(self.styles.dim())).render(
                Rect {
                    y: area.y.saturating_add(1),
                    height: 1,
                    ..area
                },
                buf,
            );
        }
    }
}

struct TranscriptCellRenderer<'a> {
    messages: &'a [ChatMessage],
    tool_executions: &'a [ToolExecution],
    width: u16,
    styles: UiStyles<'a>,
}

impl<'a> TranscriptCellRenderer<'a> {
    fn new(state: &'a AppState, styles: UiStyles<'a>, width: u16) -> Self {
        Self {
            messages: &state.session.messages,
            tool_executions: &state.session.tool_executions,
            width,
            styles,
        }
    }

    fn desired_height(
        &self,
        cell: &TranscriptSourceCell<'a>,
        expanded: bool,
        selected: bool,
    ) -> usize {
        let lines = self.render_cell(cell, expanded, selected);
        wrapped_height(&lines, self.width)
    }

    fn render_window(
        &self,
        cell: &TranscriptSourceCell<'a>,
        area: Rect,
        skip_lines: usize,
        expanded: bool,
        selected: bool,
        buf: &mut Buffer,
    ) {
        let lines = self.render_cell(cell, expanded, selected);
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((skip_lines.min(u16::MAX as usize) as u16, 0))
            .render(area, buf);
    }

    fn render_cell(
        &self,
        cell: &TranscriptSourceCell<'a>,
        expanded: bool,
        selected: bool,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        match cell {
            TranscriptSourceCell::Committed(TranscriptCell::MetaPreview { index }) => {
                self.render_meta_preview(&self.messages[*index], &mut lines);
            }
            TranscriptSourceCell::Committed(TranscriptCell::Message { index }) => {
                self.render_message(&self.messages[*index], expanded, &mut lines);
                lines.push(Line::default());
            }
            TranscriptSourceCell::Committed(TranscriptCell::ToolCall {
                invocation,
                result,
                ..
            }) => {
                self.render_tool_call(*invocation, *result, expanded, &mut lines);
                lines.push(Line::default());
            }
            TranscriptSourceCell::Committed(TranscriptCell::ToolBatch { start, end, count }) => {
                lines.push(Line::from(
                    Span::raw(format!(
                        "  ‖ {}",
                        t!("chat.tools_in_parallel", count = count)
                    ))
                    .fg(self.styles.secondary())
                    .dim(),
                ));
                for index in *start..*end {
                    self.render_message(&self.messages[index], expanded, &mut lines);
                }
                lines.push(Line::default());
            }
            TranscriptSourceCell::Committed(TranscriptCell::HookBatch {
                count,
                hook_name,
                has_error,
                ..
            }) => self.render_hook_batch(*count, hook_name, *has_error, &mut lines),
            TranscriptSourceCell::Committed(TranscriptCell::TaskNotification {
                summary,
                tone,
                ..
            }) => self.render_task_notification(summary, *tone, &mut lines),
            TranscriptSourceCell::Committed(TranscriptCell::TaskNotificationBatch {
                count,
                kind,
                ..
            }) => self.render_task_notification_batch(*count, *kind, &mut lines),
            TranscriptSourceCell::Active(active) => match active {
                crate::presentation::transcript::ActiveTranscriptCell::Streaming(view) => {
                    for block in &view.blocks {
                        match block {
                            StreamingTailBlock::AssistantText(text) => {
                                self.render_text_block("⏺", text, &mut lines);
                            }
                            StreamingTailBlock::Cursor => {
                                lines.push(Line::from(Span::raw("▌").fg(self.styles.accent())));
                            }
                            StreamingTailBlock::ThinkingTokens { count } => {
                                lines.extend(render_thinking_block(
                                    ThinkingRenderInput {
                                        content: "",
                                        duration_ms: None,
                                        reasoning_tokens: Some(*count),
                                        display: ThinkingDisplay::Collapsed,
                                    },
                                    self.styles,
                                ));
                            }
                        }
                    }
                }
                crate::presentation::transcript::ActiveTranscriptCell::BusySpinner => {
                    lines.push(Line::from(
                        Span::raw(format!("  {}", t!("chat.processing")))
                            .fg(self.styles.thinking()),
                    ));
                }
            },
        }

        if selected {
            if let Some(first) = lines.first_mut() {
                first
                    .spans
                    .insert(0, Span::raw("▶ ").fg(self.styles.primary()));
            } else {
                lines.push(Line::from(Span::raw("▶").fg(self.styles.primary())));
            }
        }
        lines
    }

    fn render_tool_call(
        &self,
        invocation: Option<usize>,
        result: Option<usize>,
        expanded: bool,
        lines: &mut Vec<Line<'static>>,
    ) {
        if expanded {
            let invocation = invocation.and_then(|index| self.messages.get(index));
            let result = result.and_then(|index| self.messages.get(index));
            if let Some(msg) = invocation
                && let MessageContent::ToolUse {
                    tool_name,
                    call_id,
                    input_preview,
                    status,
                } = &msg.content
            {
                self.render_tool_call_header(tool_name, call_id, input_preview, *status, lines);
            }
            if let Some(result) = result {
                self.render_tool_result_full(&result.content, lines);
            } else if let Some(msg) = invocation {
                self.render_message(msg, expanded, lines);
            }
            return;
        }

        let invocation = invocation.and_then(|index| self.messages.get(index));
        let result = result.and_then(|index| self.messages.get(index));
        if let Some(msg) = invocation
            && let MessageContent::ToolUse {
                tool_name,
                call_id,
                input_preview,
                status,
            } = &msg.content
        {
            self.render_tool_call_header(tool_name, call_id, input_preview, *status, lines);
            if let Some(result) = result {
                self.render_tool_result_summary(&result.content, lines);
            }
            return;
        }
        if let Some(result) = result {
            self.render_tool_result_header(&result.content, lines);
            self.render_tool_result_summary(&result.content, lines);
        }
    }

    fn render_message(&self, msg: &ChatMessage, expanded: bool, lines: &mut Vec<Line<'static>>) {
        match &msg.content {
            MessageContent::Text(text) => self.render_text_block(">", text, lines),
            MessageContent::AssistantText(text) => self.render_text_block("⏺", text, lines),
            MessageContent::SystemText(text) => self.render_text_block("#", text, lines),
            MessageContent::Thinking {
                content,
                duration_ms,
                reasoning_tokens,
            } => lines.extend(render_thinking_block(
                ThinkingRenderInput {
                    content,
                    duration_ms: *duration_ms,
                    reasoning_tokens: *reasoning_tokens,
                    display: if expanded {
                        ThinkingDisplay::Expanded {
                            max_body_lines: TRANSCRIPT_EXPANDED_CELL_LINE_CAP,
                            truncated_hint: TRANSCRIPT_TRUNCATED_HINT,
                        }
                    } else {
                        ThinkingDisplay::Collapsed
                    },
                },
                self.styles,
            )),
            MessageContent::ToolUse {
                tool_name,
                call_id,
                input_preview,
                status,
            } => self.render_tool_call_header(tool_name, call_id, input_preview, *status, lines),
            MessageContent::ToolSuccess { tool_name, output } => {
                lines.push(Line::from(vec![
                    Span::raw("  ● ").fg(self.styles.tool_completed()),
                    Span::raw(tool_name.clone()).fg(self.styles.text()).bold(),
                ]));
                self.render_capped_lines("    ", output, self.styles.text(), lines);
            }
            MessageContent::ToolError { tool_name, error } => {
                lines.push(Line::from(vec![
                    Span::raw("  ● ").fg(self.styles.tool_error()),
                    Span::raw(tool_name.clone()).fg(self.styles.text()).bold(),
                    Span::raw(": ").fg(self.styles.dim()),
                    Span::raw(transcript_safe_line(error)).fg(self.styles.error()),
                ]));
            }
            MessageContent::ToolRejected { tool_name, reason } => {
                lines.push(Line::from(vec![
                    Span::raw("  ⊘ ").fg(self.styles.warning()),
                    Span::raw(t!("chat.tool_rejected", tool_name = tool_name).to_string())
                        .fg(self.styles.dim()),
                    Span::raw(transcript_safe_line(reason)).fg(self.styles.warning()),
                ]));
            }
            MessageContent::ToolCanceled { tool_name } => {
                lines.push(Line::from(
                    Span::raw(t!("chat.tool_canceled", tool_name = tool_name).to_string())
                        .fg(self.styles.dim())
                        .italic(),
                ));
            }
            MessageContent::InterruptionMarker { .. } => {
                lines.push(Line::from(
                    Span::raw(t!("chat.interrupted_marker").to_string()).fg(self.styles.dim()),
                ));
            }
            _ => self.render_text_block("•", msg.text_content(), lines),
        }
    }

    fn render_tool_call_header(
        &self,
        tool_name: &str,
        call_id: &str,
        input_preview: &str,
        _status: ToolUseStatus,
        lines: &mut Vec<Line<'static>>,
    ) {
        let execution = self
            .tool_executions
            .iter()
            .find(|tool| tool.call_id == call_id);
        let preview = single_line_capped(input_preview, 96);
        let elapsed = execution
            .map(|tool| format!(" ({})", format_duration_seconds(tool.elapsed())))
            .unwrap_or_default();
        let mut spans = vec![
            Span::raw("🔨 ").fg(self.styles.dim()),
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

    fn render_tool_result_summary(&self, content: &MessageContent, lines: &mut Vec<Line<'static>>) {
        match content {
            MessageContent::ToolSuccess { output, .. } => self.render_output_preview(output, lines),
            MessageContent::ToolError { error, .. } => {
                lines.push(result_line(
                    format!(
                        "error: {}",
                        single_line_capped(error, TRANSCRIPT_LINE_CHAR_CAP)
                    ),
                    self.styles.error(),
                ));
            }
            MessageContent::ToolRejected { reason, .. } => {
                lines.push(result_line(
                    format!(
                        "rejected: {}",
                        single_line_capped(reason, TRANSCRIPT_LINE_CHAR_CAP)
                    ),
                    self.styles.warning(),
                ));
            }
            MessageContent::ToolCanceled { .. } => {
                lines.push(result_line("canceled".to_string(), self.styles.dim()));
            }
            _ => {}
        }
    }

    fn render_tool_result_header(&self, content: &MessageContent, lines: &mut Vec<Line<'static>>) {
        match content {
            MessageContent::ToolSuccess { tool_name, .. } => {
                lines.push(Line::from(vec![
                    Span::raw("● ").fg(self.styles.tool_completed()),
                    Span::raw(tool_name.clone()).fg(self.styles.text()).bold(),
                ]));
            }
            MessageContent::ToolError { tool_name, .. } => {
                lines.push(Line::from(vec![
                    Span::raw("● ").fg(self.styles.tool_error()),
                    Span::raw(tool_name.clone()).fg(self.styles.text()).bold(),
                ]));
            }
            MessageContent::ToolRejected { tool_name, .. } => {
                lines.push(Line::from(vec![
                    Span::raw("⊘ ").fg(self.styles.warning()),
                    Span::raw(t!("chat.tool_rejected", tool_name = tool_name).to_string())
                        .fg(self.styles.dim()),
                ]));
            }
            MessageContent::ToolCanceled { tool_name } => {
                lines.push(Line::from(
                    Span::raw(t!("chat.tool_canceled", tool_name = tool_name).to_string())
                        .fg(self.styles.dim())
                        .italic(),
                ));
            }
            _ => {}
        }
    }

    fn render_tool_result_full(&self, content: &MessageContent, lines: &mut Vec<Line<'static>>) {
        match content {
            MessageContent::ToolSuccess { output, .. } => {
                if output.is_empty() {
                    lines.push(result_line("(no output)".to_string(), self.styles.dim()));
                } else {
                    self.render_capped_lines("    ", output, self.styles.text(), lines);
                }
            }
            MessageContent::ToolError { error, .. } => {
                lines.push(result_line(
                    format!("error: {}", transcript_safe_line(error)),
                    self.styles.error(),
                ));
            }
            MessageContent::ToolRejected { reason, .. } => {
                lines.push(result_line(
                    format!("rejected: {}", transcript_safe_line(reason)),
                    self.styles.warning(),
                ));
            }
            MessageContent::ToolCanceled { .. } => {
                lines.push(result_line("canceled".to_string(), self.styles.dim()));
            }
            _ => {}
        }
    }

    fn render_output_preview(&self, output: &str, lines: &mut Vec<Line<'static>>) {
        match tool_output_preview(output, TRANSCRIPT_COLLAPSED_PREVIEW_LINES) {
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
                    format!("… +{omitted} lines (ctrl+o to expand)"),
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

    fn render_text_block(&self, marker: &str, text: &str, lines: &mut Vec<Line<'static>>) {
        let mut iter = text.lines();
        if let Some(first) = iter.next() {
            lines.push(Line::from(vec![
                Span::raw(format!("{marker} ")).fg(self.styles.dim()),
                Span::raw(transcript_safe_line(first)).fg(self.styles.text()),
            ]));
            for line in iter.take(TRANSCRIPT_EXPANDED_CELL_LINE_CAP.saturating_sub(1)) {
                lines.push(Line::from(
                    Span::raw(format!("  {}", transcript_safe_line(line))).fg(self.styles.text()),
                ));
            }
        } else {
            lines.push(Line::from(
                Span::raw(marker.to_string()).fg(self.styles.dim()),
            ));
        }
    }

    fn render_capped_lines(
        &self,
        prefix: &str,
        text: &str,
        color: ratatui::style::Color,
        lines: &mut Vec<Line<'static>>,
    ) {
        let mut iter = text.lines();
        for line in iter.by_ref().take(TRANSCRIPT_EXPANDED_CELL_LINE_CAP) {
            lines.push(Line::from(
                Span::raw(format!("{prefix}{}", transcript_safe_line(line))).fg(color),
            ));
        }
        if iter.next().is_some() {
            lines.push(Line::from(
                Span::raw(format!("{prefix}{TRANSCRIPT_TRUNCATED_HINT}"))
                    .fg(self.styles.dim())
                    .italic(),
            ));
        }
    }

    fn render_hook_batch(
        &self,
        count: usize,
        hook_name: &str,
        has_error: bool,
        lines: &mut Vec<Line<'static>>,
    ) {
        let color = if has_error {
            self.styles.warning()
        } else {
            self.styles.accent()
        };
        lines.push(Line::from(vec![
            Span::raw("  ").fg(color),
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
        lines: &mut Vec<Line<'static>>,
    ) {
        let color = match tone {
            TaskNotificationTone::Completed => self.styles.success(),
            TaskNotificationTone::Failed => self.styles.error(),
            TaskNotificationTone::Killed => self.styles.warning(),
            TaskNotificationTone::Unknown => self.styles.dim(),
        };
        lines.push(Line::from(vec![
            Span::raw("  ").fg(color),
            Span::raw(summary.to_string()).fg(color),
        ]));
        lines.push(Line::default());
    }

    fn render_task_notification_batch(
        &self,
        count: usize,
        kind: TaskNotificationBatchKind,
        lines: &mut Vec<Line<'static>>,
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
            Span::raw("  ").fg(self.styles.success()),
            Span::raw(label).fg(self.styles.dim()),
        ]));
        lines.push(Line::default());
    }

    fn render_meta_preview(&self, msg: &ChatMessage, lines: &mut Vec<Line<'static>>) {
        const PREVIEW_CHARS: usize = 50;
        let raw = msg.text_content();
        let single_line = raw.lines().next().unwrap_or("");
        let trimmed = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
        let preview = truncate_chars(&trimmed, PREVIEW_CHARS);
        lines.push(Line::from(vec![
            Span::raw("  # [meta] ").fg(self.styles.system_message()),
            Span::raw(preview).fg(self.styles.dim()).italic(),
        ]));
    }
}

struct TranscriptPager<'cells, 'state, 'r> {
    cells: &'cells [TranscriptSourceCell<'state>],
    messages: &'state [ChatMessage],
    renderer: &'r mut TranscriptCellRenderer<'state>,
    collapsed_cell_ids: &'cells HashSet<TranscriptCellId>,
    layout_index: &'r mut TranscriptLayoutIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VisibleCell {
    index: usize,
    skip: usize,
    height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisibleCells {
    cells: Vec<VisibleCell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisibleScan {
    cells: Vec<VisibleCell>,
    reached_end: bool,
    total_height: usize,
}

impl<'cells, 'state, 'r> TranscriptPager<'cells, 'state, 'r> {
    fn new(
        cells: &'cells [TranscriptSourceCell<'state>],
        messages: &'state [ChatMessage],
        renderer: &'r mut TranscriptCellRenderer<'state>,
        collapsed_cell_ids: &'cells HashSet<TranscriptCellId>,
        _selected_cell_id: Option<&'cells TranscriptCellId>,
        layout_index: &'r mut TranscriptLayoutIndex,
    ) -> Self {
        Self {
            cells,
            messages,
            renderer,
            collapsed_cell_ids,
            layout_index,
        }
    }

    fn visible_cells(&mut self, scroll: usize, viewport_height: usize) -> VisibleCells {
        let scan = self.scan_visible_cells(scroll, viewport_height);
        if scan.reached_end && scroll > scan.total_height.saturating_sub(viewport_height) {
            let max_scroll = scan.total_height.saturating_sub(viewport_height);
            let clamped = self.scan_visible_cells(max_scroll, viewport_height);
            return VisibleCells {
                cells: clamped.cells,
            };
        }

        VisibleCells { cells: scan.cells }
    }

    fn scan_visible_cells(&mut self, scroll: usize, viewport_height: usize) -> VisibleScan {
        let end = scroll.saturating_add(viewport_height).saturating_add(2);
        let (mut index, mut top) = self.first_visible_index(scroll);
        let mut visible = Vec::new();
        while index < self.cells.len() {
            if top >= end {
                return VisibleScan {
                    cells: visible,
                    reached_end: false,
                    total_height: top,
                };
            }
            let height = self.height(index);
            let bottom = top.saturating_add(height);
            if bottom > scroll && top < end {
                visible.push(VisibleCell {
                    index,
                    skip: scroll.saturating_sub(top),
                    height,
                });
            }
            top = bottom;
            self.set_prefix(index.saturating_add(1), top);
            index = index.saturating_add(1);
        }
        VisibleScan {
            cells: visible,
            reached_end: true,
            total_height: top,
        }
    }

    fn cell_top(&mut self, cell_id: &TranscriptCellId) -> Option<usize> {
        for index in 0..self.cells.len() {
            if self.cells[index]
                .cell_id(self.messages)
                .as_ref()
                .is_some_and(|id| id == cell_id)
            {
                return Some(self.prefix_top(index));
            }
        }
        None
    }

    fn total_height(&mut self) -> usize {
        self.prefix_top(self.cells.len())
    }

    fn first_visible_index(&mut self, scroll: usize) -> (usize, usize) {
        let mut index = 0usize;
        let mut top = 0usize;
        while index < self.cells.len() {
            let height = self.height(index);
            let bottom = top.saturating_add(height);
            self.set_prefix(index.saturating_add(1), bottom);
            if bottom > scroll {
                return (index, top);
            }
            index = index.saturating_add(1);
            top = bottom;
        }
        (self.cells.len(), top)
    }

    fn prefix_top(&mut self, index: usize) -> usize {
        let index = index.min(self.cells.len());
        if let Some(top) = self
            .layout_index
            .prefix
            .get(index)
            .and_then(|prefix| *prefix)
        {
            return top;
        }

        let mut start = index;
        while start > 0
            && self
                .layout_index
                .prefix
                .get(start)
                .is_none_or(Option::is_none)
        {
            start -= 1;
        }

        let mut top = self
            .layout_index
            .prefix
            .get(start)
            .and_then(|prefix| *prefix)
            .unwrap_or(0);
        for current in start..index {
            top = top.saturating_add(self.height(current));
            self.set_prefix(current.saturating_add(1), top);
        }
        top
    }

    fn set_prefix(&mut self, index: usize, top: usize) {
        if let Some(prefix) = self.layout_index.prefix.get_mut(index) {
            *prefix = Some(top);
        }
    }

    fn height(&mut self, index: usize) -> usize {
        let cell = &self.cells[index];
        let id = cell.cell_id(self.messages);
        let expanded = id
            .as_ref()
            .is_none_or(|id| !self.collapsed_cell_ids.contains(id));
        if matches!(cell, TranscriptSourceCell::Active(_)) {
            return self.renderer.desired_height(cell, expanded, false).max(1);
        }
        let Some(id) = id else {
            return self.renderer.desired_height(cell, expanded, false).max(1);
        };
        let key = TranscriptHeightCacheKey {
            cell_id: id,
            width: self.renderer.width,
            expanded,
        };
        if let Some(height) = self.layout_index.heights.get(&key).copied() {
            return height;
        }
        let height = self.renderer.desired_height(cell, expanded, false).max(1);
        self.layout_index.heights.insert(key, height);
        self.layout_index.invalidate_prefix_from(index);
        height
    }
}

fn signed_offset(base: usize, offset: i32) -> usize {
    if offset < 0 {
        base.saturating_sub(offset.unsigned_abs() as usize)
    } else {
        base.saturating_add(offset as usize)
    }
}

fn effective_scroll(
    scroll: &TranscriptScrollPosition,
    pager: &mut TranscriptPager<'_, '_, '_>,
    viewport_height: usize,
) -> usize {
    match scroll {
        TranscriptScrollPosition::Top => 0,
        TranscriptScrollPosition::Absolute(top) => *top,
        TranscriptScrollPosition::Anchor {
            cell_id,
            offset_rows,
        } => pager
            .cell_top(cell_id)
            .map(|top| signed_offset(top, *offset_rows))
            .unwrap_or(0),
        TranscriptScrollPosition::Tail { offset_from_bottom } => pager
            .total_height()
            .saturating_sub(viewport_height)
            .saturating_sub(*offset_from_bottom),
    }
}

fn transcript_layout_generation(state: &AppState) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash = mix_u64(hash, state.session.messages.len() as u64);
    if let Some(last) = state.session.messages.last() {
        hash = mix_str(hash, &last.id);
        hash = mix_u64(hash, message_content_len(&last.content) as u64);
    }
    hash = mix_u64(hash, state.session.tool_executions.len() as u64);
    for tool in &state.session.tool_executions {
        hash = mix_str(hash, &tool.call_id);
        hash = mix_u64(hash, tool.status as u64);
    }
    hash
}

fn transcript_prefix_generation(
    state: &AppState,
    cells: &[TranscriptSourceCell<'_>],
    width: u16,
    collapsed_cell_ids: &HashSet<TranscriptCellId>,
) -> u64 {
    let mut hash = transcript_layout_generation(state);
    hash = mix_u64(hash, u64::from(width));
    for cell in cells {
        let Some(id) = cell.cell_id(&state.session.messages) else {
            continue;
        };
        if collapsed_cell_ids.contains(&id) {
            hash = mix_u64(hash, 1);
            hash = mix_cell_id(hash, &id);
        }
    }
    hash
}

fn mix_cell_id(mut hash: u64, cell_id: &TranscriptCellId) -> u64 {
    match cell_id {
        TranscriptCellId::ToolCall { call_id } => {
            hash = mix_u64(hash, 1);
            mix_str(hash, call_id)
        }
        TranscriptCellId::Message { index, message_id } => {
            hash = mix_u64(hash, 2);
            hash = mix_u64(hash, *index as u64);
            mix_str(hash, message_id)
        }
        TranscriptCellId::ToolBatch { start, end } => {
            hash = mix_u64(hash, 3);
            hash = mix_u64(hash, *start as u64);
            mix_u64(hash, *end as u64)
        }
        TranscriptCellId::HookBatch { start, end } => {
            hash = mix_u64(hash, 4);
            hash = mix_u64(hash, *start as u64);
            mix_u64(hash, *end as u64)
        }
        TranscriptCellId::TaskNotificationBatch { start, end } => {
            hash = mix_u64(hash, 5);
            hash = mix_u64(hash, *start as u64);
            mix_u64(hash, *end as u64)
        }
        TranscriptCellId::ActiveTail => mix_u64(hash, 6),
    }
}

fn message_content_len(content: &MessageContent) -> usize {
    match content {
        MessageContent::Text(text)
        | MessageContent::AssistantText(text)
        | MessageContent::SystemText(text) => text.len(),
        MessageContent::Thinking { content, .. } => content.len(),
        MessageContent::ToolSuccess { output, .. } => output.len(),
        MessageContent::ToolError { error, .. } => error.len(),
        MessageContent::ToolRejected { reason, .. } => reason.len(),
        MessageContent::ToolCanceled { tool_name } => tool_name.len(),
        MessageContent::ToolUse {
            tool_name,
            call_id,
            input_preview,
            ..
        } => tool_name.len() + call_id.len() + input_preview.len(),
        _ => 0,
    }
}

fn mix_str(mut hash: u64, value: &str) -> u64 {
    for byte in value.bytes() {
        hash = mix_u64(hash, u64::from(byte));
    }
    hash
}

fn mix_u64(hash: u64, value: u64) -> u64 {
    hash.wrapping_mul(0x0000_0100_0000_01b3) ^ value
}

fn result_line(text: String, color: ratatui::style::Color) -> Line<'static> {
    output_result_line(text, color, true)
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
    let prefix = if first { "    └ " } else { "      " };
    Line::from(vec![Span::raw(prefix).fg(color), Span::raw(text).fg(color)])
}

fn single_line_capped(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, part) in text.split_whitespace().enumerate() {
        if index > 0 {
            push_capped(&mut out, " ", max_chars);
        }
        push_capped(&mut out, part, max_chars);
        if out.chars().count() >= max_chars {
            break;
        }
    }
    out
}

fn transcript_safe_line(line: &str) -> String {
    truncate_chars(line, TRANSCRIPT_LINE_CHAR_CAP)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

fn push_capped(out: &mut String, text: &str, max_chars: usize) {
    let remaining = max_chars.saturating_sub(out.chars().count());
    out.extend(text.chars().take(remaining));
}

fn wrapped_height(lines: &[Line<'static>], width: u16) -> usize {
    let width = usize::from(width).max(1);
    lines
        .iter()
        .map(|line| {
            let line_width = line
                .spans
                .iter()
                .map(|span| text_width(span.content.as_ref()))
                .sum::<usize>();
            line_width.saturating_add(width - 1) / width
        })
        .map(|rows| rows.max(1))
        .sum::<usize>()
        .max(1)
}
