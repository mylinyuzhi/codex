//! Exactly-once finalized-history emission tracking.
//!
//! Phase 3d (§4): the tracker keys off engine-message UUIDs (the
//! `message_uuid` on each `RenderedCell`). A `Message::Assistant` that
//! produces multiple cells (text + thinking + tool_use) is represented
//! by one entry — the tracker collapses repeated UUIDs so the
//! "previously emitted prefix" comparison works at engine-message
//! granularity rather than per-cell.
// S2 lands before production native scrollback wiring; keep this scoped while
// `terminal::Tui` still owns the live UI.
#![allow(dead_code)]

use uuid::Uuid;

use crate::state::transcript_view::RenderedCell;
use coco_tui_ui::engine::terminal::SurfaceBackend;
use coco_tui_ui::engine::terminal::SurfaceTerminal;
use ratatui::text::Line;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryEmissionPlan {
    Noop,
    Append { start: usize },
    ReplayRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryEmissionOutcome {
    Noop,
    Appended {
        start: usize,
        message_count: usize,
        rows: u16,
    },
    Replayed {
        message_count: usize,
        rows: u16,
    },
    ReplayRequired,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct HistoryEmissionTracker {
    /// UUIDs of engine messages already emitted, in append order. One
    /// entry per distinct `message_uuid` regardless of how many cells
    /// that engine message produced.
    emitted_message_uuids: Vec<Uuid>,
}

impl HistoryEmissionTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn emitted_count(&self) -> usize {
        self.emitted_message_uuids.len()
    }

    pub(crate) fn plan(&self, cells: &[RenderedCell]) -> HistoryEmissionPlan {
        let cell_starts = engine_message_cell_starts(cells);
        let total_messages = cell_starts.len();

        if self.emitted_message_uuids.is_empty() {
            return if cells.is_empty() {
                HistoryEmissionPlan::Noop
            } else {
                HistoryEmissionPlan::Append { start: 0 }
            };
        }

        if self.emitted_message_uuids.len() > total_messages {
            return HistoryEmissionPlan::ReplayRequired;
        }

        let prefix_matches = self
            .emitted_message_uuids
            .iter()
            .zip(cell_starts.iter())
            .all(|(emitted_uuid, &start_idx)| {
                cells
                    .get(start_idx)
                    .is_some_and(|cell| cell.message_uuid == *emitted_uuid)
            });
        if !prefix_matches {
            return HistoryEmissionPlan::ReplayRequired;
        }

        if self.emitted_message_uuids.len() == total_messages {
            HistoryEmissionPlan::Noop
        } else {
            HistoryEmissionPlan::Append {
                start: cell_starts[self.emitted_message_uuids.len()],
            }
        }
    }

    pub(crate) fn mark_emitted_through(&mut self, cells: &[RenderedCell], end: usize) {
        let end = end.min(cells.len());
        let mut uuids: Vec<Uuid> = Vec::new();
        let mut prev = None;
        for cell in cells.iter().take(end) {
            if Some(cell.message_uuid) != prev {
                uuids.push(cell.message_uuid);
                prev = Some(cell.message_uuid);
            }
        }
        self.emitted_message_uuids = uuids;
    }

    pub(crate) fn reset(&mut self) {
        self.emitted_message_uuids.clear();
    }

    pub(crate) fn emit_append_only<B, F>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        cells: &[RenderedCell],
        render_tail: F,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
        F: FnOnce(&[RenderedCell]) -> Vec<Line<'static>>,
    {
        let start = match self.plan(cells) {
            HistoryEmissionPlan::Noop => return Ok(HistoryEmissionOutcome::Noop),
            HistoryEmissionPlan::ReplayRequired => {
                return Ok(HistoryEmissionOutcome::ReplayRequired);
            }
            HistoryEmissionPlan::Append { start } => start,
        };

        let rows = terminal.insert_history_lines(render_tail(&cells[start..]))?;
        self.mark_emitted_through(cells, cells.len());
        Ok(HistoryEmissionOutcome::Appended {
            start,
            message_count: cells.len() - start,
            rows,
        })
    }

    pub(crate) fn replay_all<B, F>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        cells: &[RenderedCell],
        render_all: F,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
        F: FnOnce(&[RenderedCell]) -> Vec<Line<'static>>,
    {
        terminal.clear_owned_scrollback()?;
        let rows = terminal.insert_history_lines(render_all(cells))?;
        self.mark_emitted_through(cells, cells.len());
        Ok(HistoryEmissionOutcome::Replayed {
            message_count: cells.len(),
            rows,
        })
    }
}

/// Indices where each distinct engine-message UUID starts within
/// `cells`. Multiple cells sharing a UUID (assistant turn fanout)
/// collapse to one entry.
fn engine_message_cell_starts(cells: &[RenderedCell]) -> Vec<usize> {
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

#[cfg(test)]
#[path = "history_emitter.test.rs"]
mod tests;
