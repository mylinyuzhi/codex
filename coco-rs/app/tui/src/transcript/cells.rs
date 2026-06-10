//! Tool-commit boundary for the native finalize — the transcript-owner half of
//! `docs/coco-rs/ui/tui-v2-design.md` §6.4 / §6.5.
//!
//! Rows are immutable once they enter native scrollback, so the finalize may
//! only commit a leading cell prefix in which every `ToolUse` already pairs
//! with its forward `ToolResult`. `committable_prefix_len` computes that bound:
//! an unresolved `ToolUse` blocks the prefix until its result arrives; orphan
//! results don't block; duplicate call ids each need their own result.
//!
//! The canonical `Message` → `RenderedCell` derivation currently lives in
//! `state::derive` / `state::transcript_view` (the cell model is state-owned);
//! the ordering + batch/pairing projection lives in
//! `presentation::transcript`. Re-homing the projection here is deferred
//! until the cell model itself moves (v2 Stage 2+), so this module never
//! holds an unused re-export and `state` keeps no dependency back into the
//! transcript renderer.

use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;

/// Indices into `cells` where each engine message begins — one entry per
/// `message_uuid` run (an assistant turn's fanout cells share their first
/// index). The shared grouping primitive for emission counting and replay
/// truncation.
pub(crate) fn engine_message_starts(cells: &[RenderedCell]) -> impl Iterator<Item = usize> + '_ {
    let mut prev = None;
    cells.iter().enumerate().filter_map(move |(index, cell)| {
        if Some(cell.message_uuid) != prev {
            prev = Some(cell.message_uuid);
            Some(index)
        } else {
            None
        }
    })
}

/// Length of the leading cell prefix that is safe to insert into native
/// scrollback: every `ToolUse` in the prefix must pair with a forward,
/// not-yet-consumed `ToolResult`. The first `ToolUse` without one truncates
/// the prefix at its engine-message start — the whole message stays live in
/// the viewport until the tool resolves.
pub(crate) fn committable_prefix_len(cells: &[RenderedCell]) -> usize {
    let mut consumed_results = vec![false; cells.len()];
    for (index, cell) in cells.iter().enumerate() {
        if let CellKind::ToolUse { call_id, .. } = &cell.kind {
            let Some(result_index) =
                find_forward_unconsumed_tool_result(cells, &consumed_results, index + 1, call_id)
            else {
                return engine_message_start_containing(cells, index);
            };
            consumed_results[result_index] = true;
        }
    }
    cells.len()
}

fn find_forward_unconsumed_tool_result(
    cells: &[RenderedCell],
    consumed_results: &[bool],
    start: usize,
    call_id: &str,
) -> Option<usize> {
    for (index, cell) in cells.iter().enumerate().skip(start) {
        if consumed_results[index] {
            continue;
        }
        if let CellKind::ToolResult {
            call_id: result_call_id,
        } = &cell.kind
            && result_call_id == call_id
        {
            return Some(index);
        }
    }
    None
}

/// First index of the engine-message run containing `index` (backward scan
/// over the `message_uuid` group).
fn engine_message_start_containing(cells: &[RenderedCell], index: usize) -> usize {
    let uuid = cells[index].message_uuid;
    let mut start = index;
    while start > 0 && cells[start - 1].message_uuid == uuid {
        start -= 1;
    }
    start
}
