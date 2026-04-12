//! Async TUI event stream.
//!
//! Multiplexes crossterm terminal events with tick/spinner timers
//! into a unified [`TuiEvent`] stream. Respects [`EventBroker`]
//! pause state for external editor integration.

use crossterm::event::Event;
use crossterm::event::EventStream;
use crossterm::event::KeyEventKind;
use tokio::time::Interval;
use tokio::time::interval;
use tokio_stream::StreamExt;

use super::broker::EventBroker;
use crate::constants;
use crate::events::TuiEvent;

/// Async stream that yields [`TuiEvent`]s from multiple sources.
pub struct TuiEventStream {
    event_stream: EventStream,
    tick_interval: Interval,
    spinner_interval: Interval,
    broker: EventBroker,
}

impl TuiEventStream {
    /// Create a new event stream.
    pub fn new(broker: EventBroker) -> Self {
        Self {
            event_stream: EventStream::new(),
            tick_interval: interval(constants::TICK_INTERVAL),
            spinner_interval: interval(constants::SPINNER_TICK_INTERVAL),
            broker,
        }
    }

    /// Get the next event, blocking until one is available.
    ///
    /// Returns `None` if the event stream is exhausted (terminal closed).
    pub async fn next(&mut self) -> Option<TuiEvent> {
        loop {
            let event = tokio::select! {
                // Terminal events (respects pause state)
                Some(Ok(evt)) = self.event_stream.next(), if !self.broker.is_paused() => {
                    self.convert_crossterm_event(evt)
                }
                // Tick timer (250ms)
                _ = self.tick_interval.tick() => {
                    Some(TuiEvent::Tick)
                }
                // Spinner timer (50ms)
                _ = self.spinner_interval.tick() => {
                    Some(TuiEvent::SpinnerTick)
                }
            };

            if let Some(evt) = event {
                return Some(evt);
            }
            // If convert returned None (e.g., key release), loop again
        }
    }

    /// Convert a crossterm event to a TuiEvent.
    fn convert_crossterm_event(&self, event: Event) -> Option<TuiEvent> {
        match event {
            Event::Key(key) => {
                // Only handle Press events (not Release/Repeat) for cross-platform
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
}
