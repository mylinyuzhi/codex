use super::*;
use pretty_assertions::assert_eq;

use crate::i18n::locale_test_guard;

#[test]
fn transcript_overlay_content_renders_empty_state_and_show_all_footer() {
    let _locale = locale_test_guard("en");
    let state = AppState::default();
    let theme = Theme::default();
    let mut overlay = TranscriptOverlay::new();
    overlay.show_all = false;
    overlay.scroll = -5;

    let (title, body, border) = transcript_overlay_content(&state, &overlay, &theme);

    assert_eq!(title, " Transcript ");
    assert_eq!(border, theme.primary);
    assert!(body.contains("No messages yet."));
    assert!(body.contains("ctrl+o to toggle"));
    assert!(body.contains("show all"));
}
