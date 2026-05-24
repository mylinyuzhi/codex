//! Demand-driven frame scheduling with coalescing.
//!
//! The [`FrameRequester`] accepts draw requests (immediate or delayed) and
//! coalesces them into a single broadcast notification per frame, rate-limited
//! to 120 FPS via [`rate_limiter::FrameRateLimiter`].

use std::time::Duration;
use std::time::Instant;

use tokio::sync::broadcast;
use tokio::sync::mpsc;

use rate_limiter::FrameRateLimiter;

pub mod rate_limiter;

/// Cloneable handle for requesting frames.
///
/// Callers use [`schedule_frame`](FrameRequester::schedule_frame) for immediate
/// redraws or [`schedule_frame_in`](FrameRequester::schedule_frame_in) for
/// delayed redraws. All requests are coalesced by the background
/// [`FrameScheduler`] task.
#[derive(Clone, Debug)]
pub struct FrameRequester {
    frame_schedule_tx: mpsc::UnboundedSender<Instant>,
}

impl FrameRequester {
    /// Spawn a new frame scheduler and return a requester handle.
    ///
    /// The scheduler runs as a background tokio task that listens for draw
    /// requests and coalesces them into single `()` notifications on `draw_tx`.
    pub fn new(draw_tx: broadcast::Sender<()>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        let scheduler = FrameScheduler {
            receiver: rx,
            draw_tx,
            rate_limiter: FrameRateLimiter::default(),
        };
        tokio::spawn(scheduler.run());

        FrameRequester {
            frame_schedule_tx: tx,
        }
    }

    /// Request a frame as soon as the rate limiter allows.
    pub fn schedule_frame(&self) {
        let _ = self.frame_schedule_tx.send(Instant::now());
    }

    /// Request a frame after `dur` has elapsed.
    pub fn schedule_frame_in(&self, dur: Duration) {
        let _ = self.frame_schedule_tx.send(Instant::now() + dur);
    }
}

#[cfg(test)]
impl FrameRequester {
    /// Create a dummy requester for tests that don't need scheduling.
    pub(crate) fn test_dummy() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        FrameRequester {
            frame_schedule_tx: tx,
        }
    }
}

/// Background task that coalesces draw requests and emits frame notifications.
struct FrameScheduler {
    receiver: mpsc::UnboundedReceiver<Instant>,
    draw_tx: broadcast::Sender<()>,
    rate_limiter: FrameRateLimiter,
}

impl FrameScheduler {
    async fn run(mut self) {
        const ONE_YEAR: Duration = Duration::from_secs(60 * 60 * 24 * 365);

        let mut next_deadline: Option<Instant> = None;

        loop {
            let target = next_deadline.unwrap_or_else(|| Instant::now() + ONE_YEAR);
            let deadline = tokio::time::sleep_until(target.into());
            tokio::pin!(deadline);

            tokio::select! {
                draw_at = self.receiver.recv() => {
                    let Some(draw_at) = draw_at else { break };
                    let draw_at = self.rate_limiter.clamp_deadline(draw_at);
                    next_deadline = Some(
                        next_deadline.map_or(draw_at, |cur| cur.min(draw_at)),
                    );
                    continue;
                }
                () = &mut deadline => {
                    if next_deadline.is_some() {
                        next_deadline = None;
                        self.rate_limiter.mark_emitted(target);
                        let _ = self.draw_tx.send(());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
