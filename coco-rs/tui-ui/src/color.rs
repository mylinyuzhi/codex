//! Terminal color-capability detection and truecolor→xterm-256 downsampling.
//!
//! Absorbed from jcode's `jcode-tui-style` color handling: terminals without
//! 24-bit color render `Color::Rgb` poorly (or not at all), so we detect the
//! capability once and quantize RGB to the nearest xterm-256 palette index when
//! truecolor is unavailable. Quantization picks the closer of the 6×6×6 color
//! cube and the 24-step grayscale ramp under a green-weighted distance.

use std::sync::OnceLock;

use ratatui::style::Color;

/// The terminal's color depth, detected once from the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColorCapability {
    /// 24-bit truecolor (`Color::Rgb` passes through unchanged).
    TrueColor,
    /// 256-color palette (`Color::Rgb` is downsampled to `Color::Indexed`).
    Ansi256,
}

/// Detected color capability, cached for the process lifetime.
pub fn color_capability() -> ColorCapability {
    static CAP: OnceLock<ColorCapability> = OnceLock::new();
    *CAP.get_or_init(|| detect_from_env(std::env::var("COLORTERM").ok().as_deref()))
}

fn detect_from_env(colorterm: Option<&str>) -> ColorCapability {
    match colorterm {
        Some(value) => {
            let value = value.to_ascii_lowercase();
            if value.contains("truecolor") || value.contains("24bit") {
                ColorCapability::TrueColor
            } else {
                ColorCapability::Ansi256
            }
        }
        None => ColorCapability::Ansi256,
    }
}

/// Adapt a color to the given capability: pass truecolor through, otherwise
/// downsample `Color::Rgb` to the nearest xterm-256 index. Non-RGB colors
/// (named, already-indexed, reset) are returned unchanged.
#[allow(clippy::disallowed_methods)] // this IS the downsampler that produces palette indices
pub fn adapt_color(color: Color, capability: ColorCapability) -> Color {
    match (capability, color) {
        (ColorCapability::Ansi256, Color::Rgb(r, g, b)) => Color::Indexed(rgb_to_xterm256(r, g, b)),
        _ => color,
    }
}

/// Map a 24-bit RGB triple to the nearest xterm-256 palette index, choosing the
/// closer of the 6×6×6 color cube (indices 16–231) and the grayscale ramp
/// (232–255) under a green-weighted squared distance.
pub fn rgb_to_xterm256(r: u8, g: u8, b: u8) -> u8 {
    const CUBE_STEPS: [i32; 6] = [0, 95, 135, 175, 215, 255];

    fn nearest_cube_index(v: i32) -> usize {
        let mut best = 0usize;
        let mut best_dist = i32::MAX;
        for (i, &step) in CUBE_STEPS.iter().enumerate() {
            let dist = (v - step).abs();
            if dist < best_dist {
                best_dist = dist;
                best = i;
            }
        }
        best
    }

    // Eye is most sensitive to green; weight the channels accordingly.
    fn weighted_dist(a: (i32, i32, i32), b: (i32, i32, i32)) -> i32 {
        2 * (a.0 - b.0).pow(2) + 4 * (a.1 - b.1).pow(2) + 3 * (a.2 - b.2).pow(2)
    }

    let (r, g, b) = (r as i32, g as i32, b as i32);

    // Candidate 1: color cube.
    let (ri, gi, bi) = (
        nearest_cube_index(r),
        nearest_cube_index(g),
        nearest_cube_index(b),
    );
    let cube_index = 16 + 36 * ri + 6 * gi + bi;
    let cube_rgb = (CUBE_STEPS[ri], CUBE_STEPS[gi], CUBE_STEPS[bi]);

    // Candidate 2: grayscale ramp (232..=255 → values 8, 18, …, 238).
    let gray_level = ((r + g + b) / 3 - 8).clamp(0, 230);
    let gray_step = ((gray_level + 5) / 10).min(23);
    let gray_value = 8 + 10 * gray_step;
    let gray_index = 232 + gray_step;
    let gray_rgb = (gray_value, gray_value, gray_value);

    let target = (r, g, b);
    if weighted_dist(target, gray_rgb) < weighted_dist(target, cube_rgb) {
        gray_index as u8
    } else {
        cube_index as u8
    }
}

#[cfg(test)]
#[path = "color.test.rs"]
mod tests;
