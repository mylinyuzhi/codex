//! Main TUI application — the event loop driver.
//!
//! Implements the async run loop using `tokio::select!` to multiplex
//! terminal events, agent events, and timer ticks.

use std::io;

use crossterm::event::Event;
use crossterm::event::EventStream;
use crossterm::event::KeyEventKind;
use crossterm::event::MouseEventKind;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio_stream::StreamExt;

use crate::command::UserCommand;
use crate::constants;
use crate::events::TuiCommand;
use crate::events::TuiEvent;
use crate::keybinding_bridge;
use crate::render;
use crate::state::AppState;
use crate::terminal::Tui;
use crate::update::handle_command;

use coco_types::CoreEvent;

use crate::server_notification_handler;

/// Create the TUI ↔ Core communication channels.
///
/// Returns (command_tx, command_rx, event_tx, event_rx):
/// - command: TUI → Core (user actions)
/// - event: Core → TUI (agent CoreEvent stream, 3-layer Protocol/Stream/Tui)
pub fn create_channels() -> (
    mpsc::Sender<UserCommand>,
    mpsc::Receiver<UserCommand>,
    mpsc::Sender<CoreEvent>,
    mpsc::Receiver<CoreEvent>,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<UserCommand>(32);
    let (event_tx, event_rx) = mpsc::channel::<CoreEvent>(256);
    (cmd_tx, cmd_rx, event_tx, event_rx)
}

/// Main TUI application.
pub struct App {
    tui: Tui,
    state: AppState,
    command_tx: mpsc::Sender<UserCommand>,
    /// Receives CoreEvent (3-layer: Protocol/Stream/Tui) from the agent loop.
    notification_rx: mpsc::Receiver<CoreEvent>,
}

impl App {
    /// Create a new TUI application.
    pub fn new(
        command_tx: mpsc::Sender<UserCommand>,
        notification_rx: mpsc::Receiver<CoreEvent>,
    ) -> io::Result<Self> {
        let tui = Tui::new()?;
        let state = AppState::new();

        Ok(Self {
            tui,
            state,
            command_tx,
            notification_rx,
        })
    }

    /// Create with an existing terminal (for testing).
    pub fn with_terminal(
        tui: Tui,
        command_tx: mpsc::Sender<UserCommand>,
        notification_rx: mpsc::Receiver<CoreEvent>,
    ) -> Self {
        Self {
            tui,
            state: AppState::new(),
            command_tx,
            notification_rx,
        }
    }

    /// Get a reference to the state.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Get a mutable reference to the state.
    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    /// Run the main event loop.
    ///
    /// Uses `tokio::select!` to multiplex:
    /// - Terminal events (key, mouse, resize, paste)
    /// - Tick timer (250ms — toast expiry, idle detection)
    /// - Spinner timer (50ms — animation frames)
    pub async fn run(&mut self) -> io::Result<()> {
        // Initial render
        let state = &self.state;
        self.tui.draw(|frame| render::render(frame, state))?;

        let mut event_stream = EventStream::new();
        let mut tick_interval = interval(constants::TICK_INTERVAL);
        let mut spinner_interval = interval(constants::SPINNER_TICK_INTERVAL);

        loop {
            let mut needs_redraw = false;

            tokio::select! {
                // Terminal events
                Some(Ok(evt)) = event_stream.next() => {
                    if let Some(event) = self.convert_crossterm_event(evt) {
                        needs_redraw = self.handle_event(event).await;
                    }
                }
                // Agent CoreEvent from core — coalesce pending events before redraw.
                // Under high throughput (e.g. 100+ TextDeltas/sec) this avoids
                // one redraw per token by draining all ready events first.
                Some(event) = self.notification_rx.recv() => {
                    needs_redraw = server_notification_handler::handle_core_event(
                        &mut self.state,
                        event,
                    );
                    while let Ok(next) = self.notification_rx.try_recv() {
                        needs_redraw |= server_notification_handler::handle_core_event(
                            &mut self.state,
                            next,
                        );
                    }
                }
                // Tick timer
                _ = tick_interval.tick() => {
                    needs_redraw = self.handle_event(TuiEvent::Tick).await;
                }
                // Spinner timer
                _ = spinner_interval.tick() => {
                    needs_redraw = self.handle_event(TuiEvent::SpinnerTick).await;
                }
            };

            if needs_redraw {
                let state = &self.state;
                self.tui.draw(|frame| render::render(frame, state))?;
            }

            if self.state.should_exit() {
                break;
            }
        }

        Ok(())
    }

    /// Convert a crossterm event to a TuiEvent.
    fn convert_crossterm_event(&self, event: Event) -> Option<TuiEvent> {
        match event {
            Event::Key(key) => {
                // Only handle key press events (not release/repeat) for cross-platform
                if key.kind != KeyEventKind::Press {
                    return None;
                }
                Some(TuiEvent::Key(key))
            }
            Event::Mouse(mouse) => Some(TuiEvent::Mouse(mouse)),
            Event::Resize(w, h) => Some(TuiEvent::Resize {
                width: w,
                height: h,
            }),
            Event::FocusGained => Some(TuiEvent::FocusChanged { focused: true }),
            Event::FocusLost => Some(TuiEvent::FocusChanged { focused: false }),
            Event::Paste(text) => Some(TuiEvent::Paste(text)),
        }
    }

    /// Handle a TUI event, returning true if redraw needed.
    async fn handle_event(&mut self, event: TuiEvent) -> bool {
        match event {
            TuiEvent::Key(key) => {
                // Track Esc timing for double-Esc rewind detection.
                // TS: useDoublePress() in PromptInput.tsx
                if key.code == crossterm::event::KeyCode::Esc {
                    self.state.ui.last_esc_time = Some(std::time::Instant::now());
                }
                // Delegate all key mapping to keybinding_bridge
                if let Some(cmd) = keybinding_bridge::map_key(&self.state, key) {
                    handle_command(&mut self.state, cmd, &self.command_tx).await
                } else {
                    false
                }
            }
            TuiEvent::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => {
                    handle_command(
                        &mut self.state,
                        TuiCommand::MouseScroll(/*up*/ 1),
                        &self.command_tx,
                    )
                    .await
                }
                MouseEventKind::ScrollDown => {
                    handle_command(
                        &mut self.state,
                        TuiCommand::MouseScroll(/*down*/ -1),
                        &self.command_tx,
                    )
                    .await
                }
                MouseEventKind::Down(_) => {
                    handle_command(
                        &mut self.state,
                        TuiCommand::MouseClick {
                            col: mouse.column,
                            row: mouse.row,
                        },
                        &self.command_tx,
                    )
                    .await
                }
                _ => false,
            },
            TuiEvent::Tick => {
                let had_toasts = self.state.ui.has_toasts();
                self.state.ui.expire_toasts();
                had_toasts && !self.state.ui.has_toasts()
            }
            TuiEvent::SpinnerTick => {
                if let Some(ref mut streaming) = self.state.ui.streaming {
                    streaming.advance_display()
                } else {
                    false
                }
            }
            TuiEvent::Paste(text) => {
                for c in text.chars() {
                    self.state.ui.input.insert_char(c);
                }
                true
            }
            TuiEvent::Resize { .. } => true,
            TuiEvent::FocusChanged { .. } => false,
            TuiEvent::Draw => true,
            TuiEvent::ClassifierApproved {
                request_id,
                matched_rule,
            } => {
                if let Some(crate::state::Overlay::Permission(ref p)) = self.state.ui.overlay
                    && p.request_id == request_id
                    && let Some(crate::state::Overlay::Permission(ref mut p)) =
                        self.state.ui.overlay
                {
                    p.classifier_checking = false;
                    p.classifier_auto_approved = Some(matched_rule.unwrap_or_default());
                }
                true
            }
            TuiEvent::ClassifierDenied { .. } => {
                if let Some(crate::state::Overlay::Permission(ref mut p)) = self.state.ui.overlay {
                    p.classifier_checking = false;
                }
                true
            }
        }
    }
}
