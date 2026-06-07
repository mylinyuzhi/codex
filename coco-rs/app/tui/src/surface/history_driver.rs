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

use crate::presentation::transcript::TranscriptCell;
use crate::presentation::transcript::TranscriptProjectionOptions;
use crate::presentation::transcript::native_history_projection;
use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;
use crate::streaming::render_controller::StreamRenderInput;
use crate::streaming::render_controller::StreamRenderKey;
use crate::streaming::render_controller::StreamRenderMode;
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
use crate::surface::stream::CommittedStablePrefix;
use crate::surface::stream::PreparedProvisionalAppend;
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
    provisional: Option<ProvisionalStreamLedger>,
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
    pub(crate) message_count: usize,
    pub(crate) transcript_revision: u64,
    pub(crate) header_fingerprint: Vec<RenderedLineFingerprint>,
    pub(crate) emitted_header: bool,
    pub(crate) rows: HistoryRows,
    pub(crate) render_elapsed: Duration,
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
}

impl SurfaceHistoryDriver {
    pub(crate) fn emit_provisional_stream<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        append: &PreparedProvisionalAppend,
    ) -> Result<ProvisionalAppendOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        if append.rows.is_empty() {
            return Ok(ProvisionalAppendOutcome::SkippedNoRows);
        }
        if let Some(existing) = self.provisional.as_ref()
            && let Some(failure) = existing.compatibility_failure(append)
        {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "provisional_stream_render_key_or_prefix_mismatch",
                guard = failure.as_str(),
                existing_prefix_source_bytes = existing.source.len(),
                append_prefix_source_bytes = append.committed_prefix.source.len(),
                existing_line_count = existing.line_count,
                append_line_count = append.committed_prefix.line_count,
                ?existing.render_key,
                ?append.committed_prefix.render_key,
                "history full replay required",
            );
            return Ok(ProvisionalAppendOutcome::ReplayRequired);
        }
        let append_line_count = append.line_count;
        let prefix = append.committed_prefix.clone();
        let ledger_line_count = prefix.line_count;
        let prefix_source_bytes = prefix.source.len();
        let render_key = prefix.render_key;
        let width = terminal.viewport_area().width;
        let rows = terminal.insert_history_rows(&append.rows)?;
        if rows == 0 {
            return Ok(ProvisionalAppendOutcome::SkippedNoRows);
        }
        self.emitted_history_rows = self.emitted_history_rows.saturating_add(rows);
        self.tail_cache.extend_from_rows(width, &append.rows);
        self.provisional = Some(prefix);
        tracing::debug!(
            target: "tui::surface::append",
            rows,
            append_line_count,
            ledger_line_count,
            prefix_source_bytes,
            render_elapsed_us = append.render_elapsed.as_micros(),
            ?render_key,
            "provisional stream stable append",
        );
        Ok(ProvisionalAppendOutcome::Written { rows })
    }

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
        let header_fingerprint = fingerprint_lines(&session_header);
        if self
            .header_fingerprint
            .as_ref()
            .is_some_and(|emitted| emitted != &header_fingerprint)
        {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "header_fingerprint_changed",
                cells = cells.len(),
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

        let plan = self.emitter.plan(cells);
        if matches!(plan, HistoryEmissionPlan::Noop) && !should_emit_header {
            return PreparedFinalizedHistory::Noop {
                transcript_revision,
            };
        }
        if matches!(plan, HistoryEmissionPlan::ReplayRequired) {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "emitter_uuid_prefix_mismatch",
                cells = cells.len(),
                emitted_messages = self.emitter.emitted_count(),
                "history full replay required",
            );
            return PreparedFinalizedHistory::ReplayRequired;
        }

        let start = match plan {
            HistoryEmissionPlan::Append { start } => start,
            HistoryEmissionPlan::Noop | HistoryEmissionPlan::ReplayRequired => cells.len(),
        };
        let Some(lines) = self.append_candidate_lines_from_plan(
            session_header,
            cells,
            start,
            should_emit_header,
            options,
        ) else {
            return PreparedFinalizedHistory::ReplayRequired;
        };
        let render_started = Instant::now();
        let rows = render_history_rows(lines, options.width);
        PreparedFinalizedHistory::Append(PreparedHistoryAppend {
            start,
            message_count: cells.len() - start,
            transcript_revision,
            header_fingerprint,
            emitted_header: should_emit_header,
            rows,
            render_elapsed: render_started.elapsed(),
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
                self.provisional = None;
                let width = terminal.viewport_area().width;
                let rows = terminal.insert_history_rows(&append.rows)?;
                self.emitted_history_rows = self.emitted_history_rows.saturating_add(rows);
                self.tail_cache.extend_from_rows(width, &append.rows);
                self.header_fingerprint = Some(append.header_fingerprint.clone());
                if append.emitted_header {
                    self.emitter.mark_emitted_through(cells, cells.len());
                } else {
                    self.emitter.mark_appended_from(cells, append.start);
                }
                self.emitted_transcript_revision = Some(append.transcript_revision);
                tracing::trace!(
                    target: "tui::surface::append",
                    start = append.start,
                    cells_total = cells.len(),
                    message_count = append.message_count,
                    rows,
                    emitted_header = append.emitted_header,
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

    #[cfg(test)]
    pub(crate) fn replay_all<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        session_header: Vec<Line<'static>>,
        cells: &[RenderedCell],
        transcript_revision: u64,
        options: HistoryLineRenderOptions<'_>,
        stream_active: bool,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        let outcome = self.replay_lines(
            terminal,
            session_header,
            cells,
            transcript_revision,
            render_finalized_history_lines(cells, options),
        )?;
        let area = terminal.viewport_area();
        self.reflow
            .mark_replayed_viewport(area.width, stream_active);
        Ok(outcome)
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
        let started = Instant::now();
        let replay = render_replay_history_lines_cached(
            cells,
            options,
            DEFAULT_MAX_REFLOW_ROWS,
            &mut self.replay_cache,
        );
        let render_elapsed = started.elapsed();
        let line_count = replay.lines.len();
        let outcome = self.replay_rows(
            terminal,
            session_header,
            cells,
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
            cells = cells.len(),
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
        self.provisional = None;
    }

    #[cfg(test)]
    fn replay_lines<B, I>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        session_header: Vec<Line<'static>>,
        cells: &[RenderedCell],
        transcript_revision: u64,
        message_lines: I,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: SurfaceBackend,
        I: IntoIterator<Item = Line<'static>>,
    {
        let header_fingerprint = fingerprint_lines(&session_header);
        let mut lines = session_header;
        lines.extend(message_lines);
        let viewport_area = terminal.viewport_area();
        terminal.clear_owned_scrollback()?;
        let width = viewport_area.width;
        let rendered = render_history_rows(lines, width);
        let rows = terminal.insert_history_rows(&rendered)?;
        if terminal.viewport_area() != viewport_area {
            terminal.set_viewport_area(viewport_area);
        }
        self.emitted_history_rows = rows;
        self.tail_cache.replace_from_rows(width, &rendered);
        self.header_fingerprint = Some(header_fingerprint);
        self.emitter.mark_emitted_through(cells, cells.len());
        self.emitted_transcript_revision = Some(transcript_revision);
        self.provisional = None;
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
        let rows = terminal.insert_history_rows(&rendered)?;
        if terminal.viewport_area() != viewport_area {
            terminal.set_viewport_area(viewport_area);
        }
        self.emitted_history_rows = rows;
        self.tail_cache.replace_from_rows(width, &rendered);
        self.header_fingerprint = Some(header_fingerprint);
        self.emitter.mark_emitted_through(cells, cells.len());
        self.emitted_transcript_revision = Some(transcript_revision);
        self.provisional = None;
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
        should_emit_header: bool,
        options: HistoryLineRenderOptions<'_>,
    ) -> Option<Vec<Line<'static>>> {
        let mut lines = Vec::new();
        if should_emit_header {
            lines.extend(session_header);
        }
        if let Some(provisional) = self.provisional.as_ref() {
            match consolidated_final_tail_lines(cells, start, provisional, options) {
                Ok(consolidated) => {
                    tracing::debug!(
                        target: "tui::surface::append",
                        provisional_line_count = provisional.line_count,
                        finalized_line_count = consolidated.final_line_count,
                        residual_line_count = consolidated.lines.len(),
                        prefix_source_bytes = provisional.source.len(),
                        render_elapsed_us = consolidated.render_elapsed_us,
                        ?provisional.render_key,
                        "provisional stream finalized tail append",
                    );
                    lines.extend(consolidated.lines);
                }
                Err(failure) => {
                    failure.log(
                        provisional,
                        cells.len(),
                        start,
                        self.emitter.emitted_count(),
                        options,
                    );
                    tracing::debug!(
                        target: "tui::surface::replay",
                        cause = "provisional_stream_guard_mismatch",
                        cells = cells.len(),
                        emitted_messages = self.emitter.emitted_count(),
                        "history full replay required",
                    );
                    return None;
                }
            }
        } else {
            lines.extend(render_finalized_history_lines(&cells[start..], options));
        }
        Some(lines)
    }
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

type ProvisionalStreamLedger = CommittedStablePrefix;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProvisionalAppendOutcome {
    Written { rows: u16 },
    SkippedNoRows,
    ReplayRequired,
}

impl ProvisionalStreamLedger {
    fn compatibility_failure(
        &self,
        append: &PreparedProvisionalAppend,
    ) -> Option<ProvisionalAppendCompatibilityFailure> {
        if self.render_key != append.committed_prefix.render_key {
            return Some(ProvisionalAppendCompatibilityFailure::RenderKeyMismatch);
        }
        if append.committed_prefix.line_count < self.line_count {
            return Some(ProvisionalAppendCompatibilityFailure::LineCountRegression);
        }
        if !append.committed_prefix.source.starts_with(&self.source) {
            return Some(ProvisionalAppendCompatibilityFailure::SourcePrefixMismatch);
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProvisionalAppendCompatibilityFailure {
    RenderKeyMismatch,
    LineCountRegression,
    SourcePrefixMismatch,
}

impl ProvisionalAppendCompatibilityFailure {
    const fn as_str(self) -> &'static str {
        match self {
            Self::RenderKeyMismatch => "render_key_mismatch",
            Self::LineCountRegression => "line_count_regression",
            Self::SourcePrefixMismatch => "source_prefix_mismatch",
        }
    }
}

#[derive(Debug)]
struct ConsolidatedFinalTail {
    lines: Vec<Line<'static>>,
    final_line_count: usize,
    render_elapsed_us: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProvisionalFinalizationGuard {
    MissingFinalCell,
    PresentationPrefixIncompatible,
    RenderKeyMismatch {
        finalized_render_key: StreamRenderKey,
    },
    SourcePrefixMismatch {
        common_prefix_bytes: usize,
        prefix_next_byte: DivergenceByteKind,
        final_next_byte: DivergenceByteKind,
        prefix_trailing_newline_bytes: usize,
        final_trailing_newline_bytes: usize,
    },
    LineCountExceedsFinalized {
        final_line_count: usize,
        render_elapsed_us: u128,
    },
}

impl ProvisionalFinalizationGuard {
    fn log(
        self,
        provisional: &ProvisionalStreamLedger,
        cells_len: usize,
        start: usize,
        emitted_messages: usize,
        options: HistoryLineRenderOptions<'_>,
    ) {
        match self {
            Self::MissingFinalCell => tracing::debug!(
                target: "tui::surface::replay",
                guard = "missing_final_cell",
                cells = cells_len,
                start,
                emitted_messages,
                provisional_line_count = provisional.line_count,
                prefix_source_bytes = provisional.source.len(),
                ?provisional.render_key,
                "provisional stream finalization guard failed",
            ),
            Self::PresentationPrefixIncompatible => tracing::debug!(
                target: "tui::surface::replay",
                guard = "presentation_prefix_incompatible",
                cells = cells_len,
                start,
                emitted_messages,
                provisional_line_count = provisional.line_count,
                prefix_source_bytes = provisional.source.len(),
                ?provisional.render_key,
                "provisional stream finalization guard failed",
            ),
            Self::RenderKeyMismatch {
                finalized_render_key,
            } => tracing::debug!(
                target: "tui::surface::replay",
                guard = "render_key_mismatch",
                cells = cells_len,
                start,
                emitted_messages,
                provisional_line_count = provisional.line_count,
                prefix_source_bytes = provisional.source.len(),
                width = options.width,
                syntax_highlighting = options.syntax_highlighting.is_enabled(),
                theme_hash = options.styles.theme_hash(),
                ?provisional.render_key,
                ?finalized_render_key,
                "provisional stream finalization guard failed",
            ),
            Self::SourcePrefixMismatch {
                common_prefix_bytes,
                prefix_next_byte,
                final_next_byte,
                prefix_trailing_newline_bytes,
                final_trailing_newline_bytes,
            } => tracing::debug!(
                target: "tui::surface::replay",
                guard = "source_prefix_mismatch",
                cells = cells_len,
                start,
                emitted_messages,
                provisional_line_count = provisional.line_count,
                prefix_source_bytes = provisional.source.len(),
                common_prefix_bytes,
                prefix_remainder_bytes = provisional
                    .source
                    .len()
                    .saturating_sub(common_prefix_bytes),
                ?prefix_next_byte,
                ?final_next_byte,
                prefix_trailing_newline_bytes,
                final_trailing_newline_bytes,
                ?provisional.render_key,
                "provisional stream finalization guard failed",
            ),
            Self::LineCountExceedsFinalized {
                final_line_count,
                render_elapsed_us,
            } => tracing::debug!(
                target: "tui::surface::replay",
                guard = "line_count_exceeds_finalized",
                cells = cells_len,
                start,
                emitted_messages,
                provisional_line_count = provisional.line_count,
                final_line_count,
                prefix_source_bytes = provisional.source.len(),
                render_elapsed_us,
                ?provisional.render_key,
                "provisional stream finalization guard failed",
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DivergenceByteKind {
    Eof,
    Newline,
    AsciiWhitespace,
    AsciiAlphanumeric,
    AsciiPunctuation,
    NonAscii,
}

fn consolidated_final_tail_lines(
    cells: &[RenderedCell],
    start: usize,
    provisional: &ProvisionalStreamLedger,
    options: HistoryLineRenderOptions<'_>,
) -> Result<ConsolidatedFinalTail, ProvisionalFinalizationGuard> {
    let projected = native_history_projection(
        &cells[start..],
        TranscriptProjectionOptions {
            show_system_reminders: options.show_system_reminders,
            show_compact_internals: false,
        },
    );
    let first_segment = projected
        .cells
        .first()
        .ok_or(ProvisionalFinalizationGuard::MissingFinalCell)?;
    let TranscriptCell::Cell { index } = first_segment else {
        return Err(ProvisionalFinalizationGuard::PresentationPrefixIncompatible);
    };
    let first = cells
        .get(start + index)
        .ok_or(ProvisionalFinalizationGuard::MissingFinalCell)?;
    let CellKind::AssistantText { text, .. } = &first.kind else {
        return Err(ProvisionalFinalizationGuard::PresentationPrefixIncompatible);
    };
    let finalized_render_key = finalized_render_key(options);
    if provisional.render_key != finalized_render_key {
        return Err(ProvisionalFinalizationGuard::RenderKeyMismatch {
            finalized_render_key,
        });
    }
    if !text.starts_with(&provisional.source) {
        let common_prefix_bytes = common_prefix_bytes(&provisional.source, text);
        return Err(ProvisionalFinalizationGuard::SourcePrefixMismatch {
            common_prefix_bytes,
            prefix_next_byte: divergence_byte_kind(&provisional.source, common_prefix_bytes),
            final_next_byte: divergence_byte_kind(text, common_prefix_bytes),
            prefix_trailing_newline_bytes: trailing_newline_bytes(&provisional.source),
            final_trailing_newline_bytes: trailing_newline_bytes(text),
        });
    }

    let render_started = Instant::now();
    let final_tail_lines = render_finalized_history_lines(&cells[start..], options);
    let render_elapsed_us = render_started.elapsed().as_micros();
    if final_tail_lines.len() < provisional.line_count {
        return Err(ProvisionalFinalizationGuard::LineCountExceedsFinalized {
            final_line_count: final_tail_lines.len(),
            render_elapsed_us,
        });
    }
    let final_line_count = final_tail_lines.len();
    Ok(ConsolidatedFinalTail {
        lines: final_tail_lines
            .into_iter()
            .skip(provisional.line_count)
            .collect(),
        final_line_count,
        render_elapsed_us,
    })
}

fn finalized_render_key(options: HistoryLineRenderOptions<'_>) -> StreamRenderKey {
    StreamRenderKey::new(
        StreamRenderInput {
            source: "",
            styles: options.styles,
            width: options.width,
            syntax_highlighting: options.syntax_highlighting,
        },
        StreamRenderMode::FinalizedStable,
    )
}

fn common_prefix_bytes(left: &str, right: &str) -> usize {
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .take_while(|(left, right)| left == right)
        .count()
}

fn trailing_newline_bytes(source: &str) -> usize {
    source
        .as_bytes()
        .iter()
        .rev()
        .take_while(|&&byte| matches!(byte, b'\n' | b'\r'))
        .count()
}

fn divergence_byte_kind(source: &str, index: usize) -> DivergenceByteKind {
    let Some(byte) = source.as_bytes().get(index).copied() else {
        return DivergenceByteKind::Eof;
    };
    match byte {
        b'\n' | b'\r' => DivergenceByteKind::Newline,
        b'\t' | b' ' => DivergenceByteKind::AsciiWhitespace,
        b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' => DivergenceByteKind::AsciiAlphanumeric,
        0x00..=0x7f => DivergenceByteKind::AsciiPunctuation,
        0x80..=0xff => DivergenceByteKind::NonAscii,
    }
}

#[cfg(test)]
#[path = "history_driver.test.rs"]
mod tests;
