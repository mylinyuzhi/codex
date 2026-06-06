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
fn streaming_height_floor_is_grow_only_while_streaming() {
    // While streaming, the height grows but never shrinks (high-water), so the
    // live-tail viewport stops oscillating as lines grow then commit to
    // scrollback. Applied on every terminal — DEC 2026 makes each frame atomic
    // but cannot stop consecutive frames from differing in height.
    assert_eq!(
        streaming_height_floor(/*desired*/ 5, /*high_water*/ 0, true),
        (5, 5)
    );
    assert_eq!(
        streaming_height_floor(/*desired*/ 7, /*high_water*/ 5, true),
        (7, 7)
    );
    // A shrink to 4 is suppressed; the watermark holds at 7.
    assert_eq!(
        streaming_height_floor(/*desired*/ 4, /*high_water*/ 7, true),
        (7, 7)
    );
}

#[test]
fn streaming_height_floor_passes_through_when_idle() {
    // Not streaming: passthrough and reset the watermark so the viewport can
    // relax back to its natural size on the next frame.
    assert_eq!(
        streaming_height_floor(/*desired*/ 4, /*high_water*/ 7, false),
        (4, 0)
    );
}

#[test]
fn streaming_height_freeze_spans_the_whole_turn() {
    use crate::state::PanePromptState;
    use crate::state::PlanEntryPromptState;
    use std::time::Instant;
    let mut state = crate::state::AppState::new();
    // Idle: no freeze, the viewport relaxes to its natural height.
    assert!(!streaming_height_freeze(&state));
    // Mid-turn but NOT streaming (e.g. a tool is running, or just after an
    // assistant message committed): still frozen. This is the case the old
    // `is_streaming()` gate missed — `is_streaming()` flips off at every tool
    // call and message boundary while `turn_active()` stays true, so the floor
    // used to reset mid-turn and the input bar bounced.
    state.ui.ephemeral.start_turn("Working", Instant::now());
    assert!(!state.is_streaming());
    assert!(
        streaming_height_freeze(&state),
        "frozen for the whole active turn, not just streaming spans"
    );
    // An interactive prompt sizes to content — exempt so it can shrink as the
    // user navigates options.
    state
        .ui
        .push_prompt(PanePromptState::PlanEntry(PlanEntryPromptState {
            description: "x".into(),
        }));
    assert!(
        !streaming_height_freeze(&state),
        "interactive prompt is exempt from the freeze"
    );
}

#[test]
fn streaming_height_floor_holds_watermark_across_midturn_toggle() {
    // Drive the floor across a multi-frame turn, feeding next_high_water back in
    // each frame — the real per-frame loop. While `freeze` holds (the whole
    // turn), the height must stay monotonic non-decreasing even when `desired`
    // dips at a tool-call / assistant-message boundary; that dip — desired
    // collapsing while the turn keeps running — is the bottom-bar bounce being
    // fixed. desired = [5, 8, 4, 5]: live tail grows to 8, a boundary collapses
    // it to 4, then it regrows.
    let mut hw = 0u16;
    let mut heights = Vec::new();
    for desired in [5u16, 8, 4, 5] {
        let (h, next) = streaming_height_floor(desired, hw, /*freeze*/ true);
        hw = next;
        heights.push(h);
    }
    assert_eq!(heights, vec![5, 8, 8, 8], "height never drops mid-turn");
    assert!(
        heights.windows(2).all(|w| w[1] >= w[0]),
        "monotonic non-decreasing while the turn is frozen"
    );

    // An interactive-prompt frame is exempt (freeze=false): the watermark resets
    // to 0 so a tall prompt height is never recorded, and the turn resumes from
    // the small natural height instead of a frozen-tall one (the regression the
    // active_prompt exemption prevents).
    let (h_prompt, next) =
        streaming_height_floor(/*desired*/ 30, /*high_water*/ 8, false);
    assert_eq!(
        (h_prompt, next),
        (30, 0),
        "prompt sizes to content, watermark cleared"
    );
    let (h_resume, _) = streaming_height_floor(/*desired*/ 5, /*high_water*/ next, true);
    assert_eq!(
        h_resume, 5,
        "turn resumes from natural height, not frozen tall"
    );
}

#[test]
fn hold_bottom_edge_keeps_input_steady_on_stream_finish() {
    // Streaming frame: y=42 h=12 → bottom 54. A top-anchored relax to h=5 gives
    // y=42 bottom=47 — the input bar would jump UP 7 rows. The hold pins the
    // bottom back to 54 (y=49) for the transition frame.
    let prev = Rect::new(0, 42, 80, 12);
    let relaxed = Rect::new(0, 42, 80, 5);
    let held = hold_bottom_edge_on_relax(relaxed, prev, Size::new(80, 60), true);
    assert_eq!(held, Rect::new(0, 49, 80, 5));
    assert_eq!(
        held.bottom(),
        prev.bottom(),
        "bottom edge (input bar) stays put"
    );
}

#[test]
fn hold_bottom_edge_is_noop_outside_the_transition() {
    let prev = Rect::new(0, 42, 80, 12);
    let relaxed = Rect::new(0, 42, 80, 5);
    assert_eq!(
        hold_bottom_edge_on_relax(relaxed, prev, Size::new(80, 60), false),
        relaxed,
        "only the transition frame holds the bottom"
    );
}

#[test]
fn hold_bottom_edge_is_noop_when_bottom_would_not_rise() {
    // Height grew: the bottom doesn't move up, so nothing to hold.
    let prev = Rect::new(0, 42, 80, 5);
    let grown = Rect::new(0, 42, 80, 8);
    assert_eq!(
        hold_bottom_edge_on_relax(grown, prev, Size::new(80, 60), true),
        grown
    );
}

#[test]
fn needs_repin_on_relax_only_when_freeze_releases_and_height_shrinks() {
    let prev = Rect::new(0, 42, 80, 12);
    let shrunk = Rect::new(0, 50, 80, 4);
    let grown = Rect::new(0, 30, 80, 14);
    // Freeze just released AND the viewport shrank (turn end): re-pin history so
    // the input bar doesn't settle high with a gap below.
    assert!(needs_repin_on_relax(/*relaxing*/ true, shrunk, prev));
    // Not a relax frame (mid-turn or idle): never re-pin.
    assert!(!needs_repin_on_relax(/*relaxing*/ false, shrunk, prev));
    // Relaxing but the viewport GREW (an interactive prompt taking over): the
    // prompt sizes to content, no re-pin needed.
    assert!(!needs_repin_on_relax(/*relaxing*/ true, grown, prev));
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
