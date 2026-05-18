use crossterm::Command as _;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use ratatui::layout::Size;

use super::*;

#[test]
fn native_viewport_uses_anchor_when_it_fits() {
    assert_eq!(
        native_viewport_area(/*anchor_y*/ 3, Size::new(80, 24), 6),
        Rect::new(0, 3, 80, 6)
    );
}

#[test]
fn native_viewport_clamps_to_small_terminal_height() {
    assert_eq!(
        native_viewport_area(/*anchor_y*/ 10, Size::new(80, 3), 12),
        Rect::new(0, 0, 80, 3)
    );
}

#[test]
fn native_viewport_handles_zero_height() {
    assert_eq!(
        native_viewport_area(/*anchor_y*/ 10, Size::new(80, 0), 12),
        Rect::new(0, 0, 80, 0)
    );
}

#[test]
fn native_viewport_uses_minimum_height_for_idle_composer() {
    assert_eq!(
        native_viewport_area(/*anchor_y*/ 2, Size::new(80, 24), 1),
        Rect::new(0, 2, 80, 4)
    );
}

#[test]
fn native_viewport_moves_up_only_when_anchor_would_overflow() {
    assert_eq!(
        native_viewport_area(/*anchor_y*/ 22, Size::new(80, 24), 6),
        Rect::new(0, 18, 80, 6)
    );
}

#[test]
fn native_viewport_anchors_to_history_bottom_not_stale_viewport_top() {
    assert_eq!(
        native_viewport_area(/*anchor_y*/ 8, Size::new(80, 40), 4),
        Rect::new(0, 8, 80, 4)
    );
}

#[test]
fn native_viewport_caps_to_native_max_height() {
    assert_eq!(
        native_viewport_area(/*anchor_y*/ 0, Size::new(80, 80), 80).height,
        NATIVE_VIEWPORT_MAX_HEIGHT
    );
}

#[test]
fn alternate_scroll_commands_emit_xterm_private_mode_bytes() {
    let mut enabled = String::new();
    EnableAlternateScroll
        .write_ansi(&mut enabled)
        .expect("write enable bytes");
    assert_eq!(enabled, "\x1b[?1007h");

    let mut disabled = String::new();
    DisableAlternateScroll
        .write_ansi(&mut disabled)
        .expect("write disable bytes");
    assert_eq!(disabled, "\x1b[?1007l");
}
