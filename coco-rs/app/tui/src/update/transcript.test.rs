use super::toggle;
use crate::state::AppState;
use crate::state::Overlay;

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
fn transcript_overlay_defaults_show_all_to_true() {
    let mut state = AppState::new();
    toggle(&mut state);
    let overlay = state.ui.active_overlay().expect("transcript opened");
    let Overlay::Transcript(t) = overlay else {
        panic!("expected Transcript overlay");
    };
    assert!(t.show_all);
    assert_eq!(t.scroll, 0);
}

#[test]
fn toggle_show_all_flips_when_transcript_active() {
    let mut state = AppState::new();
    toggle(&mut state);
    assert!(super::toggle_show_all(&mut state));
    let Some(Overlay::Transcript(t)) = state.ui.active_overlay() else {
        panic!("expected Transcript overlay");
    };
    assert!(!t.show_all, "show_all flipped to false");
}

#[test]
fn toggle_show_all_no_op_when_no_transcript() {
    let mut state = AppState::new();
    assert!(!super::toggle_show_all(&mut state));
    assert!(!state.ui.has_overlay());
}
