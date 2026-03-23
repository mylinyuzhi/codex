//! Frame rate limiter that clamps deadlines to 120 FPS max.

use std::time::Duration;
use std::time::Instant;

/// 120 FPS minimum frame interval (~8.33ms).
pub(super) const MIN_FRAME_INTERVAL: Duration = Duration::from_nanos(8_333_334);

/// Clamps draw deadlines to max 120 FPS.
#[derive(Debug, Default)]
pub(super) struct FrameRateLimiter {
    last_emitted_at: Option<Instant>,
}

impl FrameRateLimiter {
    /// Clamp a requested deadline so it is no earlier than
    /// `last_emitted + MIN_FRAME_INTERVAL`.
    pub(super) fn clamp_deadline(&self, requested: Instant) -> Instant {
        match self.last_emitted_at {
            None => requested,
            Some(last) => {
                let earliest_allowed = last + MIN_FRAME_INTERVAL;
                requested.max(earliest_allowed)
            }
        }
    }

    /// Record that a frame was emitted at the given instant.
    pub(super) fn mark_emitted(&mut self, emitted_at: Instant) {
        self.last_emitted_at = Some(emitted_at);
    }
}

#[cfg(test)]
#[path = "rate_limiter.test.rs"]
mod tests;
