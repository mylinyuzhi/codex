//! Exactly-once finalized-history emission tracking.
//!
//! Phase 3d (§4): the tracker keys off engine-message UUIDs (the
//! `message_uuid` on each `RenderedCell`). A `Message::Assistant` that
//! produces multiple cells (text + thinking + tool_use) is represented
//! by one entry — the tracker collapses repeated UUIDs so the
//! "previously emitted prefix" comparison works at engine-message
//! granularity rather than per-cell.
use uuid::Uuid;

use crate::state::transcript_view::RenderedCell;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryEmissionPlan {
    Noop,
    Append { start: usize },
    ReplayRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryEmissionOutcome {
    Noop,
    FastNoop {
        revision: u64,
    },
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
    pub(crate) fn emitted_count(&self) -> usize {
        self.emitted_message_uuids.len()
    }

    pub(crate) fn plan(&self, cells: &[RenderedCell]) -> HistoryEmissionPlan {
        let total_messages = engine_message_count(cells);

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

        let mut emitted_index = 0usize;
        let mut next_start = None;
        let mut prev = None;
        let mut prefix_matches = true;
        for (cell_index, cell) in cells.iter().enumerate() {
            if Some(cell.message_uuid) == prev {
                continue;
            }
            prev = Some(cell.message_uuid);
            if let Some(emitted_uuid) = self.emitted_message_uuids.get(emitted_index) {
                if *emitted_uuid != cell.message_uuid {
                    prefix_matches = false;
                    break;
                }
                emitted_index += 1;
            } else {
                next_start = Some(cell_index);
                break;
            }
        }
        if !prefix_matches {
            return HistoryEmissionPlan::ReplayRequired;
        }

        if self.emitted_message_uuids.len() == total_messages {
            HistoryEmissionPlan::Noop
        } else {
            HistoryEmissionPlan::Append {
                start: next_start.unwrap_or(cells.len()),
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

    #[cfg(test)]
    pub(crate) fn mark_appended_from(&mut self, cells: &[RenderedCell], start: usize) {
        let mut prev = cells
            .get(start.wrapping_sub(1))
            .map(|cell| cell.message_uuid);
        for cell in cells.iter().skip(start) {
            if Some(cell.message_uuid) != prev {
                self.emitted_message_uuids.push(cell.message_uuid);
                prev = Some(cell.message_uuid);
            }
        }
    }

    pub(crate) fn reset(&mut self) {
        self.emitted_message_uuids.clear();
    }
}

#[cfg(test)]
impl HistoryEmissionTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn emit_append_only<B, F>(
        &mut self,
        terminal: &mut coco_tui_ui::engine::terminal::SurfaceTerminal<B>,
        cells: &[RenderedCell],
        render_tail: F,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: coco_tui_ui::engine::terminal::SurfaceBackend,
        F: FnOnce(&[RenderedCell]) -> Vec<ratatui::text::Line<'static>>,
    {
        let start = match self.plan(cells) {
            HistoryEmissionPlan::Noop => return Ok(HistoryEmissionOutcome::Noop),
            HistoryEmissionPlan::ReplayRequired => {
                return Ok(HistoryEmissionOutcome::ReplayRequired);
            }
            HistoryEmissionPlan::Append { start } => start,
        };

        let rendered = coco_tui_ui::engine::history_insert::render_history_rows(
            render_tail(&cells[start..]),
            terminal.viewport_area().width,
        );
        let rows = terminal.insert_history_rows(&rendered)?;
        self.mark_emitted_through(cells, cells.len());
        Ok(HistoryEmissionOutcome::Appended {
            start,
            message_count: cells.len() - start,
            rows,
        })
    }

    pub(crate) fn replay_all<B, F>(
        &mut self,
        terminal: &mut coco_tui_ui::engine::terminal::SurfaceTerminal<B>,
        cells: &[RenderedCell],
        render_all: F,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: coco_tui_ui::engine::terminal::SurfaceBackend,
        F: FnOnce(&[RenderedCell]) -> Vec<ratatui::text::Line<'static>>,
    {
        terminal.clear_owned_scrollback()?;
        let rendered = coco_tui_ui::engine::history_insert::render_history_rows(
            render_all(cells),
            terminal.viewport_area().width,
        );
        let rows = terminal.insert_history_rows(&rendered)?;
        self.mark_emitted_through(cells, cells.len());
        Ok(HistoryEmissionOutcome::Replayed {
            message_count: cells.len(),
            rows,
        })
    }
}

fn engine_message_count(cells: &[RenderedCell]) -> usize {
    let mut count = 0usize;
    let mut prev = None;
    for cell in cells {
        if Some(cell.message_uuid) != prev {
            count += 1;
            prev = Some(cell.message_uuid);
        }
    }
    count
}

#[cfg(test)]
#[path = "history_emitter.test.rs"]
mod tests;
