//! Surface history orchestration for native scrollback.
//!
//! Phase 3d (§4): operates on `&[RenderedCell]` directly. The
//! `HistoryEmissionTracker` still tracks exactly-once emission by
//! engine message UUIDs, which are stable across the engine
//! `MessageAppended` events and survive resume reloads (each cell
//! carries `Arc<Message>` from the engine `MessageHistory`).
use std::time::Duration;
use std::time::Instant;

use ratatui::text::Line;

use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;
use crate::streaming::render_controller::StreamRenderInput;
use crate::streaming::render_controller::StreamRenderKey;
use crate::surface::history_emitter::HistoryEmissionOutcome;
use crate::surface::history_emitter::HistoryEmissionPlan;
use crate::surface::history_emitter::HistoryEmissionTracker;
use crate::surface::history_lines::DEFAULT_MAX_REFLOW_ROWS;
use crate::surface::history_lines::HistoryLineRenderOptions;
use crate::surface::history_lines::HistoryReplayCache;
use crate::surface::history_lines::render_finalized_history_lines;
use crate::surface::history_lines::render_replay_history_lines_cached;
use crate::surface::line_fingerprint::RenderedLineFingerprint;
use crate::surface::line_fingerprint::fingerprint_lines;
use crate::surface::stream::PendingStreamPrefix;
use crate::surface::stream::PreparedStreamAppend;
use crate::widgets::chat::render_assistant::CommittedAssistantMarkdownOptions;
use crate::widgets::chat::render_assistant::render_committed_assistant_markdown;
use coco_tui_ui::engine::history_insert::HistoryRows;
use coco_tui_ui::engine::history_insert::HistoryRowsSlice;
use coco_tui_ui::engine::history_insert::render_history_rows;
use coco_tui_ui::engine::history_reflow::HistoryReflowState;
use coco_tui_ui::engine::history_reflow::HistoryViewportChange;
use coco_tui_ui::engine::terminal::SurfaceBackend;
use coco_tui_ui::engine::terminal::SurfaceTerminal;

const HISTORY_TAIL_CACHE_MAX_ROWS: u16 = 128;

#[derive(Debug, Default, Clone)]
pub(crate) struct SurfaceHistoryDriver {
    emitter: HistoryEmissionTracker,
    reflow: HistoryReflowState,
    header_fingerprint: Option<Vec<RenderedLineFingerprint>>,
    emitted_transcript_revision: Option<u64>,
    emitted_history_rows: u16,
    replay_cache: HistoryReplayCache,
    tail_cache: HistoryTailCache,
    pending_stream_prefix: Option<PendingStreamPrefix>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HistoryReplayMode {
    pub(crate) stream_active: bool,
    pub(crate) cause: &'static str,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HistoryTailCacheStats {
    pub(crate) rows: u16,
    pub(crate) width: Option<u16>,
    pub(crate) bytes_estimate: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum PreparedFinalizedHistory {
    Noop { transcript_revision: u64 },
    FastNoop { revision: u64 },
    Append(PreparedHistoryAppend),
    ReplayRequired,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedHistoryAppend {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) message_count: usize,
    pub(crate) transcript_revision: u64,
    pub(crate) header_fingerprint: Vec<RenderedLineFingerprint>,
    pub(crate) emitted_header: bool,
    pub(crate) rows: HistoryRows,
    /// Time spent building the candidate `Line`s (cell rendering — markdown,
    /// diffs, syntax highlighting, stream-prefix verification).
    pub(crate) lines_build_elapsed: Duration,
    /// Time spent rendering those lines into terminal rows.
    pub(crate) render_elapsed: Duration,
    clear_pending_stream_prefix: bool,
}

impl PreparedFinalizedHistory {
    pub(crate) fn expected_rows(&self) -> u16 {
        match self {
            Self::Append(append) => append.rows.height(),
            Self::Noop { .. } | Self::FastNoop { .. } | Self::ReplayRequired => 0,
        }
    }

    pub(crate) fn render_elapsed(&self) -> Duration {
        match self {
            Self::Append(append) => append.render_elapsed,
            Self::Noop { .. } | Self::FastNoop { .. } | Self::ReplayRequired => Duration::default(),
        }
    }

    pub(crate) fn lines_build_elapsed(&self) -> Duration {
        match self {
            Self::Append(append) => append.lines_build_elapsed,
            Self::Noop { .. } | Self::FastNoop { .. } | Self::ReplayRequired => Duration::default(),
        }
    }
}

impl SurfaceHistoryDriver {
    pub(crate) fn note_viewport(
        &mut self,
        width: u16,
        stream_active: bool,
    ) -> HistoryViewportChange {
        let change = self.reflow.note_viewport(width);
        if self.tail_cache.width != Some(width) {
            self.tail_cache.clear();
        }
        if change.changed && self.reflow.replay_needed_for_viewport(width) {
            self.reflow.schedule_viewport_replay(width, stream_active);
        }
        change
    }

    pub(crate) fn replay_due(&self, now: Instant) -> bool {
        self.reflow.pending_is_due(now)
    }

    pub(crate) fn prepare_append(
        &self,
        session_header: Vec<Line<'static>>,
        cells: &[RenderedCell],
        transcript_revision: u64,
        options: HistoryLineRenderOptions<'_>,
    ) -> PreparedFinalizedHistory {
        let end = committable_prefix_len(cells);
        let committable_cells = &cells[..end];
        let header_fingerprint = fingerprint_lines(&session_header);
        if self
            .header_fingerprint
            .as_ref()
            .is_some_and(|emitted| emitted != &header_fingerprint)
        {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "header_fingerprint_changed",
                cells = committable_cells.len(),
                cells_total = cells.len(),
                emitted_messages = self.emitter.emitted_count(),
                "history full replay required",
            );
            return PreparedFinalizedHistory::ReplayRequired;
        }

        let should_emit_header = self.header_fingerprint.is_none();
        if !should_emit_header && self.emitted_transcript_revision == Some(transcript_revision) {
            return PreparedFinalizedHistory::FastNoop {
                revision: transcript_revision,
            };
        }

        let plan = self.emitter.plan(committable_cells);
        if matches!(plan, HistoryEmissionPlan::Noop) && !should_emit_header {
            return PreparedFinalizedHistory::Noop {
                transcript_revision,
            };
        }
        if matches!(plan, HistoryEmissionPlan::ReplayRequired) {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "emitter_uuid_prefix_mismatch",
                cells = committable_cells.len(),
                cells_total = cells.len(),
                emitted_messages = self.emitter.emitted_count(),
                "history full replay required",
            );
            return PreparedFinalizedHistory::ReplayRequired;
        }

        let start = match plan {
            HistoryEmissionPlan::Append { start } => start,
            HistoryEmissionPlan::Noop | HistoryEmissionPlan::ReplayRequired => end,
        };
        let lines_build_started = Instant::now();
        let Some(lines) = self.append_candidate_lines_from_plan(
            session_header,
            cells,
            start,
            end,
            should_emit_header,
            options,
        ) else {
            return PreparedFinalizedHistory::ReplayRequired;
        };
        let lines_build_elapsed = lines_build_started.elapsed();
        let render_started = Instant::now();
        let rows = render_history_rows(lines, options.width);
        PreparedFinalizedHistory::Append(PreparedHistoryAppend {
            start,
            end,
            message_count: end - start,
            transcript_revision,
            header_fingerprint,
            emitted_header: should_emit_header,
            rows,
            lines_build_elapsed,
            render_elapsed: render_started.elapsed(),
            clear_pending_stream_prefix: self.pending_stream_prefix.is_some(),
        })
    }

    pub(crate) fn tail_reveal_rows(&self, width: u16) -> u16 {
        self.tail_cache.available_rows(width)
    }

    pub(crate) fn tail_cache_stats(&self) -> HistoryTailCacheStats {
        self.tail_cache.stats()
    }

    pub(crate) fn fill_tail_gap<B>(
        &self,
        terminal: &mut SurfaceTerminal<B>,
        rows: u16,
    ) -> Result<u16, B::Error>
    where
        B: SurfaceBackend,
    {
        let Some(slice) = self.tail_cache.tail_slice(rows) else {
            return Ok(0);
        };
        terminal.fill_history_gap_rows(slice)
    }

    pub(crate) fn commit_prepared_append<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        prepared: &PreparedFinalizedHistory,
        cells: &[RenderedCell],
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        match prepared {
            PreparedFinalizedHistory::Noop {
                transcript_revision,
            } => {
                self.emitted_transcript_revision = Some(*transcript_revision);
                Ok(HistoryEmissionOutcome::Noop)
            }
            PreparedFinalizedHistory::FastNoop { revision } => {
                Ok(HistoryEmissionOutcome::FastNoop {
                    revision: *revision,
                })
            }
            PreparedFinalizedHistory::ReplayRequired => Ok(HistoryEmissionOutcome::ReplayRequired),
            PreparedFinalizedHistory::Append(append) => {
                let width = terminal.viewport_area().width;
                let rows = terminal.insert_history_rows(&append.rows)?;
                self.emitted_history_rows = self.emitted_history_rows.saturating_add(rows);
                self.tail_cache.extend_from_rows(width, &append.rows);
                self.header_fingerprint = Some(append.header_fingerprint.clone());
                self.emitter.mark_emitted_through(cells, append.end);
                if append.clear_pending_stream_prefix {
                    self.pending_stream_prefix = None;
                }
                self.emitted_transcript_revision = Some(append.transcript_revision);
                tracing::trace!(
                    target: "tui::surface::append",
                    start = append.start,
                    end = append.end,
                    cells_total = cells.len(),
                    message_count = append.message_count,
                    rows,
                    emitted_header = append.emitted_header,
                    lines_build_elapsed_us = append.lines_build_elapsed.as_micros(),
                    render_elapsed_us = append.render_elapsed.as_micros(),
                    "history incremental append",
                );
                Ok(HistoryEmissionOutcome::Appended {
                    start: append.start,
                    message_count: append.message_count,
                    rows,
                })
            }
        }
    }

    pub(crate) fn commit_stream_append<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        append: &PreparedStreamAppend,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        let width = terminal.viewport_area().width;
        let rows = terminal.insert_history_rows(&append.rows)?;
        if rows == 0 {
            return Ok(HistoryEmissionOutcome::Noop);
        }
        self.emitted_history_rows = self.emitted_history_rows.saturating_add(rows);
        self.tail_cache.extend_from_rows(width, &append.rows);
        self.pending_stream_prefix = Some(append.prefix.clone());
        tracing::trace!(
            target: "tui::surface::append",
            source_prefix_len = append.prefix.source_prefix_len,
            line_prefix_len = append.prefix.line_prefix_len,
            rows,
            "history stream prefix append",
        );
        Ok(HistoryEmissionOutcome::Appended {
            start: 0,
            message_count: 0,
            rows,
        })
    }

    pub(crate) fn clear_pending_stream_prefix(&mut self) {
        self.pending_stream_prefix = None;
    }

    #[cfg(test)]
    pub(crate) fn emit_append_only<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        session_header: Vec<Line<'static>>,
        cells: &[RenderedCell],
        transcript_revision: u64,
        options: HistoryLineRenderOptions<'_>,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        let prepared = self.prepare_append(session_header, cells, transcript_revision, options);
        self.commit_prepared_append(terminal, &prepared, cells)
    }

    pub(crate) fn replay_all_capped<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        session_header: Vec<Line<'static>>,
        cells: &[RenderedCell],
        transcript_revision: u64,
        options: HistoryLineRenderOptions<'_>,
        mode: HistoryReplayMode,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        let end = committable_prefix_len(cells);
        let committable_cells = &cells[..end];
        let started = Instant::now();
        let replay = render_replay_history_lines_cached(
            committable_cells,
            options,
            DEFAULT_MAX_REFLOW_ROWS,
            &mut self.replay_cache,
        );
        let render_elapsed = started.elapsed();
        let line_count = replay.lines.len();
        let outcome = self.replay_rows(
            terminal,
            session_header,
            committable_cells,
            transcript_revision,
            &replay.rows,
        )?;
        if replay.omitted_messages > 0 {
            tracing::info!(
                target: "tui::surface::replay",
                cause = mode.cause,
                max_rows = DEFAULT_MAX_REFLOW_ROWS,
                omitted_messages = replay.omitted_messages,
                rows = replay.rows.height(),
                lines = line_count,
                "history replay capped older messages",
            );
        }
        let area = terminal.viewport_area();
        self.reflow
            .mark_replayed_viewport(area.width, mode.stream_active);
        tracing::debug!(
            target: "tui::surface::replay",
            cause = mode.cause,
            render_elapsed_ms = render_elapsed.as_millis(),
            cells = committable_cells.len(),
            cells_total = cells.len(),
            cells_rendered = replay.stats.cells_rendered,
            finalized_render_calls = replay.stats.finalized_render_calls,
            lines = line_count,
            omitted_messages = replay.omitted_messages,
            cache_hit = replay.stats.cache_hit,
            cacheable = replay.stats.cacheable,
            cache_lookup = ?replay.stats.cache_lookup,
            cache_skip_reason = ?replay.stats.cache_skip_reason,
            cache_admitted = replay.stats.cache_admitted,
            key_build_elapsed_us = replay.stats.key_build_elapsed_us,
            cache_entries = replay.stats.cache_entries,
            cache_estimated_bytes = replay.stats.cache_estimated_bytes,
            cell_content_estimated_bytes = replay.stats.cell_content_estimated_bytes,
            replay_estimated_bytes = replay.stats.replay_estimated_bytes,
            cache_evictions = replay.stats.cache_evictions,
            width = area.width,
            rows = replay.rows.height(),
            height = area.height,
            stream_active = mode.stream_active,
            "history replay render completed",
        );
        Ok(outcome)
    }

    pub(crate) fn reset(&mut self) {
        self.emitter.reset();
        self.header_fingerprint = None;
        self.emitted_transcript_revision = None;
        self.emitted_history_rows = 0;
        self.reflow.clear();
        self.replay_cache.clear();
        self.tail_cache.clear();
        self.pending_stream_prefix = None;
    }

    fn replay_rows<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        session_header: Vec<Line<'static>>,
        cells: &[RenderedCell],
        transcript_revision: u64,
        message_rows: &HistoryRows,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        let header_fingerprint = fingerprint_lines(&session_header);
        let viewport_area = terminal.viewport_area();
        terminal.clear_owned_scrollback()?;
        let width = viewport_area.width;
        let header_rows = render_history_rows(session_header, width);
        let Some(rendered) = HistoryRows::try_copy_tail_from_slices(
            width,
            &[
                header_rows.tail_slice(u16::MAX),
                message_rows.tail_slice(u16::MAX),
            ],
            u16::MAX,
        ) else {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "replay_rows_width_mismatch",
                header_width = header_rows.width(),
                message_width = message_rows.width(),
                width,
                "history replay row concat failed",
            );
            return Ok(HistoryEmissionOutcome::ReplayRequired);
        };
        // No viewport reseat after insert: `clear_owned_scrollback` zeroed the
        // viewport and `insert_history_rows` flowed it back down to the freshly
        // inserted history bottom (or pinned it at the screen bottom on
        // overflow). That seat is already correct — the old clamp/restore only
        // existed to undo `sync_surface_area`'s stale-anchor reposition, which
        // no longer happens (it anchors on the owned viewport top).
        let rows = terminal.insert_history_rows(&rendered)?;
        self.emitted_history_rows = rows;
        self.tail_cache.replace_from_rows(width, &rendered);
        self.header_fingerprint = Some(header_fingerprint);
        self.emitter.mark_emitted_through(cells, cells.len());
        self.emitted_transcript_revision = Some(transcript_revision);
        self.pending_stream_prefix = None;
        tracing::debug!(
            target: "tui::surface::replay",
            message_count = cells.len(),
            rows,
            "history full replay completed",
        );
        Ok(HistoryEmissionOutcome::Replayed {
            message_count: cells.len(),
            rows,
        })
    }

    fn append_candidate_lines_from_plan(
        &self,
        session_header: Vec<Line<'static>>,
        cells: &[RenderedCell],
        start: usize,
        end: usize,
        should_emit_header: bool,
        options: HistoryLineRenderOptions<'_>,
    ) -> Option<Vec<Line<'static>>> {
        if let Some(pending) = self.pending_stream_prefix.as_ref() {
            return self.append_candidate_lines_after_stream_prefix(
                cells,
                start,
                end,
                should_emit_header,
                options,
                pending,
            );
        }
        let mut lines = Vec::new();
        if should_emit_header {
            lines.extend(session_header);
        }
        lines.extend(render_finalized_history_lines(&cells[start..end], options));
        Some(lines)
    }

    fn append_candidate_lines_after_stream_prefix(
        &self,
        cells: &[RenderedCell],
        start: usize,
        end: usize,
        should_emit_header: bool,
        options: HistoryLineRenderOptions<'_>,
        pending: &PendingStreamPrefix,
    ) -> Option<Vec<Line<'static>>> {
        if should_emit_header {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "pending_stream_prefix_without_header",
                "history full replay required",
            );
            return None;
        }
        // Mirror `push_text_first_assistant_group`: a message's leading
        // thinking cells render AFTER its text under the native presentation,
        // so the streamed text rows already in scrollback are the leading rows
        // of the group. Skip the same-message leading thinking run to find the
        // text cell the pending prefix anchors to; the thinking cells join the
        // suffix below, after the remaining text rows.
        let first = cells.get(start)?;
        let group_uuid = first.message_uuid;
        let mut text_idx = start;
        while text_idx < end
            && cells[text_idx].message_uuid == group_uuid
            && matches!(
                cells[text_idx].kind,
                CellKind::AssistantThinking { .. } | CellKind::AssistantRedactedThinking
            )
        {
            text_idx += 1;
        }
        let Some(text_cell) = cells.get(text_idx) else {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "pending_stream_prefix_next_cell_not_assistant_text",
                start,
                text_idx,
                "history full replay required",
            );
            return None;
        };
        let CellKind::AssistantText { text, .. } = &text_cell.kind else {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "pending_stream_prefix_next_cell_not_assistant_text",
                start,
                text_idx,
                "history full replay required",
            );
            return None;
        };
        // A thinking run whose text belongs to a different message is a
        // thinking-only group — the presentation renders it unreordered, so
        // the streamed text rows cannot be the group's leading rows.
        if text_idx > start && text_cell.message_uuid != group_uuid {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "pending_stream_prefix_thinking_without_same_message_text",
                start,
                text_idx,
                "history full replay required",
            );
            return None;
        }
        if !text.starts_with(&pending.source_prefix)
            || pending.source_prefix_len != pending.source_prefix.len()
        {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "pending_stream_prefix_source_mismatch",
                start,
                pending_source_prefix_len = pending.source_prefix_len,
                text_len = text.len(),
                "history full replay required",
            );
            return None;
        }
        let render_key = StreamRenderKey::committed(StreamRenderInput {
            source: "",
            styles: options.styles,
            width: options.width,
            syntax_highlighting: options.syntax_highlighting,
        });
        if render_key != pending.render_key {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "pending_stream_prefix_render_key_mismatch",
                "history full replay required",
            );
            return None;
        }

        let markdown_started = Instant::now();
        let assistant_lines = render_committed_assistant_markdown(
            text,
            CommittedAssistantMarkdownOptions {
                styles: options.styles,
                width: options.width,
                syntax_highlighting: options.syntax_highlighting,
            },
        );
        let markdown_elapsed = markdown_started.elapsed();
        if pending.line_prefix_len > assistant_lines.len() {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "pending_stream_prefix_line_len_exceeds_final",
                pending_line_prefix_len = pending.line_prefix_len,
                final_lines = assistant_lines.len(),
                "history full replay required",
            );
            return None;
        }
        let fingerprint_started = Instant::now();
        let fingerprint_matches = fingerprint_lines(&assistant_lines[..pending.line_prefix_len])
            == pending.line_fingerprints;
        tracing::debug!(
            target: "tui::streaming",
            text_bytes = text.len(),
            final_lines = assistant_lines.len(),
            prefix_lines = pending.line_prefix_len,
            markdown_us = markdown_elapsed.as_micros(),
            fingerprint_us = fingerprint_started.elapsed().as_micros(),
            matched = fingerprint_matches,
            "stream prefix finalize verification",
        );
        if !fingerprint_matches {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "pending_stream_prefix_rows_mismatch",
                "history full replay required",
            );
            return None;
        }

        let mut lines = assistant_lines[pending.line_prefix_len..].to_vec();
        lines.push(Line::default());
        // Presentation order: the message's leading thinking cells render
        // after its text, then everything past the text cell (tool calls,
        // following messages) renders normally.
        if start < text_idx {
            lines.extend(render_finalized_history_lines(
                &cells[start..text_idx],
                options,
            ));
        }
        if text_idx + 1 < end {
            lines.extend(render_finalized_history_lines(
                &cells[text_idx + 1..end],
                options,
            ));
        }
        Some(lines)
    }
}

fn committable_prefix_len(cells: &[RenderedCell]) -> usize {
    let mut consumed_results = vec![false; cells.len()];
    for (index, cell) in cells.iter().enumerate() {
        if let CellKind::ToolUse { call_id, .. } = &cell.kind {
            let Some(result_index) =
                find_forward_unconsumed_tool_result(cells, &consumed_results, index + 1, call_id)
            else {
                return engine_message_start(cells, index);
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

fn engine_message_start(cells: &[RenderedCell], index: usize) -> usize {
    let uuid = cells[index].message_uuid;
    let mut start = index;
    while start > 0 && cells[start - 1].message_uuid == uuid {
        start -= 1;
    }
    start
}

#[derive(Debug, Default, Clone)]
struct HistoryTailCache {
    width: Option<u16>,
    rows: Option<HistoryRows>,
}

impl HistoryTailCache {
    fn clear(&mut self) {
        self.width = None;
        self.rows = None;
    }

    fn replace_from_rows(&mut self, width: u16, rows: &HistoryRows) {
        self.width = Some(width);
        let source_rows = rows.height();
        let cached = rows.tail_rows_copy(HISTORY_TAIL_CACHE_MAX_ROWS);
        if source_rows > cached.height() {
            tracing::info!(
                target: "tui::surface::history_cache",
                width,
                source_rows,
                cached_rows = cached.height(),
                max_rows = HISTORY_TAIL_CACHE_MAX_ROWS,
                "history tail cache truncated to row cap",
            );
        }
        self.rows = Some(cached);
    }

    fn extend_from_rows(&mut self, width: u16, rows: &HistoryRows) {
        if rows.is_empty() {
            return;
        }
        if self.width != Some(width) {
            self.width = Some(width);
            self.rows = None;
        }
        let existing_rows = self.rows.as_ref().map_or(0, HistoryRows::height);
        let append_rows = rows.height();
        let source_rows = existing_rows.saturating_add(append_rows);
        let cached = match self.rows.as_ref() {
            Some(existing) => HistoryRows::copy_tail_from_slices(
                width,
                &[
                    existing.tail_slice(HISTORY_TAIL_CACHE_MAX_ROWS),
                    rows.tail_slice(HISTORY_TAIL_CACHE_MAX_ROWS),
                ],
                HISTORY_TAIL_CACHE_MAX_ROWS,
            ),
            None => rows.tail_rows_copy(HISTORY_TAIL_CACHE_MAX_ROWS),
        };
        if source_rows > cached.height() {
            tracing::info!(
                target: "tui::surface::history_cache",
                width,
                existing_rows,
                append_rows,
                source_rows,
                cached_rows = cached.height(),
                max_rows = HISTORY_TAIL_CACHE_MAX_ROWS,
                "history tail cache truncated to row cap",
            );
        }
        self.rows = Some(cached);
    }

    fn available_rows(&self, width: u16) -> u16 {
        if self.width != Some(width) {
            return 0;
        }
        self.rows.as_ref().map_or(0, HistoryRows::height)
    }

    fn tail_slice(&self, rows: u16) -> Option<HistoryRowsSlice<'_>> {
        let cached = self.rows.as_ref()?;
        if self.width != Some(cached.width()) {
            return None;
        }
        Some(cached.tail_slice(rows))
    }

    fn stats(&self) -> HistoryTailCacheStats {
        let Some(rows) = self.rows.as_ref() else {
            return HistoryTailCacheStats {
                width: self.width,
                ..HistoryTailCacheStats::default()
            };
        };
        HistoryTailCacheStats {
            rows: rows.height(),
            width: self.width,
            bytes_estimate: rows.estimated_bytes(),
        }
    }
}

#[cfg(test)]
#[path = "history_driver.test.rs"]
mod tests;
