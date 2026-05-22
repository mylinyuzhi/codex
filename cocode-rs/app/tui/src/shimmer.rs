//! Shimmer loading effect.
//!
//! Creates a sweeping highlight band across text, used during thinking
//! and loading states. The band sweeps left-to-right on a 2-second cycle.
//!
//! - **Truecolor**: blends foreground toward background at 90% for highlights
//! - **Fallback**: uses DIM/normal/BOLD for non-truecolor terminals

use std::sync::OnceLock;
use std::time::Instant;

use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Span;

use crate::terminal_palette;

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

fn elapsed_since_start() -> std::time::Duration {
    let start = PROCESS_START.get_or_init(Instant::now);
    start.elapsed()
}

/// Generate spans with a sweeping shimmer highlight effect.
///
/// Each character gets an individually styled span. The highlight band
/// sweeps across the text on a 2-second cycle using cosine falloff.
pub fn shimmer_spans(text: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let padding = 10_usize;
    let period = chars.len() + padding * 2;
    let sweep_seconds = 2.0_f32;
    let pos_f =
        (elapsed_since_start().as_secs_f32() % sweep_seconds) / sweep_seconds * (period as f32);
    let pos = pos_f as usize;
    let has_truecolor = terminal_palette::has_truecolor();
    let band_half_width = 5.0_f32;

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(chars.len());

    for (i, ch) in chars.iter().enumerate() {
        let i_pos = i as isize + padding as isize;
        let pos = pos as isize;
        let dist = (i_pos - pos).unsigned_abs() as f32;

        let intensity = if dist <= band_half_width {
            let x = std::f32::consts::PI * (dist / band_half_width);
            0.5 * (1.0 + x.cos())
        } else {
            0.0
        };

        let style = if has_truecolor {
            truecolor_style(intensity)
        } else {
            fallback_style(intensity)
        };

        spans.push(Span::styled(ch.to_string(), style));
    }
    spans
}

/// Truecolor: blend between dim base and bright highlight.
fn truecolor_style(intensity: f32) -> Style {
    let base = terminal_palette::detect_bg().unwrap_or((128, 128, 128));
    let highlight = (255, 255, 255);
    let t = intensity.clamp(0.0, 1.0) * 0.9;
    let (r, g, b) = terminal_palette::blend(highlight, base, t);
    // Allow custom RGB: intentionally adjusting level of terminal colors
    #[allow(clippy::disallowed_methods)]
    {
        Style::default()
            .fg(ratatui::style::Color::Rgb(r, g, b))
            .add_modifier(Modifier::BOLD)
    }
}

/// Fallback for non-truecolor: DIM / normal / BOLD based on intensity.
fn fallback_style(intensity: f32) -> Style {
    if intensity < 0.2 {
        Style::default().add_modifier(Modifier::DIM)
    } else if intensity < 0.6 {
        Style::default()
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
#[path = "shimmer.test.rs"]
mod tests;
