use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use ratatui::layout::Size;

use super::*;

#[test]
fn native_viewport_is_pinned_to_terminal_bottom() {
    assert_eq!(
        native_viewport_area(Size::new(80, 24), 6),
        Rect::new(0, 18, 80, 6)
    );
}

#[test]
fn native_viewport_clamps_to_small_terminal_height() {
    assert_eq!(
        native_viewport_area(Size::new(80, 3), 12),
        Rect::new(0, 0, 80, 3)
    );
}

#[test]
fn native_viewport_handles_zero_height() {
    assert_eq!(
        native_viewport_area(Size::new(80, 0), 12),
        Rect::new(0, 0, 80, 0)
    );
}

#[test]
fn native_viewport_uses_minimum_height_for_idle_composer() {
    assert_eq!(
        native_viewport_area(Size::new(80, 24), 1),
        Rect::new(0, 20, 80, 4)
    );
}
