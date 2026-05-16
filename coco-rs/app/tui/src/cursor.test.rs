use pretty_assertions::assert_eq;
use ratatui::layout::Rect;

use super::*;
use crate::state::AppState;
use crate::state::CommandPaletteOverlay;
use crate::state::Overlay;

/// Helper: build a minimal AppState with input focused and given text.
fn make_state(text: &str) -> AppState {
    let mut state = AppState::default();
    state.ui.focus = FocusTarget::Input;
    if !text.is_empty() {
        state.ui.input.textarea.insert_str(text);
    }
    state
}

const INPUT_AREA: Rect = Rect {
    x: 0,
    y: 10,
    width: 80,
    height: 3,
};

#[test]
fn compute_cursor_returns_none_when_input_not_focused() {
    let mut state = AppState::default();
    state.ui.focus = FocusTarget::Chat;
    assert!(compute_cursor(&state, INPUT_AREA).is_none());
}

#[test]
fn compute_cursor_returns_some_for_empty_input_when_focused() {
    // This is the regression: empty input must still return a claim so
    // the post-draw pin has somewhere to put the cursor; otherwise
    // focus-gained re-shows it in the status bar.
    let state = make_state("");
    let claim = compute_cursor(&state, INPUT_AREA).expect("empty input must claim cursor");
    // indicator_width=2 → cursor sits in column 2, second row of input area.
    assert_eq!(claim.position.x, INPUT_AREA.x + 2);
    assert_eq!(claim.position.y, INPUT_AREA.y + 1);
}

#[test]
fn compute_cursor_returns_none_when_modal_overlay_owns_focus() {
    let mut state = make_state("hello");
    state.ui.set_overlay(Overlay::Help);

    assert!(compute_cursor(&state, INPUT_AREA).is_none());
}

#[test]
fn compute_cursor_still_tracks_command_palette_filter() {
    let mut state = make_state("");
    state
        .ui
        .set_overlay(Overlay::CommandPalette(CommandPaletteOverlay {
            commands: Vec::new(),
            filter: "help".to_string(),
            selected: 0,
        }));

    let claim = compute_cursor(&state, INPUT_AREA).expect("command palette uses input mirror");
    assert_eq!(claim.position.x, INPUT_AREA.x + 2 + 1 + 4);
    assert_eq!(claim.position.y, INPUT_AREA.y + 1);
}

#[test]
fn compute_cursor_advances_past_ascii_text() {
    let state = make_state("hello");
    let claim = compute_cursor(&state, INPUT_AREA).unwrap();
    assert_eq!(claim.position.x, INPUT_AREA.x + 2 + 5);
    assert_eq!(claim.position.y, INPUT_AREA.y + 1);
}

#[test]
fn compute_cursor_handles_cjk_width() {
    // "你好" is 4 display columns, not 2.
    let state = make_state("你好");
    let claim = compute_cursor(&state, INPUT_AREA).unwrap();
    assert_eq!(claim.position.x, INPUT_AREA.x + 2 + 4);
}

#[test]
fn compute_cursor_returns_none_for_zero_sized_area() {
    let state = make_state("hi");
    let zero = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };
    assert!(compute_cursor(&state, zero).is_none());
}

#[test]
fn compute_cursor_clamps_to_area_width() {
    let state = make_state(&"x".repeat(200));
    let claim = compute_cursor(&state, INPUT_AREA).unwrap();
    // max_cursor = width - (indicator_width + 1) = 80 - 3 = 77
    // cursor_x = area.x + indicator_width + min(raw, 77) = 0 + 2 + 77 = 79
    assert_eq!(claim.position.x, INPUT_AREA.x + 2 + 77);
}
