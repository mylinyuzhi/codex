//! Finalized transcript rendering for native history emission.
//!
//! Phase 3d (§4): consumes the engine-authoritative `&[RenderedCell]`
//! slice from `session.transcript.cells()`. The "messages omitted"
//! counter in [`HistoryReplayLines`] still names messages because
//! truncation occurs at engine-message (not cell) boundaries — a
//! single `Message::Assistant` with text + thinking + tool_use blocks
//! contributes one increment, never three.
// S2 adapter: this initially reuses the existing chat renderer in committed-only
// mode while the native history cell renderer is carved out.
#![allow(dead_code)]

use ratatui::text::Line;

use crate::display_settings::SyntaxHighlighting;
use crate::keybinding_resolver::KeybindingHandle;
use crate::presentation::styles::UiStyles;
use crate::state::transcript_view::RenderedCell;
use crate::widgets::ChatWidget;

pub(crate) const DEFAULT_MAX_REFLOW_ROWS: usize = 9_000;

#[derive(Debug, Clone, Copy)]
pub(crate) struct HistoryLineRenderOptions<'a> {
    pub(crate) styles: UiStyles<'a>,
    pub(crate) width: u16,
    pub(crate) syntax_highlighting: SyntaxHighlighting,
    pub(crate) show_system_reminders: bool,
    pub(crate) show_thinking: bool,
    pub(crate) kb_handle: Option<&'a KeybindingHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryReplayLines {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) omitted_messages: usize,
}

pub(crate) fn render_finalized_history_lines(
    cells: &[RenderedCell],
    options: HistoryLineRenderOptions<'_>,
) -> Vec<Line<'static>> {
    let mut chat = ChatWidget::new(cells, options.styles)
        .show_system_reminders(options.show_system_reminders)
        .show_thinking(options.show_thinking)
        .width(options.width)
        .syntax_highlighting(options.syntax_highlighting);
    if let Some(kb_handle) = options.kb_handle {
        chat = chat.kb_handle(kb_handle);
    }
    chat.build_lines_owned()
}

pub(crate) fn render_replay_history_lines(
    cells: &[RenderedCell],
    options: HistoryLineRenderOptions<'_>,
    max_rows: usize,
) -> HistoryReplayLines {
    let all_lines = render_finalized_history_lines(cells, options);
    if all_lines.len() <= max_rows || cells.is_empty() {
        return HistoryReplayLines {
            lines: all_lines,
            omitted_messages: 0,
        };
    }

    // Walk forward by engine-message UUID boundaries so the "N older
    // messages omitted" marker counts engine messages, not cells.
    let message_starts = engine_message_starts(cells);
    for (i, &start) in message_starts.iter().enumerate().skip(1) {
        let omitted_messages = i;
        let mut lines = replay_truncation_marker(omitted_messages);
        lines.extend(render_finalized_history_lines(&cells[start..], options));
        if lines.len() <= max_rows {
            return HistoryReplayLines {
                lines,
                omitted_messages,
            };
        }
    }

    HistoryReplayLines {
        lines: replay_truncation_marker(message_starts.len()),
        omitted_messages: message_starts.len(),
    }
}

/// Indices into `cells` where each engine message begins. Multiple
/// cells with the same `message_uuid` (assistant turn fanout) share an
/// entry — the index of the first cell in that group.
fn engine_message_starts(cells: &[RenderedCell]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut prev = None;
    for (i, cell) in cells.iter().enumerate() {
        if Some(cell.message_uuid) != prev {
            starts.push(i);
            prev = Some(cell.message_uuid);
        }
    }
    starts
}

fn replay_truncation_marker(omitted_messages: usize) -> Vec<Line<'static>> {
    vec![
        Line::from(format!(
            "... {omitted_messages} older messages retained in transcript, not replayed"
        )),
        Line::from("    open transcript pager for full history"),
        Line::default(),
    ]
}

#[cfg(test)]
#[path = "history_lines.test.rs"]
mod tests;
