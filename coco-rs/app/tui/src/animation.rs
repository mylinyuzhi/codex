//! Spinner and shimmer animation utilities.

use std::time::Instant;

use ratatui::style::Color;
use ratatui::style::Style;

/// Braille-pattern spinner frames (smooth rotation).
const BRAILLE_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Dot spinner frames.
const DOTS_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Line spinner frames.
const LINE_FRAMES: &[&str] = &["|", "/", "-", "\\"];

/// Bounce spinner frames.
const BOUNCE_FRAMES: &[&str] = &["⠁", "⠂", "⠄", "⠂"];

/// Arrow spinner frames.
const ARROW_FRAMES: &[&str] = &["←", "↖", "↑", "↗", "→", "↘", "↓", "↙"];

/// Available spinner animation styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpinnerStyle {
    /// Braille pattern rotation (default).
    Braille,
    /// Dot pattern rotation.
    Dots,
    /// Classic line spinner (`|/-\`).
    Line,
    /// Bouncing dot.
    Bounce,
    /// Rotating arrow.
    Arrow,
}

impl SpinnerStyle {
    /// Returns the frame set and interval (ms) for this style.
    fn frames(self) -> &'static [&'static str] {
        match self {
            Self::Braille => BRAILLE_FRAMES,
            Self::Dots => DOTS_FRAMES,
            Self::Line => LINE_FRAMES,
            Self::Bounce => BOUNCE_FRAMES,
            Self::Arrow => ARROW_FRAMES,
        }
    }

    /// Frame interval in milliseconds per style.
    fn interval_ms(self) -> i64 {
        match self {
            Self::Braille | Self::Dots => 80,
            Self::Line => 120,
            Self::Bounce => 150,
            Self::Arrow => 100,
        }
    }
}

/// Animation state tracking.
#[derive(Debug)]
pub struct Animation {
    started_at: Instant,
}

impl Animation {
    /// Create a new animation tracker.
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
        }
    }

    /// Elapsed time in milliseconds as i64.
    fn elapsed_ms(&self) -> i64 {
        self.started_at.elapsed().as_millis() as i64
    }

    /// Get the current spinner frame based on elapsed time (braille default).
    pub fn spinner_frame(&self) -> &'static str {
        self.spinner_frame_style(SpinnerStyle::Braille)
    }

    /// Get the current spinner frame for the given style.
    pub fn spinner_frame_style(&self, style: SpinnerStyle) -> &'static str {
        let frames = style.frames();
        let interval = style.interval_ms();
        let idx = ((self.elapsed_ms() / interval) % frames.len() as i64) as usize;
        frames[idx]
    }

    /// Returns a shimmer brightness alpha (0.0..1.0) for the given character
    /// offset. Produces a wave effect when applied across adjacent characters.
    pub fn shimmer_alpha(&self, offset: i32) -> f64 {
        shimmer_alpha(self.elapsed_ms(), offset)
    }
}

impl Default for Animation {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Standalone animation functions
// ---------------------------------------------------------------------------

/// Compute shimmer brightness for a character at `offset` given `elapsed_ms`.
/// Returns a value in 0.0..=1.0 representing brightness.
fn shimmer_alpha(elapsed_ms: i64, offset: i32) -> f64 {
    // Wave period ~1200ms, phase shifted by offset.
    let phase = (elapsed_ms as f64 / 1200.0) * std::f64::consts::TAU + (offset as f64 * 0.6);
    // Map sin(-1..1) to 0..1.
    (phase.sin() + 1.0) / 2.0
}

/// Map a 0.0..1.0 alpha to a grayscale `Color`.
fn alpha_to_gray(alpha: f64) -> Color {
    let clamped = alpha.clamp(0.0, 1.0);
    let v = (clamped * 255.0) as u8;
    Color::Rgb(v, v, v)
}

/// Returns `(base_char, Style)` with brightness cycling for loading text.
///
/// `offset` shifts the wave phase so adjacent characters shimmer in sequence.
pub fn shimmer_char(base_char: char, elapsed_ms: i64, offset: i32) -> (char, Style) {
    let alpha = shimmer_alpha(elapsed_ms, offset);
    let style = Style::default().fg(alpha_to_gray(alpha));
    (base_char, style)
}

/// Returns a `Style` that cycles between bright and dim for a streaming cursor.
pub fn glimmer_style(elapsed_ms: i64) -> Style {
    // Pulse period ~800ms.
    let phase = (elapsed_ms as f64 / 800.0) * std::f64::consts::TAU;
    let alpha = (phase.sin() + 1.0) / 2.0;
    // Bias toward brighter end: remap 0..1 to 0.3..1.0.
    let brightness = 0.3 + alpha * 0.7;
    Style::default().fg(alpha_to_gray(brightness))
}

#[cfg(test)]
#[path = "animation.test.rs"]
mod tests;
