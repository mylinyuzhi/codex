//! Adaptive chunking policy for streaming display pacing.
//!
//! Two-gear system with hysteresis to prevent flapping:
//! - **Smooth**: advance 1 line per tick (typewriter effect)
//! - **CatchUp**: advance N lines per tick (batch draining)

use std::time::Duration;
use std::time::Instant;

/// Thresholds for entering CatchUp mode.
const CATCHUP_QUEUE_THRESHOLD: usize = 8;
const CATCHUP_AGE_THRESHOLD: Duration = Duration::from_millis(120);

/// Thresholds for exiting CatchUp mode.
const SMOOTH_QUEUE_THRESHOLD: usize = 2;
const SMOOTH_AGE_THRESHOLD: Duration = Duration::from_millis(40);

/// Hold duration before mode transition (prevents flapping).
const MODE_HOLD_DURATION: Duration = Duration::from_millis(250);

/// Severe backlog — skip directly to CatchUp.
const SEVERE_QUEUE_THRESHOLD: usize = 64;
const SEVERE_AGE_THRESHOLD: Duration = Duration::from_millis(300);

/// Lines per tick in CatchUp mode.
const CATCHUP_BATCH_SIZE: usize = 4;

/// Chunking mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkingMode {
    Smooth,
    CatchUp,
}

/// Adaptive chunking policy.
pub struct AdaptiveChunking {
    mode: ChunkingMode,
    mode_entered_at: Instant,
}

impl AdaptiveChunking {
    /// Create a new policy in Smooth mode.
    pub fn new() -> Self {
        Self {
            mode: ChunkingMode::Smooth,
            mode_entered_at: Instant::now(),
        }
    }

    /// Determine how many lines to advance this tick.
    pub fn plan(&mut self, queue_depth: usize, age: Option<Duration>) -> usize {
        let age_ms = age.unwrap_or(Duration::ZERO);

        // Severe backlog — force CatchUp immediately
        if queue_depth >= SEVERE_QUEUE_THRESHOLD || age_ms >= SEVERE_AGE_THRESHOLD {
            self.enter_mode(ChunkingMode::CatchUp);
            return CATCHUP_BATCH_SIZE;
        }

        match self.mode {
            ChunkingMode::Smooth => {
                // Check if we should enter CatchUp
                if queue_depth >= CATCHUP_QUEUE_THRESHOLD || age_ms >= CATCHUP_AGE_THRESHOLD {
                    self.enter_mode(ChunkingMode::CatchUp);
                    return CATCHUP_BATCH_SIZE;
                }
                1 // smooth: 1 line per tick
            }
            ChunkingMode::CatchUp => {
                // Check if we can exit to Smooth
                if queue_depth <= SMOOTH_QUEUE_THRESHOLD
                    && age_ms <= SMOOTH_AGE_THRESHOLD
                    && self.mode_entered_at.elapsed() >= MODE_HOLD_DURATION
                {
                    self.enter_mode(ChunkingMode::Smooth);
                    return 1;
                }
                CATCHUP_BATCH_SIZE
            }
        }
    }

    fn enter_mode(&mut self, mode: ChunkingMode) {
        if self.mode != mode {
            self.mode = mode;
            self.mode_entered_at = Instant::now();
        }
    }
}

impl Default for AdaptiveChunking {
    fn default() -> Self {
        Self::new()
    }
}
