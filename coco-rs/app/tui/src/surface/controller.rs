//! Native-scrollback draw orchestration.

use std::time::Instant;

use crate::FrameLayout;
use crate::presentation::styles::UiStyles;
use crate::state::AppState;
#[cfg(test)]
use crate::surface::compatibility::TerminalCompatibility;
use crate::surface::history_driver::SurfaceHistoryDriver;
use crate::surface::history_emitter::HistoryEmissionOutcome;
use crate::surface::history_lines::HistoryLineRenderOptions;
#[cfg(test)]
use crate::surface::overlay::OverlaySurfaceState;
use crate::surface::overlay::SurfaceFramePlan;
use crate::surface::terminal::SurfaceBackend;
use crate::surface::terminal::SurfaceTerminal;
use crate::surface::viewport::render_interactive_viewport;

#[derive(Debug, Default, Clone)]
pub(crate) struct NativeSurfaceController {
    history: SurfaceHistoryDriver,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeSurfaceDrawOutcome {
    #[cfg(test)]
    pub(crate) history: HistoryEmissionOutcome,
    pub(crate) layout: FrameLayout,
}

impl NativeSurfaceController {
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
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

    pub(crate) fn draw_with_plan<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
        plan: SurfaceFramePlan,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        self.draw_at_with_plan(terminal, state, Instant::now(), plan)
    }

    #[cfg(test)]
    pub(crate) fn draw_at<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
        now: Instant,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        let mut overlay_state = OverlaySurfaceState::default();
        let plan = overlay_state.plan(state, TerminalCompatibility::NativeScrollback, now);
        self.draw_at_with_plan(terminal, state, now, plan)
    }

    pub(crate) fn draw_at_with_plan<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
        now: Instant,
        plan: SurfaceFramePlan,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        terminal.begin_synchronized_update()?;
        let outcome = self.draw_at_inner(terminal, state, now, plan);
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
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: SurfaceBackend,
    {
        let viewport = terminal.viewport_area();
        let width = viewport.width;
        let stream_active = state.is_streaming();
        self.history
            .note_viewport(width, viewport.height, stream_active);

        let options = history_options(state, width);
        let session_header = || session_header_lines(state, width);
        let history = if !plan.native_history_enabled() {
            HistoryEmissionOutcome::Noop
        } else {
            let needs_stream_finish_replay =
                !stream_active && self.history.stream_finish_replay_needed();
            if self.history.replay_due(now) || needs_stream_finish_replay {
                self.history.replay_all_capped(
                    terminal,
                    session_header(),
                    &state.session.messages,
                    options,
                    stream_active,
                )?
            } else {
                let outcome = self.history.emit_append_only(
                    terminal,
                    session_header(),
                    &state.session.messages,
                    options,
                )?;
                if matches!(outcome, HistoryEmissionOutcome::ReplayRequired) {
                    self.history.replay_all_capped(
                        terminal,
                        session_header(),
                        &state.session.messages,
                        options,
                        stream_active,
                    )?
                } else {
                    outcome
                }
            }
        };

        let mut layout = FrameLayout::default();
        terminal.draw_viewport(|frame| {
            layout = render_interactive_viewport(frame, state, plan);
            if let Some(claim) = crate::cursor::compute_cursor(state, layout.input) {
                frame.set_cursor_claim(claim);
            }
        })?;

        #[cfg(not(test))]
        let _ = history;

        Ok(NativeSurfaceDrawOutcome {
            #[cfg(test)]
            history,
            layout,
        })
    }

    pub(crate) fn reset(&mut self) {
        self.history.reset();
    }
}

fn history_options(state: &AppState, width: u16) -> HistoryLineRenderOptions<'_> {
    HistoryLineRenderOptions {
        styles: UiStyles::new(&state.ui.theme),
        width,
        syntax_highlighting: state.ui.display_settings.syntax_highlighting,
        show_system_reminders: state.ui.show_system_reminders,
    }
}

fn session_header_lines(state: &AppState, width: u16) -> Vec<ratatui::text::Line<'static>> {
    crate::presentation::header::header_history_lines(state, UiStyles::new(&state.ui.theme), width)
}

#[cfg(test)]
#[path = "controller.test.rs"]
mod tests;
