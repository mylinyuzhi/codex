//! Surface history orchestration for native scrollback.
//!
//! Phase 3d (§4): operates on `&[RenderedCell]` directly. The
//! `HistoryEmissionTracker` still tracks exactly-once emission by
//! engine message UUIDs, which are stable across the engine
//! `MessageAppended` events and survive resume reloads (each cell
//! carries `Arc<Message>` from the engine `MessageHistory`).
use std::time::Instant;

use ratatui::text::Line;
use sha2::Digest;
use sha2::Sha256;

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
use crate::surface::stream::ProvisionalStableAppend;
use coco_tui_ui::engine::history_reflow::HistoryReflowState;
use coco_tui_ui::engine::history_reflow::HistoryViewportChange;
use coco_tui_ui::engine::terminal::SurfaceBackend;
use coco_tui_ui::engine::terminal::SurfaceTerminal;

#[derive(Debug, Default, Clone)]
pub(crate) struct SurfaceHistoryDriver {
    emitter: HistoryEmissionTracker,
    reflow: HistoryReflowState,
    header_fingerprint: Option<Vec<RenderedLineFingerprint>>,
    emitted_transcript_revision: Option<u64>,
    replay_cache: HistoryReplayCache,
    provisional: Option<ProvisionalStreamLedger>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HistoryReplayMode {
    pub(crate) stream_active: bool,
    pub(crate) cause: &'static str,
}

impl SurfaceHistoryDriver {
    pub(crate) fn emit_provisional_stream<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        append: ProvisionalStableAppend,
    ) -> Result<ProvisionalAppendOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        if append.append_lines.is_empty() {
            return Ok(ProvisionalAppendOutcome::SkippedNoRows);
        }
        if let Some(existing) = self.provisional.as_ref()
            && !existing.is_compatible_with(&append)
        {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "provisional_stream_render_key_or_prefix_mismatch",
                "history full replay required",
            );
            return Ok(ProvisionalAppendOutcome::ReplayRequired);
        }
        let rows = terminal.insert_history_lines(append.append_lines)?;
        if rows == 0 {
            return Ok(ProvisionalAppendOutcome::SkippedNoRows);
        }
        let mut prefix_source = append.append_source;
        let mut line_fingerprints = append.append_line_fingerprints;
        if let Some(mut existing) = self.provisional.take() {
            existing.prefix_source.push_str(&prefix_source);
            prefix_source = existing.prefix_source;
            existing.line_fingerprints.append(&mut line_fingerprints);
            line_fingerprints = existing.line_fingerprints;
        }
        self.provisional = Some(ProvisionalStreamLedger {
            prefix_digest: append.prefix_digest,
            prefix_source,
            line_fingerprints,
            render_key: append.render_key,
        });
        tracing::trace!(
            target: "tui::surface::append",
            rows,
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
        if change.changed && self.reflow.replay_needed_for_viewport(width) {
            self.reflow.schedule_viewport_replay(width, stream_active);
        }
        change
    }

    pub(crate) fn replay_due(&self, now: Instant) -> bool {
        self.reflow.pending_is_due(now)
    }

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
            return Ok(HistoryEmissionOutcome::ReplayRequired);
        }

        let should_emit_header = self.header_fingerprint.is_none();
        if !should_emit_header && self.emitted_transcript_revision == Some(transcript_revision) {
            return Ok(HistoryEmissionOutcome::FastNoop {
                revision: transcript_revision,
            });
        }

        let plan = self.emitter.plan(cells);
        if matches!(plan, HistoryEmissionPlan::Noop) && !should_emit_header {
            self.emitted_transcript_revision = Some(transcript_revision);
            return Ok(HistoryEmissionOutcome::Noop);
        }
        if matches!(plan, HistoryEmissionPlan::ReplayRequired) {
            tracing::debug!(
                target: "tui::surface::replay",
                cause = "emitter_uuid_prefix_mismatch",
                cells = cells.len(),
                emitted_messages = self.emitter.emitted_count(),
                "history full replay required",
            );
            return Ok(HistoryEmissionOutcome::ReplayRequired);
        }

        let start = match plan {
            HistoryEmissionPlan::Append { start } => start,
            HistoryEmissionPlan::Noop | HistoryEmissionPlan::ReplayRequired => cells.len(),
        };
        let mut lines = Vec::new();
        if should_emit_header {
            lines.extend(session_header);
        }
        if let Some(provisional) = self.provisional.as_ref() {
            if let Some(remainder) =
                consolidated_final_tail_lines(cells, start, provisional, options)
            {
                lines.extend(remainder);
                self.provisional = None;
            } else {
                self.provisional = None;
                tracing::debug!(
                    target: "tui::surface::replay",
                    cause = "provisional_stream_parity_mismatch",
                    cells = cells.len(),
                    emitted_messages = self.emitter.emitted_count(),
                    "history full replay required",
                );
                return Ok(HistoryEmissionOutcome::ReplayRequired);
            }
        } else {
            lines.extend(render_finalized_history_lines(&cells[start..], options));
        }
        let rows = terminal.insert_history_lines(lines)?;
        self.header_fingerprint = Some(header_fingerprint);
        if should_emit_header {
            self.emitter.mark_emitted_through(cells, cells.len());
        } else {
            self.emitter.mark_appended_from(cells, start);
        }
        self.emitted_transcript_revision = Some(transcript_revision);
        tracing::trace!(
            target: "tui::surface::append",
            start,
            cells_total = cells.len(),
            message_count = cells.len() - start,
            rows,
            emitted_header = should_emit_header,
            "history incremental append",
        );
        Ok(HistoryEmissionOutcome::Appended {
            start,
            message_count: cells.len() - start,
            rows,
        })
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
        let outcome = self.replay_lines(
            terminal,
            session_header,
            cells,
            transcript_revision,
            replay.lines.iter().cloned(),
        )?;
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
            estimated_cloned_lines = line_count,
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
            height = area.height,
            stream_active = mode.stream_active,
            "history replay render completed",
        );
        Ok(outcome)
    }

    pub(crate) fn stream_finish_replay_needed(&mut self) -> bool {
        self.reflow.take_stream_finish_replay_needed()
    }

    pub(crate) fn reset(&mut self) {
        self.emitter.reset();
        self.header_fingerprint = None;
        self.emitted_transcript_revision = None;
        self.reflow.clear();
        self.replay_cache.clear();
        self.provisional = None;
    }

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
        terminal.clear_owned_scrollback()?;
        let rows =
            terminal.insert_history_lines(session_header.into_iter().chain(message_lines))?;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProvisionalStreamLedger {
    prefix_source: String,
    prefix_digest: [u8; 32],
    line_fingerprints: Vec<RenderedLineFingerprint>,
    render_key: StreamRenderKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProvisionalAppendOutcome {
    Written { rows: u16 },
    SkippedNoRows,
    ReplayRequired,
}

impl ProvisionalStreamLedger {
    fn is_compatible_with(&self, append: &ProvisionalStableAppend) -> bool {
        self.render_key == append.render_key && self.prefix_digest == append.prior_prefix_digest
    }
}

fn consolidated_final_tail_lines(
    cells: &[RenderedCell],
    start: usize,
    provisional: &ProvisionalStreamLedger,
    options: HistoryLineRenderOptions<'_>,
) -> Option<Vec<Line<'static>>> {
    let first = cells.get(start)?;
    let CellKind::AssistantText { text, .. } = &first.kind else {
        return None;
    };
    if digest_str(&provisional.prefix_source) != provisional.prefix_digest {
        return None;
    }
    if provisional.render_key != finalized_render_key(options) {
        return None;
    }
    if !text.starts_with(&provisional.prefix_source) {
        return None;
    }

    let final_tail_lines = render_finalized_history_lines(&cells[start..], options);
    let provisional_line_count = provisional.line_fingerprints.len();
    if final_tail_lines.len() < provisional_line_count {
        return None;
    }
    if fingerprint_lines(&final_tail_lines[..provisional_line_count])
        != provisional.line_fingerprints
    {
        return None;
    }
    Some(
        final_tail_lines
            .into_iter()
            .skip(provisional_line_count)
            .collect(),
    )
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

fn digest_str(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

#[cfg(test)]
#[path = "history_driver.test.rs"]
mod tests;
