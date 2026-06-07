use crossterm::Command as _;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use ratatui::layout::Size;

use super::*;

#[test]
fn native_viewport_flows_after_history_before_screen_fills() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 3,
            Size::new(80, 24),
            /*desired_height*/ 6
        ),
        Rect::new(0, 3, 80, 6)
    );
}

#[test]
fn native_viewport_bottom_pins_once_history_reaches_terminal_bottom() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 22,
            Size::new(80, 24),
            /*desired_height*/ 6
        ),
        Rect::new(0, 18, 80, 6)
    );
}

#[test]
fn native_viewport_pin_state_keeps_later_height_changes_bottom_pinned() {
    let size = Size::new(80, 24);
    let first = native_viewport_geometry_with_max(
        /*anchor_y*/ 22,
        size,
        /*desired_height*/ 4,
        NATIVE_VIEWPORT_MAX_HEIGHT,
        NativeViewportPin::Flowing,
    );
    let grown = native_viewport_geometry_with_max(
        /*anchor_y*/ 20,
        size,
        /*desired_height*/ 10,
        NATIVE_VIEWPORT_MAX_HEIGHT,
        first.pin,
    );

    assert_eq!(first.area.bottom(), 24);
    assert_eq!(grown.area.bottom(), 24);
    assert_eq!(grown.pin, NativeViewportPin::BottomPinned);
    assert!(
        grown.area.top() < first.area.top(),
        "after pinning, larger live surfaces grow upward from the terminal bottom"
    );
}

#[test]
fn interactive_viewport_max_height_grows_for_active_prompt() {
    use crate::state::PanePromptState;
    use crate::state::PlanEntryPromptState;
    let mut state = crate::state::AppState::new();
    // No prompt: the streaming/idle cap.
    assert_eq!(
        interactive_viewport_max_height(&state, 60),
        NATIVE_VIEWPORT_MAX_HEIGHT
    );
    // Active prompt: grows to nearly the full screen so all options fit.
    state
        .ui
        .push_prompt(PanePromptState::PlanEntry(PlanEntryPromptState {
            description: "x".into(),
        }));
    assert_eq!(
        interactive_viewport_max_height(&state, 60),
        60 - NATIVE_VIEWPORT_MIN_HEIGHT
    );
    // Never below the normal cap; clamped to the screen on tiny terminals.
    assert_eq!(interactive_viewport_max_height(&state, 10), 10);
}

#[test]
fn native_viewport_clamps_to_small_terminal_height() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 10,
            Size::new(80, 3),
            /*desired_height*/ 12
        ),
        Rect::new(0, 0, 80, 3)
    );
}

#[test]
fn native_viewport_handles_zero_height() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 10,
            Size::new(80, 0),
            /*desired_height*/ 12
        ),
        Rect::new(0, 0, 80, 0)
    );
}

#[test]
fn native_viewport_uses_minimum_height_for_idle_composer() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 2,
            Size::new(80, 24),
            /*desired_height*/ 1
        ),
        Rect::new(0, 2, 80, 4)
    );
}

#[test]
fn native_viewport_bottom_pin_uses_terminal_bottom_after_latch() {
    assert_eq!(
        native_viewport_geometry_with_max(
            /*anchor_y*/ 8,
            Size::new(80, 40),
            /*desired_height*/ 4,
            NATIVE_VIEWPORT_MAX_HEIGHT,
            NativeViewportPin::BottomPinned,
        )
        .area,
        Rect::new(0, 36, 80, 4)
    );
}

#[test]
fn native_viewport_caps_to_native_max_height() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 0,
            Size::new(80, 80),
            /*desired_height*/ 80
        )
        .height,
        NATIVE_VIEWPORT_MAX_HEIGHT
    );
}

#[test]
fn bottom_pinned_shrink_commits_only_backed_rows() {
    let commit = commit_native_viewport_geometry(
        NativeViewportPin::BottomPinned,
        Rect::new(0, 8, 80, 16),
        Rect::new(0, 20, 80, 4),
        /*history_bottom_y_before*/ 8,
        /*terminal_height*/ 24,
        /*history_tail_reveal_rows*/ 3,
        /*expected_append_rows*/ 4,
    );

    assert_eq!(commit.shrink_requested_rows, 12);
    assert_eq!(commit.shrink_committed_rows, 7);
    assert_eq!(commit.reveal_tail_rows, 3);
    assert_eq!(commit.append_fill_rows, 4);
    assert_eq!(commit.shrink_deferred_rows, 5);
    assert_eq!(commit.committed_viewport, Rect::new(0, 15, 80, 9));
}

#[test]
fn bottom_pinned_shrink_uses_append_rows_after_tail_reveal() {
    let commit = commit_native_viewport_geometry(
        NativeViewportPin::BottomPinned,
        Rect::new(0, 12, 80, 12),
        Rect::new(0, 20, 80, 4),
        /*history_bottom_y_before*/ 12,
        /*terminal_height*/ 24,
        /*history_tail_reveal_rows*/ 2,
        /*expected_append_rows*/ 6,
    );

    assert_eq!(commit.shrink_requested_rows, 8);
    assert_eq!(commit.shrink_committed_rows, 8);
    assert_eq!(commit.reveal_tail_rows, 2);
    assert_eq!(commit.append_fill_rows, 6);
    assert_eq!(commit.shrink_deferred_rows, 0);
    assert_eq!(commit.committed_viewport, commit.desired_viewport);
}

#[test]
fn bottom_pinned_shrink_defers_when_history_is_insufficient() {
    let commit = commit_native_viewport_geometry(
        NativeViewportPin::BottomPinned,
        Rect::new(0, 10, 80, 14),
        Rect::new(0, 20, 80, 4),
        /*history_bottom_y_before*/ 10,
        /*terminal_height*/ 24,
        /*history_tail_reveal_rows*/ 0,
        /*expected_append_rows*/ 0,
    );

    assert_eq!(commit.shrink_requested_rows, 10);
    assert_eq!(commit.shrink_committed_rows, 0);
    assert_eq!(commit.shrink_deferred_rows, 10);
    assert_eq!(commit.committed_viewport, Rect::new(0, 10, 80, 14));
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
