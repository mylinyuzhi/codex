//! Adaptive stream chunking policy for display pacing.
//!
//! Two-gear system that preserves smooth typewriter effect during normal
//! streaming and switches to batch draining when backlog grows:
//!
//! - [`ChunkingMode::Smooth`]: advance display by one line per tick
//! - [`ChunkingMode::CatchUp`]: advance display to current content
//!
//! Hysteresis prevents rapid gear-flapping near threshold boundaries:
//! - enter catch-up on higher thresholds
//! - exit after sustained low-pressure hold
//! - suppress immediate re-entry unless backlog is severe

use std::time::Duration;
use std::time::Instant;

/// Queue-depth threshold (lines) to enter catch-up mode.
const ENTER_QUEUE_DEPTH_LINES: i32 = 8;

/// Oldest-line age threshold to enter catch-up mode.
const ENTER_OLDEST_AGE: Duration = Duration::from_millis(120);

/// Queue-depth threshold for evaluating catch-up exit hysteresis.
const EXIT_QUEUE_DEPTH_LINES: i32 = 2;

/// Oldest-line age threshold for evaluating catch-up exit hysteresis.
const EXIT_OLDEST_AGE: Duration = Duration::from_millis(40);

/// Duration queue pressure must stay below exit thresholds to leave catch-up.
const EXIT_HOLD: Duration = Duration::from_millis(250);

/// Cooldown after catch-up exit that suppresses immediate re-entry.
const REENTER_CATCH_UP_HOLD: Duration = Duration::from_millis(250);

/// Queue-depth cutoff that marks backlog as severe (bypasses re-entry hold).
const SEVERE_QUEUE_DEPTH_LINES: i32 = 64;

/// Oldest-line age cutoff that marks backlog as severe.
const SEVERE_OLDEST_AGE: Duration = Duration::from_millis(300);

/// Current display pacing mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChunkingMode {
    /// Advance one line per tick.
    #[default]
    Smooth,
    /// Advance multiple lines per tick to catch up with backlog.
    CatchUp,
}

/// Snapshot of current queue pressure used for chunking decisions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueSnapshot {
    /// Number of unrevealed lines in the stream.
    pub queued_lines: i32,
    /// Age of the oldest unrevealed line.
    pub oldest_age: Option<Duration>,
}

/// How many lines to drain this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrainPlan {
    /// Advance by exactly one line.
    Single,
    /// Advance by up to N lines.
    Batch(i32),
}

/// Result of a single chunking decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkingDecision {
    /// Mode after applying hysteresis transitions.
    pub mode: ChunkingMode,
    /// Whether this decision transitioned into catch-up.
    pub entered_catch_up: bool,
    /// Drain plan for the current tick.
    pub drain_plan: DrainPlan,
}

/// Adaptive chunking policy with hysteresis state.
#[derive(Debug, Default, Clone)]
pub struct AdaptiveChunkingPolicy {
    mode: ChunkingMode,
    below_exit_threshold_since: Option<Instant>,
    last_catch_up_exit_at: Option<Instant>,
}

impl AdaptiveChunkingPolicy {
    /// Returns the current mode.
    pub fn mode(&self) -> ChunkingMode {
        self.mode
    }

    /// Reset to baseline smooth mode.
    pub fn reset(&mut self) {
        self.mode = ChunkingMode::Smooth;
        self.below_exit_threshold_since = None;
        self.last_catch_up_exit_at = None;
    }

    /// Compute a drain decision from the current queue snapshot.
    pub fn decide(&mut self, snapshot: QueueSnapshot, now: Instant) -> ChunkingDecision {
        if snapshot.queued_lines == 0 {
            self.note_catch_up_exit(now);
            self.mode = ChunkingMode::Smooth;
            self.below_exit_threshold_since = None;
            return ChunkingDecision {
                mode: self.mode,
                entered_catch_up: false,
                drain_plan: DrainPlan::Single,
            };
        }

        let entered_catch_up = match self.mode {
            ChunkingMode::Smooth => self.maybe_enter_catch_up(snapshot, now),
            ChunkingMode::CatchUp => {
                self.maybe_exit_catch_up(snapshot, now);
                false
            }
        };

        let drain_plan = match self.mode {
            ChunkingMode::Smooth => DrainPlan::Single,
            ChunkingMode::CatchUp => DrainPlan::Batch(snapshot.queued_lines.max(1)),
        };

        ChunkingDecision {
            mode: self.mode,
            entered_catch_up,
            drain_plan,
        }
    }

    fn maybe_enter_catch_up(&mut self, snapshot: QueueSnapshot, now: Instant) -> bool {
        if !should_enter_catch_up(snapshot) {
            return false;
        }
        if self.reentry_hold_active(now) && !is_severe_backlog(snapshot) {
            return false;
        }
        self.mode = ChunkingMode::CatchUp;
        self.below_exit_threshold_since = None;
        self.last_catch_up_exit_at = None;
        true
    }

    fn maybe_exit_catch_up(&mut self, snapshot: QueueSnapshot, now: Instant) {
        if !should_exit_catch_up(snapshot) {
            self.below_exit_threshold_since = None;
            return;
        }

        match self.below_exit_threshold_since {
            Some(since) if now.saturating_duration_since(since) >= EXIT_HOLD => {
                self.mode = ChunkingMode::Smooth;
                self.below_exit_threshold_since = None;
                self.last_catch_up_exit_at = Some(now);
            }
            Some(_) => {}
            None => {
                self.below_exit_threshold_since = Some(now);
            }
        }
    }

    fn note_catch_up_exit(&mut self, now: Instant) {
        if self.mode == ChunkingMode::CatchUp {
            self.last_catch_up_exit_at = Some(now);
        }
    }

    fn reentry_hold_active(&self, now: Instant) -> bool {
        self.last_catch_up_exit_at
            .is_some_and(|exit| now.saturating_duration_since(exit) < REENTER_CATCH_UP_HOLD)
    }
}

fn should_enter_catch_up(snapshot: QueueSnapshot) -> bool {
    snapshot.queued_lines >= ENTER_QUEUE_DEPTH_LINES
        || snapshot
            .oldest_age
            .is_some_and(|oldest| oldest >= ENTER_OLDEST_AGE)
}

fn should_exit_catch_up(snapshot: QueueSnapshot) -> bool {
    snapshot.queued_lines <= EXIT_QUEUE_DEPTH_LINES
        && snapshot
            .oldest_age
            .is_some_and(|oldest| oldest <= EXIT_OLDEST_AGE)
}

fn is_severe_backlog(snapshot: QueueSnapshot) -> bool {
    snapshot.queued_lines >= SEVERE_QUEUE_DEPTH_LINES
        || snapshot
            .oldest_age
            .is_some_and(|oldest| oldest >= SEVERE_OLDEST_AGE)
}

#[cfg(test)]
#[path = "chunking.test.rs"]
mod tests;
