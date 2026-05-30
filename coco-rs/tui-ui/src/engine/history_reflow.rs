//! Source-backed native-history replay scheduling.

use std::time::Duration;
use std::time::Instant;

pub const HISTORY_REFLOW_DEBOUNCE: Duration = Duration::from_millis(75);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryViewportChange {
    pub initialized: bool,
    pub changed: bool,
}

#[derive(Debug, Default, Clone)]
pub struct HistoryReflowState {
    last_observed_width: Option<u16>,
    last_observed_height: Option<u16>,
    last_replayed_width: Option<u16>,
    last_replayed_height: Option<u16>,
    pending_width: Option<u16>,
    pending_height: Option<u16>,
    pending_until: Option<Instant>,
    replayed_during_stream: bool,
    resize_requested_during_stream: bool,
}

impl HistoryReflowState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn note_viewport(&mut self, width: u16, height: u16) -> HistoryViewportChange {
        let previous_width = self.last_observed_width.replace(width);
        let previous_height = self.last_observed_height.replace(height);
        if previous_width.is_none() || previous_height.is_none() {
            self.last_replayed_width = Some(width);
            self.last_replayed_height = Some(height);
        }
        HistoryViewportChange {
            initialized: previous_width.is_none() || previous_height.is_none(),
            changed: previous_width.zip(previous_height).is_some_and(
                |(previous_width, previous_height)| {
                    previous_width != width || previous_height != height
                },
            ),
        }
    }

    pub fn replay_needed_for_viewport(&self, width: u16, height: u16) -> bool {
        let replayed =
            self.last_replayed_width == Some(width) && self.last_replayed_height == Some(height);
        let pending = self.pending_width == Some(width) && self.pending_height == Some(height);
        !replayed && !pending
    }

    pub fn schedule_viewport_replay(&mut self, width: u16, height: u16, stream_active: bool) {
        self.pending_width = Some(width);
        self.pending_height = Some(height);
        self.schedule_pending(stream_active);
    }

    fn schedule_pending(&mut self, stream_active: bool) {
        self.pending_until = Some(Instant::now() + HISTORY_REFLOW_DEBOUNCE);
        if stream_active {
            self.resize_requested_during_stream = true;
        }
    }

    pub fn schedule_immediate(&mut self) {
        self.pending_until = Some(Instant::now());
    }

    pub fn pending_is_due(&self, now: Instant) -> bool {
        self.pending_until.is_some_and(|deadline| now >= deadline)
    }

    pub fn pending_viewport(&self) -> Option<(u16, u16)> {
        Some((self.pending_width?, self.pending_height?))
    }

    pub fn clear_pending(&mut self) {
        self.pending_width = None;
        self.pending_height = None;
        self.pending_until = None;
    }

    pub fn mark_replayed_viewport(&mut self, width: u16, height: u16, stream_active: bool) {
        self.last_replayed_width = Some(width);
        self.last_replayed_height = Some(height);
        self.clear_pending();
        if stream_active {
            self.replayed_during_stream = true;
        }
    }

    pub fn take_stream_finish_replay_needed(&mut self) -> bool {
        let needed = self.replayed_during_stream || self.resize_requested_during_stream;
        self.replayed_during_stream = false;
        self.resize_requested_during_stream = false;
        needed
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn force_due_for_test(&mut self) {
        self.pending_until = Some(Instant::now() - Duration::from_millis(1));
    }
}

#[cfg(test)]
#[path = "history_reflow.test.rs"]
mod tests;
