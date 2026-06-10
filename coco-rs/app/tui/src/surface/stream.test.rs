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
fn cap_is_display_only_and_uncapped_scroll_shows_full_stream() {
    let build = |user_scrolled: bool| {
        let mut state = AppState::new();
        let mut streaming = StreamingState::new();
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

    assert!(capped.lines.len() <= STREAMING_LIVE_TAIL_CAP as usize);
    assert!(uncapped.lines.len() > STREAMING_LIVE_TAIL_CAP as usize);
    let append_text = history_rows_text(
        &uncapped
            .stream_append
            .as_ref()
            .expect("stable prefix append")
            .rows,
    );
    assert!(append_text.contains("committed paragraph"));
    let uncapped_text = plain_lines(&uncapped.lines).join("\n");
    assert!(uncapped_text.contains("item 14"));
}

#[test]
fn stable_stream_prefix_prepares_native_append_and_leaves_viewport_tail() {
    let mut state = AppState::new();
    let mut streaming = StreamingState::new();
    streaming.append_text("alpha\n\n");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);

    let mut driver = SurfaceStreamDriver::default();
    let first = driver.prepare(&state, /*width*/ 40, native_plan());
    let first_append = first.stream_append.as_ref().expect("stream append");
    assert!(history_rows_text(&first_append.rows).contains("alpha"));
    assert!(!plain_lines(&first.lines).join("\n").contains("alpha"));
    driver.mark_stream_append_committed(first_append);

    let streaming = state.ui.streaming.as_mut().expect("streaming");
    streaming.append_text("beta\n\n");
    streaming.reveal_all();
    let second = driver.prepare(&state, /*width*/ 40, native_plan());
    let second_append = second.stream_append.as_ref().expect("stream append");
    let second_append_text = history_rows_text(&second_append.rows);
    let second_text = plain_lines(&second.lines).join("\n");

    assert_eq!(
        second_append_text.matches("alpha").count(),
        0,
        "{second_append_text}"
    );
    assert_eq!(
        second_append_text.matches("beta").count(),
        1,
        "{second_append_text}"
    );
    assert_eq!(second_text.matches("alpha").count(), 0, "{second_text}");
    assert_eq!(second_text.matches("beta").count(), 0, "{second_text}");
}

#[test]
fn stream_append_fingerprints_accumulate_incrementally_to_full_prefix() {
    let mut state = AppState::new();
    let mut streaming = StreamingState::new();
    streaming.append_text("alpha\n\n");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);

    let mut driver = SurfaceStreamDriver::default();
    let first = driver.prepare(&state, /*width*/ 40, native_plan());
    let first_append = first.stream_append.as_ref().expect("first stable append");
    driver.mark_stream_append_committed(first_append);

    let streaming = state.ui.streaming.as_mut().expect("streaming");
    streaming.append_text("beta\n\ngamma\n\n");
    streaming.reveal_all();
    let second = driver.prepare(&state, /*width*/ 40, native_plan());
    let second_append = second.stream_append.expect("second stable append");

    // A fresh driver fingerprints the same full stable prefix from scratch;
    // the incremental accumulation must agree with it exactly.
    let from_scratch = SurfaceStreamDriver::default()
        .prepare(&state, /*width*/ 40, native_plan())
        .stream_append
        .expect("from-scratch append");
    assert_eq!(
        second_append.prefix.line_fingerprints,
        from_scratch.prefix.line_fingerprints
    );
    assert_eq!(
        second_append.prefix.line_prefix_len,
        from_scratch.prefix.line_prefix_len
    );
    assert_eq!(
        second_append.prefix.line_fingerprints.len(),
        second_append.prefix.line_prefix_len
    );
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

fn plain_lines(lines: &[ratatui::text::Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

fn history_rows_text(rows: &coco_tui_ui::engine::history_insert::HistoryRows) -> String {
    rows.buffer()
        .content
        .chunks(rows.width() as usize)
        .map(|cells| {
            cells
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
