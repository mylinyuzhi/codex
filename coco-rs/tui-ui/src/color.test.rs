// This module tests color quantization, so it constructs Color::Rgb inputs and
// asserts Color::Indexed outputs directly.
#![allow(clippy::disallowed_methods)]

use pretty_assertions::assert_eq;

use super::ColorCapability;
use super::adapt_color;
use super::detect_from_env;
use super::rgb_to_xterm256;
use ratatui::style::Color;

#[test]
fn test_rgb_to_xterm256_pure_black_maps_to_cube_origin() {
    assert_eq!(rgb_to_xterm256(0, 0, 0), 16);
}

#[test]
fn test_rgb_to_xterm256_pure_white_maps_to_cube_max() {
    assert_eq!(rgb_to_xterm256(255, 255, 255), 231);
}

#[test]
fn test_rgb_to_xterm256_mid_gray_prefers_grayscale_ramp() {
    // 128,128,128 sits exactly on a grayscale-ramp value (244) and off the cube.
    assert_eq!(rgb_to_xterm256(128, 128, 128), 244);
}

#[test]
fn test_rgb_to_xterm256_saturated_red_maps_to_cube() {
    assert_eq!(rgb_to_xterm256(255, 0, 0), 196);
}

#[test]
fn test_adapt_color_downsamples_rgb_only_on_ansi256() {
    assert_eq!(
        adapt_color(Color::Rgb(255, 0, 0), ColorCapability::Ansi256),
        Color::Indexed(196)
    );
    assert_eq!(
        adapt_color(Color::Rgb(255, 0, 0), ColorCapability::TrueColor),
        Color::Rgb(255, 0, 0)
    );
}

#[test]
fn test_adapt_color_passes_non_rgb_through() {
    assert_eq!(
        adapt_color(Color::Red, ColorCapability::Ansi256),
        Color::Red
    );
    assert_eq!(
        adapt_color(Color::Indexed(42), ColorCapability::Ansi256),
        Color::Indexed(42)
    );
}

#[test]
fn test_detect_from_env_truecolor_markers() {
    assert_eq!(
        detect_from_env(Some("truecolor")),
        ColorCapability::TrueColor
    );
    assert_eq!(detect_from_env(Some("24bit")), ColorCapability::TrueColor);
    assert_eq!(
        detect_from_env(Some("TrueColor")),
        ColorCapability::TrueColor
    );
}

#[test]
fn test_detect_from_env_defaults_to_ansi256() {
    assert_eq!(detect_from_env(Some("")), ColorCapability::Ansi256);
    assert_eq!(detect_from_env(Some("256color")), ColorCapability::Ansi256);
    assert_eq!(detect_from_env(None), ColorCapability::Ansi256);
}
