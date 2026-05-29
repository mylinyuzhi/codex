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

use std::collections::HashMap;

use ratatui::text::Line;

use crate::keybinding_resolver::KeybindingHandle;
use crate::state::session::ReasoningMetadata;
use crate::state::transcript_view::RenderedCell;
use crate::widgets::ChatWidget;
use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;

pub(crate) const DEFAULT_MAX_REFLOW_ROWS: usize = 9_000;

#[derive(Debug, Clone, Copy)]
pub(crate) struct HistoryLineRenderOptions<'a> {
    pub(crate) styles: UiStyles<'a>,
    pub(crate) width: u16,
    pub(crate) syntax_highlighting: SyntaxHighlighting,
    pub(crate) show_system_reminders: bool,
    pub(crate) show_thinking: bool,
    pub(crate) kb_handle: Option<&'a KeybindingHandle>,
    /// TUI-side side-cache for reasoning metadata keyed by assistant
    /// message UUID. `None` ⇒ thinking cells render without the
    /// `· <duration> · <tokens>` badge (live append before
    /// `TurnCompleted` arrives).
    pub(crate) reasoning_metadata: Option<&'a HashMap<uuid::Uuid, ReasoningMetadata>>,
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
    if let Some(meta) = options.reasoning_metadata {
        chat = chat.reasoning_metadata(meta);
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

    // Truncate at engine-message UUID boundaries so the "N older messages
    // omitted" marker counts engine messages, not cells.
    //
    // Dropping more leading messages can only shrink the rendered suffix, so
    // "suffix + marker fits within max_rows" is monotonic in the number of
    // omitted messages. Binary-search the smallest omission that fits rather
    // than re-rendering every candidate suffix forward — the old linear walk
    // re-wrapped the whole remaining transcript on each step (O(messages ×
    // cells)); this is O(messages × cells × log messages) and renders the
    // chosen suffix at most a handful of times.
    let message_starts = engine_message_starts(cells);
    let marker_rows = replay_truncation_marker(0).len();
    let n = message_starts.len();

    let fits = |omitted: usize| -> bool {
        let start = message_starts[omitted];
        marker_rows + render_finalized_history_lines(&cells[start..], options).len() <= max_rows
    };

    // Smallest `omitted` in `1..n` whose suffix fits; `n` ⇒ none fits.
    let mut lo = 1;
    let mut hi = n;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if fits(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }

    if lo < n {
        let start = message_starts[lo];
        let mut lines = replay_truncation_marker(lo);
        lines.extend(render_finalized_history_lines(&cells[start..], options));
        HistoryReplayLines {
            lines,
            omitted_messages: lo,
        }
    } else {
        // Even keeping only the final message overflows the cap; emit just
        // the marker (matches the prior fallback behaviour).
        HistoryReplayLines {
            lines: replay_truncation_marker(n),
            omitted_messages: n,
        }
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
