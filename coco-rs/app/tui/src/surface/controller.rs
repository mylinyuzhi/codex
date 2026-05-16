//! Native-scrollback draw orchestration.
// S3 controller lands before production `Tui` switches to `SurfaceTerminal`.
#![allow(dead_code)]

use std::time::Instant;

use ratatui::backend::Backend;

use crate::presentation::styles::UiStyles;
use crate::render::FrameLayout;
use crate::state::AppState;
use crate::surface::history_driver::SurfaceHistoryDriver;
use crate::surface::history_emitter::HistoryEmissionOutcome;
use crate::surface::history_lines::HistoryLineRenderOptions;
use crate::surface::overlay::history_emission_deferred;
use crate::surface::terminal::SurfaceTerminal;
use crate::surface::viewport::render_interactive_viewport;

#[derive(Debug, Default, Clone)]
pub(crate) struct NativeSurfaceController {
    history: SurfaceHistoryDriver,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeSurfaceDrawOutcome {
    pub(crate) history: HistoryEmissionOutcome,
    pub(crate) layout: FrameLayout,
}

impl NativeSurfaceController {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn draw<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: Backend,
    {
        self.draw_at(terminal, state, Instant::now())
    }

    pub(crate) fn draw_at<B>(
        &mut self,
        terminal: &mut SurfaceTerminal<B>,
        state: &AppState,
        now: Instant,
    ) -> Result<NativeSurfaceDrawOutcome, B::Error>
    where
        B: Backend,
    {
        let viewport = terminal.viewport_area();
        let width = viewport.width;
        let stream_active = state.is_streaming();
        self.history.note_width(width, stream_active);

        let options = history_options(state, width);
        let session_header = || session_header_lines(state, width);
        let history = if history_emission_deferred(state.ui.overlay.as_ref()) {
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
            layout = render_interactive_viewport(frame, state);
            if let Some(claim) = crate::cursor::compute_cursor(state, layout.input) {
                frame.set_cursor_claim(claim);
            }
        })?;

        Ok(NativeSurfaceDrawOutcome { history, layout })
    }

    pub(crate) fn reset(&mut self) {
        self.history.reset();
    }

    #[cfg(test)]
    pub(crate) fn force_history_replay_due_for_test(&mut self) {
        self.history.force_replay_due_for_test();
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
