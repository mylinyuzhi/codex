//! Native-scrollback draw orchestration.

use std::time::Instant;

use crate::FrameLayout;
use crate::state::AppState;
use crate::surface::history_driver::HistoryReplayMode;
use crate::surface::history_driver::ProvisionalAppendOutcome;
use crate::surface::history_driver::SurfaceHistoryDriver;
use crate::surface::history_emitter::HistoryEmissionOutcome;
use crate::surface::history_lines::HistoryLineRenderOptions;
#[cfg(any(test, feature = "testing"))]
use crate::surface::modal::ModalSurfaceState;
use crate::surface::modal::SurfaceFramePlan;
use crate::surface::stream::ProvisionalStableAppend;
use crate::surface::stream::SurfaceStreamDriver;
use crate::surface::viewport::build_live_tail_lines;
use crate::surface::viewport::render_interactive_viewport;
use crate::widgets::TranscriptLayoutIndex;
#[cfg(any(test, feature = "testing"))]
use coco_tui_ui::engine::compatibility::TerminalCompatibility;
use coco_tui_ui::engine::terminal::HistoryInsertStats;
use coco_tui_ui::engine::terminal::SurfaceBackend;
use coco_tui_ui::engine::terminal::SurfaceTerminal;
use coco_tui_ui::style::UiStyles;
use ratatui::text::Line;

#[derive(Debug, Default, Clone)]
pub(crate) struct NativeSurfaceController {
    history: SurfaceHistoryDriver,
    stream: SurfaceStreamDriver,
    pending_stable_append: Option<ProvisionalStableAppend>,
    transcript_layout: TranscriptLayoutIndex,
    history_display: Option<HistoryDisplayState>,
    /// One-shot: force a full history replay on the next frame so finalized
    /// content re-seats the viewport after a turn-end height relax (see
    /// `Tui::sync_surface_area`). Without it the viewport keeps the tall
    /// streaming `history_bottom_y` and the input bar settles high with a blank
    /// gap below once the conversation has overflowed the screen.
    pending_repin: bool,
}

/// Display-mode inputs whose change requires a full history re-render (the
/// committed cells are re-derived). Deliberately excludes reasoning metadata:
/// that is a side-cache read at cell-build time (`history_options`), so the
/// finalize draw bakes it into the assistant cell's single append-only emit —
/// a per-turn metadata attach must NOT force `replay_all_capped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HistoryDisplayState {
    show_system_reminders: bool,
    show_thinking: bool,
    syntax_highlighting: coco_tui_ui::display::SyntaxHighlighting,
    theme_hash: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeSurfaceDrawOutcome {
    #[cfg(test)]
    pub(crate) history: HistoryEmissionOutcome,
    pub(crate) layout: FrameLayout,
}

impl NativeSurfaceController {
    pub(crate) fn prepare_live_tail(
        &mut self,
        state: &AppState,
        width: u16,
        plan: SurfaceFramePlan,
    ) -> Vec<Line<'static>> {
        let prepared = self.stream.prepare(state, width, plan);
        self.pending_stable_append = prepared.stable_append;
        prepared.lines
    }

    /// Force a full history replay on the next frame so finalized content
    /// re-pins the viewport to the bottom of native scrollback. Called by
    /// `Tui::sync_surface_area` when the grow-only height freeze releases at
    /// turn end and the viewport shrinks.
    pub(crate) fn request_repin_replay(&mut self) {
        self.pending_repin = true;
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn draw<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        self.draw_at(terminal, state, Instant::now())
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn draw_with_plan<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
        plan: SurfaceFramePlan,
        precomputed_live: Option<Vec<Line<'static>>>,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        // Unit tests have no `Tui` wrapper, so the test helper owns the
        // synchronized-update window itself.
        self.draw_at_with_plan(terminal, state, Instant::now(), plan, precomputed_live, 0)
    }

    /// Production frame entry. The caller (`Tui::draw_with_frame_index`) owns
    /// the single synchronized-update window that also brackets the viewport
    /// resize (`sync_surface_area`), so this path must NOT open its own — a
    /// nested `?2026h`/`?2026l` would emit a premature ESU and present a torn
    /// mid-frame. It therefore calls `draw_at_inner` directly.
    pub(crate) fn draw_with_plan_at_frame<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
        plan: SurfaceFramePlan,
        precomputed_live: Option<Vec<Line<'static>>>,
        frame_index: u64,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        self.draw_at_inner(
            terminal,
            state,
            Instant::now(),
            plan,
            precomputed_live,
            frame_index,
        )
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn draw_at<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
        now: Instant,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        let mut modal_state = ModalSurfaceState::default();
        let plan = modal_state.plan(state, TerminalCompatibility::NativeScrollback, now);
        self.draw_at_with_plan(terminal, state, now, plan, None, 0)
    }

    /// Test-only frame entry that owns its synchronized-update window. The
    /// production path goes through `draw_with_plan_at_frame`, where `Tui`
    /// owns the single window (see that method's note); this variant exists so
    /// unit tests can drive a full frame without a `Tui`.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn draw_at_with_plan<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
        now: Instant,
        plan: SurfaceFramePlan,
        precomputed_live: Option<Vec<Line<'static>>>,
        frame_index: u64,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        terminal.begin_synchronized_update()?;
        let outcome = self.draw_at_inner(terminal, state, now, plan, precomputed_live, frame_index);
        let end = terminal.end_synchronized_update();
        match (outcome, end) {
            (Ok(outcome), Ok(())) => Ok(outcome),
            (Err(err), _) | (Ok(_), Err(err)) => Err(err),
        }
    }

    fn draw_at_inner<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
        now: Instant,
        plan: SurfaceFramePlan,
        precomputed_live: Option<Vec<Line<'static>>>,
        frame_index: u64,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        let viewport = terminal.viewport_area();
        let width = viewport.width;
        let stream_active = state.is_streaming();
        let perf_config = state.ui.display_settings.performance;
        let mut precomputed_live =
            precomputed_live.or_else(|| Some(self.prepare_live_tail(state, width, plan)));
        self.history.note_viewport(width, stream_active);

        let options = history_options(state, width);
        let history_display = HistoryDisplayState::from(state);
        let session_header = || session_header_lines(state, width);
        // Feed the native history driver with the engine-authoritative
        // `&[RenderedCell]` slice directly. Engine-pushed content
        // (cancel marker, resume scrollback, hooks, …) flows through
        // `MessageAppended` → `TranscriptView` → `cells()`.
        let cells = state.session.transcript.cells();
        let transcript_revision = state.session.transcript.revision();
        let history_start = perf_config.enabled.then(Instant::now);
        // Taken unconditionally so a relax request can never leak into a later
        // frame if native history is briefly disabled (alt-screen / modal).
        let needs_repin_replay = std::mem::take(&mut self.pending_repin);
        let mut history = if !plan.native_history_enabled() {
            HistoryEmissionOutcome::Noop
        } else {
            let history_display_changed = self
                .history_display
                .replace(history_display)
                .is_some_and(|previous| previous != history_display);
            let needs_stream_finish_replay =
                !stream_active && self.history.stream_finish_replay_needed();
            let needs_reflow_replay = self.history.replay_due(now);
            if history_display_changed
                || needs_reflow_replay
                || needs_stream_finish_replay
                || needs_repin_replay
            {
                let cause = if needs_repin_replay {
                    "viewport_relax_repin"
                } else if needs_stream_finish_replay {
                    "stream_finish_pending_replay"
                } else if history_display_changed {
                    "history_display_changed"
                } else {
                    "reflow_debounce_due"
                };
                tracing::debug!(
                    target: "tui::surface::replay",
                    cause,
                    reflow_due = needs_reflow_replay,
                    stream_finish = needs_stream_finish_replay,
                    history_display_changed,
                    cells = cells.len(),
                    width = viewport.width,
                    height = viewport.height,
                    stream_active,
                    "history full replay",
                );
                let outcome = self.history.replay_all_capped(
                    terminal,
                    session_header(),
                    cells,
                    transcript_revision,
                    options,
                    HistoryReplayMode {
                        stream_active,
                        cause,
                    },
                )?;
                if stream_active {
                    self.stream.forget_stable_appended();
                    precomputed_live = Some(self.prepare_live_tail(state, width, plan));
                }
                outcome
            } else {
                let outcome = self.history.emit_append_only(
                    terminal,
                    session_header(),
                    cells,
                    transcript_revision,
                    options,
                )?;
                if matches!(outcome, HistoryEmissionOutcome::ReplayRequired) {
                    let outcome = self.history.replay_all_capped(
                        terminal,
                        session_header(),
                        cells,
                        transcript_revision,
                        options,
                        HistoryReplayMode {
                            stream_active,
                            cause: "emitter_replay_required",
                        },
                    )?;
                    if stream_active {
                        self.stream.forget_stable_appended();
                        precomputed_live = Some(self.prepare_live_tail(state, width, plan));
                    }
                    outcome
                } else {
                    outcome
                }
            }
        };
        let mut finalized_history_stats = history_insert_stats_for(terminal, &history);
        let mut provisional_stats = HistoryInsertStats::default();
        if plan.native_history_enabled()
            && let Some(stable_append) = self.pending_stable_append.take()
        {
            match self
                .history
                .emit_provisional_stream(terminal, stable_append.clone())?
            {
                ProvisionalAppendOutcome::Written { .. } => {
                    provisional_stats = terminal.last_history_insert_stats();
                    self.stream.mark_stable_appended();
                }
                ProvisionalAppendOutcome::SkippedNoRows => {
                    precomputed_live = Some(build_live_tail_lines(
                        state,
                        UiStyles::new(&state.ui.theme),
                        width,
                        plan,
                    ));
                }
                ProvisionalAppendOutcome::ReplayRequired => {
                    history = self.history.replay_all_capped(
                        terminal,
                        session_header(),
                        cells,
                        transcript_revision,
                        options,
                        HistoryReplayMode {
                            stream_active,
                            cause: "provisional_stream_repair",
                        },
                    )?;
                    finalized_history_stats = history_insert_stats_for(terminal, &history);
                    match self
                        .history
                        .emit_provisional_stream(terminal, stable_append)?
                    {
                        ProvisionalAppendOutcome::Written { .. } => {
                            provisional_stats = terminal.last_history_insert_stats();
                            self.stream.mark_stable_appended();
                        }
                        ProvisionalAppendOutcome::SkippedNoRows => {
                            precomputed_live = Some(build_live_tail_lines(
                                state,
                                UiStyles::new(&state.ui.theme),
                                width,
                                plan,
                            ));
                        }
                        ProvisionalAppendOutcome::ReplayRequired => {}
                    }
                }
            }
        }
        let history_elapsed = history_start.map(|start| start.elapsed());
        if let Some(elapsed) = history_elapsed
            && crate::perf::should_log_stage(perf_config, frame_index, elapsed)
        {
            tracing::debug!(
                target: crate::perf::TARGET,
                stage = "history",
                duration_us = crate::perf::duration_us(elapsed),
                history_outcome = history_outcome_name(&history),
                history_fast_noop = matches!(history, HistoryEmissionOutcome::FastNoop { .. }),
                transcript_revision,
                cells = cells.len(),
                rows = history_rows(&history),
                wrapped_rows = finalized_history_stats.wrapped_rows,
                buffer_updates = finalized_history_stats.buffer_updates,
                bytes_written = finalized_history_stats.bytes_written,
                invalidated = finalized_history_stats.invalidated,
                build_us = crate::perf::duration_us(finalized_history_stats.build_elapsed),
                draw_us = crate::perf::duration_us(finalized_history_stats.draw_elapsed),
                flush_us = crate::perf::duration_us(finalized_history_stats.flush_elapsed),
                provisional_rows = provisional_stats.wrapped_rows,
                provisional_bytes = provisional_stats.bytes_written,
                provisional_build_us = crate::perf::duration_us(provisional_stats.build_elapsed),
                provisional_draw_us = crate::perf::duration_us(provisional_stats.draw_elapsed),
                provisional_flush_us = crate::perf::duration_us(provisional_stats.flush_elapsed),
                "tui frame history stage completed",
            );
        }

        let mut layout = FrameLayout::default();
        let viewport_start = perf_config.enabled.then(Instant::now);
        terminal.draw_viewport(|frame| {
            layout = render_interactive_viewport(
                frame,
                state,
                plan,
                &mut self.transcript_layout,
                precomputed_live,
            );
            if let Some(claim) = crate::cursor::compute_cursor(state, layout.input) {
                frame.set_cursor_claim(claim);
            }
        })?;
        let viewport_elapsed = viewport_start.map(|start| start.elapsed());
        if let Some(elapsed) = viewport_elapsed
            && crate::perf::should_log_stage(perf_config, frame_index, elapsed)
        {
            let stats = terminal.last_viewport_draw_stats();
            tracing::debug!(
                target: crate::perf::TARGET,
                stage = "viewport_draw",
                duration_us = crate::perf::duration_us(elapsed),
                buffer_updates = stats.buffer_updates,
                invalidated = stats.invalidated,
                diff_us = crate::perf::duration_us(stats.diff_elapsed),
                draw_us = crate::perf::duration_us(stats.draw_elapsed),
                flush_us = crate::perf::duration_us(stats.flush_elapsed),
                "tui frame viewport stage completed",
            );
        }

        Ok(NativeSurfaceDrawOutcome {
            #[cfg(test)]
            history,
            layout,
        })
    }

    pub(crate) fn reset(&mut self) {
        self.history.reset();
        self.stream.reset();
        self.pending_stable_append = None;
        self.transcript_layout.reset();
        self.pending_repin = false;
    }
}

fn history_outcome_name(outcome: &HistoryEmissionOutcome) -> &'static str {
    match outcome {
        HistoryEmissionOutcome::Noop => "noop",
        HistoryEmissionOutcome::FastNoop { .. } => "fast_noop",
        HistoryEmissionOutcome::Appended { .. } => "appended",
        HistoryEmissionOutcome::Replayed { .. } => "replayed",
        HistoryEmissionOutcome::ReplayRequired => "replay_required",
    }
}

fn history_rows(outcome: &HistoryEmissionOutcome) -> u16 {
    match outcome {
        HistoryEmissionOutcome::Appended { rows, .. }
        | HistoryEmissionOutcome::Replayed { rows, .. } => *rows,
        HistoryEmissionOutcome::Noop
        | HistoryEmissionOutcome::FastNoop { .. }
        | HistoryEmissionOutcome::ReplayRequired => 0,
    }
}

fn history_insert_stats_for<B>(
    terminal: &SurfaceTerminal<B>,
    outcome: &HistoryEmissionOutcome,
) -> HistoryInsertStats
where
    B: SurfaceBackend,
{
    match outcome {
        HistoryEmissionOutcome::Appended { .. } | HistoryEmissionOutcome::Replayed { .. } => {
            terminal.last_history_insert_stats()
        }
        HistoryEmissionOutcome::Noop
        | HistoryEmissionOutcome::FastNoop { .. }
        | HistoryEmissionOutcome::ReplayRequired => HistoryInsertStats::default(),
    }
}

impl From<&AppState> for HistoryDisplayState {
    fn from(state: &AppState) -> Self {
        Self {
            show_system_reminders: state.ui.show_system_reminders,
            show_thinking: state.ui.show_thinking,
            syntax_highlighting: state.ui.display_settings.syntax_highlighting,
            theme_hash: UiStyles::new(&state.ui.theme).theme_hash(),
        }
    }
}

fn history_options(state: &AppState, width: u16) -> HistoryLineRenderOptions<'_> {
    HistoryLineRenderOptions {
        styles: UiStyles::new(&state.ui.theme),
        width,
        syntax_highlighting: state.ui.display_settings.syntax_highlighting,
        show_system_reminders: state.ui.show_system_reminders,
        show_thinking: state.ui.show_thinking,
        cwd: state.session.working_dir.as_deref(),
        kb_handle: Some(&state.ui.kb_handle),
        replay_cache_policy: state.ui.display_settings.native_replay_cache,
        reasoning_metadata: Some(&state.session.reasoning_metadata),
    }
}

fn session_header_lines(state: &AppState, width: u16) -> Vec<ratatui::text::Line<'static>> {
    crate::presentation::header::header_history_lines(state, UiStyles::new(&state.ui.theme), width)
}

#[cfg(test)]
#[path = "controller.test.rs"]
mod tests;
