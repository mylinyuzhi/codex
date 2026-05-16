//! Surface history orchestration for native scrollback.
// S2 driver lands before production `Tui` switches to `SurfaceTerminal`.
#![allow(dead_code)]

use std::time::Instant;

use ratatui::backend::Backend;
use ratatui::text::Line;

use crate::state::session::ChatMessage;
use crate::surface::history_emitter::HistoryEmissionOutcome;
use crate::surface::history_emitter::HistoryEmissionPlan;
use crate::surface::history_emitter::HistoryEmissionTracker;
use crate::surface::history_lines::DEFAULT_MAX_REFLOW_ROWS;
use crate::surface::history_lines::HistoryLineRenderOptions;
use crate::surface::history_lines::render_finalized_history_lines;
use crate::surface::history_lines::render_replay_history_lines;
use crate::surface::history_reflow::HistoryReflowState;
use crate::surface::history_reflow::HistoryViewportChange;
use crate::surface::history_reflow::HistoryWidthChange;
use crate::surface::terminal::SurfaceTerminal;

#[derive(Debug, Default, Clone)]
pub(crate) struct SurfaceHistoryDriver {
    emitter: HistoryEmissionTracker,
    reflow: HistoryReflowState,
    header_fingerprint: Option<Vec<String>>,
}

impl SurfaceHistoryDriver {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn note_width(&mut self, width: u16, stream_active: bool) -> HistoryWidthChange {
        let change = self.reflow.note_width(width);
        if change.changed && self.reflow.replay_needed_for_width(width) {
            self.reflow.schedule_resize_replay(width, stream_active);
        }
        change
    }

    pub(crate) fn note_viewport(
        &mut self,
        width: u16,
        height: u16,
        stream_active: bool,
    ) -> HistoryViewportChange {
        let change = self.reflow.note_viewport(width, height);
        if change.changed && self.reflow.replay_needed_for_viewport(width, height) {
            self.reflow
                .schedule_viewport_replay(width, height, stream_active);
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
        messages: &[ChatMessage],
        options: HistoryLineRenderOptions<'_>,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: Backend,
    {
        let header_fingerprint = line_fingerprint(&session_header);
        if self
            .header_fingerprint
            .as_ref()
            .is_some_and(|emitted| emitted != &header_fingerprint)
        {
            return Ok(HistoryEmissionOutcome::ReplayRequired);
        }

        let plan = self.emitter.plan(messages);
        let should_emit_header = self.header_fingerprint.is_none();
        if matches!(plan, HistoryEmissionPlan::Noop) && !should_emit_header {
            return Ok(HistoryEmissionOutcome::Noop);
        }
        if matches!(plan, HistoryEmissionPlan::ReplayRequired) {
            return Ok(HistoryEmissionOutcome::ReplayRequired);
        }

        let start = match plan {
            HistoryEmissionPlan::Append { start } => start,
            HistoryEmissionPlan::Noop | HistoryEmissionPlan::ReplayRequired => messages.len(),
        };
        let mut lines = Vec::new();
        if should_emit_header {
            lines.extend(session_header);
        }
        lines.extend(render_finalized_history_lines(&messages[start..], options));
        let rows = terminal.insert_history_lines(lines)?;
        self.header_fingerprint = Some(header_fingerprint);
        self.emitter.mark_emitted_through(messages, messages.len());
        Ok(HistoryEmissionOutcome::Appended {
            start,
            message_count: messages.len() - start,
            rows,
        })
    }

    pub(crate) fn replay_all<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        session_header: Vec<Line<'static>>,
        messages: &[ChatMessage],
        options: HistoryLineRenderOptions<'_>,
        stream_active: bool,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: Backend,
    {
        let outcome = self.replay_lines(
            terminal,
            session_header,
            messages,
            render_finalized_history_lines(messages, options),
        )?;
        let area = terminal.viewport_area();
        self.reflow
            .mark_replayed_viewport(area.width, area.height, stream_active);
        Ok(outcome)
    }

    pub(crate) fn replay_all_capped<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        session_header: Vec<Line<'static>>,
        messages: &[ChatMessage],
        options: HistoryLineRenderOptions<'_>,
        stream_active: bool,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: Backend,
    {
        let replay = render_replay_history_lines(messages, options, DEFAULT_MAX_REFLOW_ROWS).lines;
        let outcome = self.replay_lines(terminal, session_header, messages, replay)?;
        let area = terminal.viewport_area();
        self.reflow
            .mark_replayed_viewport(area.width, area.height, stream_active);
        Ok(outcome)
    }

    pub(crate) fn stream_finish_replay_needed(&mut self) -> bool {
        self.reflow.take_stream_finish_replay_needed()
    }

    pub(crate) fn reset(&mut self) {
        self.emitter.reset();
        self.header_fingerprint = None;
        self.reflow.clear();
    }

    fn replay_lines<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        session_header: Vec<Line<'static>>,
        messages: &[ChatMessage],
        message_lines: Vec<Line<'static>>,
    ) -> Result<HistoryEmissionOutcome, B::Error>
    where
        B: Backend,
    {
        let header_fingerprint = line_fingerprint(&session_header);
        let mut lines = session_header;
        lines.extend(message_lines);
        terminal.clear_owned_scrollback()?;
        let rows = terminal.insert_history_lines(lines)?;
        self.header_fingerprint = Some(header_fingerprint);
        self.emitter.mark_emitted_through(messages, messages.len());
        Ok(HistoryEmissionOutcome::Replayed {
            message_count: messages.len(),
            rows,
        })
    }

    #[cfg(test)]
    pub(crate) fn force_replay_due_for_test(&mut self) {
        self.reflow.force_due_for_test();
    }
}

fn line_fingerprint(lines: &[Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

#[cfg(test)]
#[path = "history_driver.test.rs"]
mod tests;
