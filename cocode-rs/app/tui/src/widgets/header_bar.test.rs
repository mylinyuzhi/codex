use super::*;
use crate::theme::Theme;

#[test]
fn test_header_bar_new_session_defaults() {
    let theme = Theme::default();
    let header = HeaderBar::new(&theme);
    // Default state: no session, no working dir, zero turns
    assert!(header.session_id.is_none());
    assert!(header.working_dir.is_none());
    assert_eq!(header.turn_count, 0);
    assert!(!header.is_compacting);
    assert!(header.fallback_model.is_none());
    assert_eq!(header.active_worktrees, 0);
}

#[test]
fn test_header_bar_builder_chain() {
    let theme = Theme::default();
    let header = HeaderBar::new(&theme)
        .session_id(Some("sess-123"))
        .working_dir(Some("/home/user/project"))
        .turn_count(5)
        .is_compacting(true)
        .fallback_model(Some("gpt-4o"))
        .active_worktrees(2);
    assert_eq!(header.session_id, Some("sess-123"));
    assert_eq!(header.turn_count, 5);
    assert!(header.is_compacting);
    assert_eq!(header.active_worktrees, 2);
}
