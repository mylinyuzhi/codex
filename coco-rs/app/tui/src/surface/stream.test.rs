use super::*;
use crate::state::AppState;
use crate::state::ui::StreamingState;
use crate::surface::modal::HistorySurfaceMode;

fn native_plan() -> SurfaceFramePlan {
    SurfaceFramePlan {
        modal_placement: None,
        history_surface: HistorySurfaceMode::NativeScrollback,
        attention_requested: false,
    }
}

/// A long bullet list with NO trailing blank line stays entirely in the mutable
/// tail (the markdown stable boundary only commits at blank lines / closed
/// fences), and each item renders to its own row — so the tail (one Line per
/// item) exceeds the cap. Lists/code blocks are what actually grow the viewport
/// row count (soft-wrapped prose is one logical Line wrapped at paint).
fn streaming_state_long_tail() -> AppState {
    let mut state = AppState::new();
    let mut streaming = StreamingState::new();
    let list: String = (0..15).map(|i| format!("- item number {i}\n")).collect();
    streaming.append_text(&list);
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    state
}

#[test]
fn prepare_caps_streaming_tail_to_constant_height() {
    let state = streaming_state_long_tail();
    let mut driver = SurfaceStreamDriver::default();
    let prepared = driver.prepare(&state, /*width*/ 24, native_plan());
    assert!(
        prepared.lines.len() <= STREAMING_LIVE_TAIL_CAP as usize,
        "streaming tail must be capped to {} rows, got {}",
        STREAMING_LIVE_TAIL_CAP,
        prepared.lines.len()
    );
}

#[test]
fn cap_is_display_only_and_does_not_change_committed_lines() {
    // Safety invariant: the cap drains `lines` AFTER `stable_append` is built, so
    // it must NOT change what commits to native scrollback. That keeps the
    // finalize dedup/consolidation path intact → no loss, no duplication, and a
    // streaming code fence/list is never split mid-construct.
    let build = |user_scrolled: bool| {
        let mut state = AppState::new();
        let mut streaming = StreamingState::new();
        // "committed\n\n" crosses a blank-line boundary → commits as stable;
        // the long list after it stays in the mutable tail.
        let mut src = String::from("committed paragraph\n\n");
        src.push_str(&(0..15).map(|i| format!("- item {i}\n")).collect::<String>());
        streaming.append_text(&src);
        streaming.reveal_all();
        state.ui.streaming = Some(streaming);
        state.ui.user_scrolled = user_scrolled;
        SurfaceStreamDriver::default().prepare(&state, /*width*/ 40, native_plan())
    };
    let capped = build(/*user_scrolled*/ false);
    let uncapped = build(/*user_scrolled*/ true);

    let committed = |p: &PreparedLiveTail| p.stable_append.as_ref().map(|s| s.line_count);
    assert_eq!(
        committed(&capped),
        committed(&uncapped),
        "the display cap must not change the committed (stable_append) lines"
    );
    assert!(capped.lines.len() <= STREAMING_LIVE_TAIL_CAP as usize);
    assert!(uncapped.lines.len() > STREAMING_LIVE_TAIL_CAP as usize);
}

#[test]
fn stable_append_preserves_blank_line_between_streamed_blocks() {
    let mut state = AppState::new();
    let mut streaming = StreamingState::new();
    streaming.append_text("alpha\n\n");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);

    let mut driver = SurfaceStreamDriver::default();
    let first = driver.prepare(&state, /*width*/ 40, native_plan());
    let first_append = first.stable_append.expect("first stable append");
    assert_eq!(plain_rows(&first_append.rows), vec!["⏺ alpha"]);
    driver.mark_stable_appended();

    let streaming = state.ui.streaming.as_mut().expect("streaming");
    streaming.append_text("beta\n\n");
    streaming.reveal_all();
    let second = driver.prepare(&state, /*width*/ 40, native_plan());
    let second_append = second.stable_append.expect("second stable append");

    assert_eq!(plain_rows(&second_append.rows), vec!["", "  beta"]);
}

#[test]
fn prepare_does_not_cap_while_user_is_scrolling() {
    let mut state = streaming_state_long_tail();
    state.ui.user_scrolled = true;
    let mut driver = SurfaceStreamDriver::default();
    let prepared = driver.prepare(&state, /*width*/ 24, native_plan());
    // The same content un-capped renders to MORE than the cap — proving the
    // input genuinely overflows and that the cap (not a short render) is what
    // bounds the other test, and that an actively-scrolling user sees it all.
    assert!(
        prepared.lines.len() > STREAMING_LIVE_TAIL_CAP as usize,
        "while scrolling the full tail is shown; got {} rows (cap {})",
        prepared.lines.len(),
        STREAMING_LIVE_TAIL_CAP
    );
}

fn plain_rows(rows: &coco_tui_ui::engine::history_insert::HistoryRows) -> Vec<String> {
    let buffer = rows.buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}
