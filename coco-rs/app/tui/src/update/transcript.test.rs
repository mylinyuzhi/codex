use super::toggle;
use crate::state::AppState;
use crate::state::Overlay;
use crate::state::session::ChatMessage;
use crate::state::transcript::TranscriptCellId;
use crate::state::transcript::TranscriptScrollPosition;

#[test]
fn toggle_opens_transcript_when_no_overlay_active() {
    let mut state = AppState::new();
    toggle(&mut state);
    assert!(matches!(
        state.ui.active_overlay(),
        Some(Overlay::Transcript(_))
    ));
}

#[test]
fn toggle_closes_transcript_when_already_open() {
    let mut state = AppState::new();
    toggle(&mut state);
    assert!(matches!(
        state.ui.active_overlay(),
        Some(Overlay::Transcript(_))
    ));
    toggle(&mut state);
    assert!(!state.ui.has_overlay());
}

#[test]
fn transcript_overlay_defaults_to_cell_pager_state() {
    let mut state = AppState::new();
    toggle(&mut state);
    let overlay = state.ui.active_overlay().expect("transcript opened");
    let Overlay::Transcript(t) = overlay else {
        panic!("expected Transcript overlay");
    };
    assert_eq!(t.scroll, TranscriptScrollPosition::Top);
    assert_eq!(t.selected_cell_id, None);
    assert!(t.collapsed_cell_ids.is_empty());
}

#[test]
fn toggle_opens_transcript_on_latest_expandable_cell() {
    let mut state = AppState::new();
    state
        .session
        .add_message(ChatMessage::tool_success("tool-old", "Read", "old\nlines"));
    state
        .session
        .add_message(ChatMessage::tool_success("tool-new", "Read", "new\nlines"));

    toggle(&mut state);

    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert_eq!(
        t.selected_cell_id.as_ref(),
        Some(&TranscriptCellId::tool("new"))
    );
    assert_eq!(
        t.scroll,
        TranscriptScrollPosition::anchor(TranscriptCellId::tool("new"))
    );
}

#[test]
fn select_and_enter_toggle_collapsed_cell() {
    let mut state = AppState::new();
    state.session.add_message(ChatMessage::tool_success(
        "tool-call-1",
        "Read",
        "alpha\nbeta",
    ));
    toggle(&mut state);

    assert!(super::select_expandable(&mut state, 1));
    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert_eq!(
        t.selected_cell_id.as_ref(),
        Some(&TranscriptCellId::tool("call-1"))
    );

    assert!(super::toggle_selected_cell(&mut state));
    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert!(
        t.collapsed_cell_ids
            .contains(&TranscriptCellId::tool("call-1"))
    );

    assert!(super::toggle_selected_cell(&mut state));
    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert!(
        !t.collapsed_cell_ids
            .contains(&TranscriptCellId::tool("call-1"))
    );
}

#[test]
fn select_expandable_wraps_at_edges() {
    let mut state = AppState::new();
    state
        .session
        .add_message(ChatMessage::tool_success("tool-first", "Read", "one\ntwo"));
    state.session.add_message(ChatMessage::tool_success(
        "tool-last",
        "Read",
        "three\nfour",
    ));
    toggle(&mut state);

    assert!(super::select_expandable(&mut state, 1));
    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert_eq!(
        t.selected_cell_id.as_ref(),
        Some(&TranscriptCellId::tool("first"))
    );

    assert!(super::select_expandable(&mut state, -1));
    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert_eq!(
        t.selected_cell_id.as_ref(),
        Some(&TranscriptCellId::tool("last"))
    );
}

#[test]
fn select_expandable_anchors_selected_cell_from_current_scroll() {
    let mut state = AppState::new();
    state.session.add_message(ChatMessage::tool_success(
        "tool-call-1",
        "Read",
        "alpha\nbeta",
    ));
    toggle(&mut state);
    assert!(super::scroll_lines(&mut state, 40));

    assert!(super::select_expandable(&mut state, 1));

    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert_eq!(
        t.scroll,
        TranscriptScrollPosition::anchor(TranscriptCellId::tool("call-1"))
    );
}

#[test]
fn transcript_scroll_uses_overlay_state() {
    let mut state = AppState::new();
    toggle(&mut state);

    assert!(super::scroll_lines(&mut state, 5));
    assert_eq!(state.ui.scroll_offset, 0);
    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert_eq!(t.scroll, TranscriptScrollPosition::Absolute(5));
}

#[test]
fn transcript_page_uses_terminal_size() {
    let mut state = AppState::new();
    state.ui.terminal_size = ratatui::layout::Size::new(100, 21);
    toggle(&mut state);
    let Some(Overlay::Transcript(_)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };

    assert!(super::page(&mut state, 1));
    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert_eq!(t.scroll, TranscriptScrollPosition::Absolute(17));

    assert!(super::page(&mut state, -1));
    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert_eq!(t.scroll, TranscriptScrollPosition::Top);
}
