//! Multi-variant animation system.
//!
//! Provides time-based frame selection across multiple animation variants.
//! The animation automatically cycles through frames based on elapsed time
//! rather than an external frame counter.

use std::time::Duration;
use std::time::Instant;

/// Frame tick interval (how often the animation advances).
const FRAME_TICK: Duration = Duration::from_millis(80);

/// Available animation variants.
const VARIANTS: &[&[&str]] = &[
    // 0: Braille (default)
    &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"],
    // 1: Dots
    &["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈"],
    // 2: Blocks
    &["▖", "▘", "▝", "▗"],
    // 3: Moon phases
    &["🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"],
    // 4: Line bounce
    &["⠁", "⠉", "⠋", "⠛", "⠟", "⠿", "⠟", "⠛", "⠋", "⠉"],
];

/// A time-based animation with multiple visual variants.
#[derive(Debug, Clone)]
pub struct Animation {
    variant_idx: i32,
    start: Instant,
}

impl Default for Animation {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Animation {
    /// Create a new animation with the specified variant index.
    pub fn new(variant: i32) -> Self {
        let clamped = variant.clamp(0, VARIANTS.len() as i32 - 1);
        Self {
            variant_idx: clamped,
            start: Instant::now(),
        }
    }

    /// Get the current animation frame based on elapsed time.
    pub fn current_frame(&self) -> &'static str {
        let frames = VARIANTS[self.variant_idx as usize];
        if frames.is_empty() {
            return "";
        }
        let tick_ms = FRAME_TICK.as_millis();
        if tick_ms == 0 {
            return frames[0];
        }
        let elapsed_ms = self.start.elapsed().as_millis();
        let idx = ((elapsed_ms / tick_ms) % frames.len() as u128) as usize;
        frames[idx]
    }

    /// Get the number of available variants.
    pub fn variant_count() -> i32 {
        VARIANTS.len() as i32
    }

    /// Switch to the next variant (wrapping around).
    pub fn next_variant(&mut self) {
        self.variant_idx = (self.variant_idx + 1) % VARIANTS.len() as i32;
    }
}

#[cfg(test)]
#[path = "animation.test.rs"]
mod tests;
