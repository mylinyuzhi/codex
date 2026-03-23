//! Terminal palette detection and color utilities.
//!
//! Detects terminal background color and truecolor support at startup.
//! Provides color blending and adaptive background tinting for user messages.

use std::sync::OnceLock;

use ratatui::style::Color;

/// Cached terminal background detection result.
static TERMINAL_BG: OnceLock<Option<(u8, u8, u8)>> = OnceLock::new();

/// Cached truecolor support detection.
static HAS_TRUECOLOR: OnceLock<bool> = OnceLock::new();

/// Cached 256-color support detection.
static HAS_256_COLORS: OnceLock<bool> = OnceLock::new();

/// Detect the terminal background color.
///
/// Tries `COLORFGBG` env var (format: "fg;bg" where bg is ANSI index).
/// Returns `None` if detection fails or env is unset.
pub fn detect_bg() -> Option<(u8, u8, u8)> {
    *TERMINAL_BG.get_or_init(detect_bg_inner)
}

/// Check if the terminal supports 24-bit truecolor.
pub fn has_truecolor() -> bool {
    *HAS_TRUECOLOR.get_or_init(|| {
        std::env::var("COLORTERM")
            .map(|v| v == "truecolor" || v == "24bit")
            .unwrap_or(false)
    })
}

/// Check if the terminal supports 256 colors.
pub fn has_256_colors() -> bool {
    *HAS_256_COLORS.get_or_init(|| {
        std::env::var("TERM")
            .map(|v| v.contains("256color"))
            .unwrap_or(false)
    })
}

/// Check if a background color is light (luminance > 0.5).
pub fn is_light(bg: (u8, u8, u8)) -> bool {
    // Relative luminance (simplified sRGB)
    let (r, g, b) = bg;
    let luminance =
        0.2126 * (r as f32 / 255.0) + 0.7152 * (g as f32 / 255.0) + 0.0722 * (b as f32 / 255.0);
    luminance > 0.5
}

/// Blend two RGB colors: `result = top * alpha + base * (1 - alpha)`.
pub fn blend(top: (u8, u8, u8), base: (u8, u8, u8), alpha: f32) -> (u8, u8, u8) {
    let alpha = alpha.clamp(0.0, 1.0);
    let inv = 1.0 - alpha;
    (
        (top.0 as f32 * alpha + base.0 as f32 * inv) as u8,
        (top.1 as f32 * alpha + base.1 as f32 * inv) as u8,
        (top.2 as f32 * alpha + base.2 as f32 * inv) as u8,
    )
}

/// Compute a subtle background tint for user messages.
///
/// - Dark terminal: white at 12% opacity → slight brightening
/// - Light terminal: black at 4% opacity → slight darkening
///
/// Returns `None` if terminal background is unknown.
pub fn user_message_bg() -> Option<Color> {
    let bg = detect_bg()?;
    let (top, alpha) = if is_light(bg) {
        ((0, 0, 0), 0.04)
    } else {
        ((255, 255, 255), 0.12)
    };
    let (r, g, b) = blend(top, bg, alpha);
    if has_truecolor() {
        // Allow custom RGB: we are deliberately adjusting the terminal's own
        // background color by a small opacity offset, not introducing arbitrary colors.
        #[allow(clippy::disallowed_methods)]
        Some(Color::Rgb(r, g, b))
    } else {
        // For non-truecolor, a subtle tint isn't possible — skip
        None
    }
}

/// Convert ANSI color index (0-15) to approximate RGB.
fn ansi_index_to_rgb(idx: u8) -> Option<(u8, u8, u8)> {
    match idx {
        0 => Some((0, 0, 0)),        // Black
        1 => Some((170, 0, 0)),      // Red
        2 => Some((0, 170, 0)),      // Green
        3 => Some((170, 85, 0)),     // Yellow/Brown
        4 => Some((0, 0, 170)),      // Blue
        5 => Some((170, 0, 170)),    // Magenta
        6 => Some((0, 170, 170)),    // Cyan
        7 => Some((170, 170, 170)),  // White
        8 => Some((85, 85, 85)),     // Bright Black
        9 => Some((255, 85, 85)),    // Bright Red
        10 => Some((85, 255, 85)),   // Bright Green
        11 => Some((255, 255, 85)),  // Bright Yellow
        12 => Some((85, 85, 255)),   // Bright Blue
        13 => Some((255, 85, 255)),  // Bright Magenta
        14 => Some((85, 255, 255)),  // Bright Cyan
        15 => Some((255, 255, 255)), // Bright White
        _ => None,
    }
}

fn detect_bg_inner() -> Option<(u8, u8, u8)> {
    // COLORFGBG format: "fg;bg" where values are ANSI color indices
    let val = std::env::var("COLORFGBG").ok()?;
    let bg_str = val.rsplit(';').next()?;
    let idx: u8 = bg_str.parse().ok()?;
    ansi_index_to_rgb(idx)
}

#[cfg(test)]
#[path = "terminal_palette.test.rs"]
mod tests;
