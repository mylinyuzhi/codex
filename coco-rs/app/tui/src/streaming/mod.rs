//! Streaming display with adaptive chunking.
//!
//! Manages the display pacing of streamed LLM output using a two-gear
//! system: smooth (1 line/tick) for normal flow, catch-up (N lines/tick)
//! when the backlog grows.

pub mod chunking;

use std::time::Instant;

use chunking::AdaptiveChunking;

/// Streaming display state with adaptive pacing.
pub struct StreamDisplay {
    /// Byte offset into content that has been displayed.
    display_cursor: usize,
    /// Adaptive chunking policy.
    chunking: AdaptiveChunking,
    /// When unrevealed content first appeared.
    pending_since: Option<Instant>,
}

impl StreamDisplay {
    /// Create a new stream display.
    pub fn new() -> Self {
        Self {
            display_cursor: 0,
            chunking: AdaptiveChunking::new(),
            pending_since: None,
        }
    }

    /// Current display cursor position.
    pub fn cursor(&self) -> usize {
        self.display_cursor
    }

    /// Notify that new content was appended.
    pub fn on_content_appended(&mut self, total_len: usize) {
        if total_len > self.display_cursor && self.pending_since.is_none() {
            self.pending_since = Some(Instant::now());
        }
    }

    /// Advance display cursor (called on SpinnerTick). Returns true if changed.
    pub fn advance(&mut self, content: &str) -> bool {
        if self.display_cursor >= content.len() {
            self.pending_since = None;
            return false;
        }

        let queue_depth = content[self.display_cursor..]
            .chars()
            .filter(|c| *c == '\n')
            .count();
        let age = self.pending_since.map(|t| t.elapsed());

        let lines_to_advance = self.chunking.plan(queue_depth, age);

        if lines_to_advance == 0 {
            return false;
        }

        let remaining = &content[self.display_cursor..];
        let mut advanced = 0;
        let mut lines_done = 0;

        for (i, c) in remaining.char_indices() {
            advanced = i + c.len_utf8();
            if c == '\n' {
                lines_done += 1;
                if lines_done >= lines_to_advance {
                    break;
                }
            }
        }

        // If no newline found, advance to end
        if lines_done == 0 {
            advanced = remaining.len();
        }

        self.display_cursor += advanced;

        if self.display_cursor >= content.len() {
            self.pending_since = None;
        }

        true
    }

    /// Reveal all content immediately.
    pub fn reveal_all(&mut self, total_len: usize) {
        self.display_cursor = total_len;
        self.pending_since = None;
    }
}

impl Default for StreamDisplay {
    fn default() -> Self {
        Self::new()
    }
}
