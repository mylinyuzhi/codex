//! Adaptive streaming display for smooth content pacing.
//!
//! Instead of rendering the full accumulated content on every frame,
//! this module tracks a "display cursor" that advances through the
//! content adaptively:
//!
//! - **Smooth mode**: reveals one line per tick (typewriter effect)
//! - **CatchUp mode**: reveals all pending lines when backlog grows
//!
//! The [`StreamDisplay`] struct holds the cursor and chunking policy.
//! It is embedded in `StreamingState` and advanced on each `SpinnerTick`.

pub mod chunking;

use std::time::Instant;

use chunking::AdaptiveChunkingPolicy;
use chunking::ChunkingDecision;
use chunking::DrainPlan;
use chunking::QueueSnapshot;

/// Tracks how much of the streaming content has been "revealed" to the user.
#[derive(Debug, Clone)]
pub struct StreamDisplay {
    /// Byte offset into `StreamingState.content` up to which the user can see.
    display_cursor: usize,
    /// Adaptive pacing policy.
    chunking: AdaptiveChunkingPolicy,
    /// When unrevealed content first appeared (for age-based pressure).
    pending_since: Option<Instant>,
}

impl Default for StreamDisplay {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamDisplay {
    /// Create a new display tracker starting at the beginning.
    pub fn new() -> Self {
        Self {
            display_cursor: 0,
            chunking: AdaptiveChunkingPolicy::default(),
            pending_since: None,
        }
    }

    /// The byte offset up to which content should be displayed.
    pub fn cursor(&self) -> usize {
        self.display_cursor
    }

    /// Mark that new content was appended at the given total content length.
    ///
    /// If there is already unrevealed content, the pending timestamp is preserved.
    /// Otherwise it is set to `now`.
    pub fn on_content_appended(&mut self, total_len: usize) {
        if self.display_cursor < total_len && self.pending_since.is_none() {
            self.pending_since = Some(Instant::now());
        }
    }

    /// Advance the display cursor based on adaptive chunking.
    ///
    /// Returns the new display content slice endpoint and whether the display changed.
    /// `content` is the full accumulated streaming content.
    pub fn advance(&mut self, content: &str) -> bool {
        let total = content.len();
        if self.display_cursor >= total {
            return false;
        }

        let now = Instant::now();
        let unrevealed = &content[self.display_cursor..];
        let queued_lines = count_newlines(unrevealed);

        let snapshot = QueueSnapshot {
            queued_lines,
            oldest_age: self.pending_since.map(|t| now.saturating_duration_since(t)),
        };

        let decision: ChunkingDecision = self.chunking.decide(snapshot, now);

        let advanced = match decision.drain_plan {
            DrainPlan::Single => self.advance_one_line(content),
            DrainPlan::Batch(n) => self.advance_n_lines(content, n),
        };

        // Clear pending timestamp if we've caught up
        if self.display_cursor >= total {
            self.pending_since = None;
        }

        advanced
    }

    /// Instantly reveal all content (used on finalization).
    pub fn reveal_all(&mut self, total_len: usize) {
        self.display_cursor = total_len;
        self.pending_since = None;
        self.chunking.reset();
    }

    /// Reset to initial state.
    pub fn reset(&mut self) {
        self.display_cursor = 0;
        self.pending_since = None;
        self.chunking.reset();
    }

    /// Advance display cursor to the next newline boundary.
    fn advance_one_line(&mut self, content: &str) -> bool {
        let unrevealed = &content[self.display_cursor..];
        if let Some(nl_offset) = unrevealed.find('\n') {
            self.display_cursor += nl_offset + 1;
            true
        } else {
            // No newline found — don't advance (wait for more content or finalization)
            false
        }
    }

    /// Advance display cursor by up to `n` lines.
    fn advance_n_lines(&mut self, content: &str, n: i32) -> bool {
        let start = self.display_cursor;
        let mut pos = start;
        for _ in 0..n {
            let remaining = &content[pos..];
            if let Some(nl_offset) = remaining.find('\n') {
                pos += nl_offset + 1;
            } else {
                break;
            }
        }
        if pos > start {
            self.display_cursor = pos;
            true
        } else {
            false
        }
    }
}

/// Count newlines in a string slice.
fn count_newlines(s: &str) -> i32 {
    s.bytes().filter(|&b| b == b'\n').count() as i32
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
