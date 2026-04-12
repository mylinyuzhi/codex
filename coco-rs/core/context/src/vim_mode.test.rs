use super::*;

#[test]
fn test_normal_to_insert() {
    let (action, mode) = process_vim_key(VimMode::Normal, "i");
    assert!(matches!(action, VimAction::SwitchMode(VimMode::Insert)));
    assert_eq!(mode, VimMode::Insert);
}

#[test]
fn test_insert_escape() {
    let (action, mode) = process_vim_key(VimMode::Insert, "escape");
    assert!(matches!(action, VimAction::SwitchMode(VimMode::Normal)));
    assert_eq!(mode, VimMode::Normal);
}

#[test]
fn test_normal_movement() {
    let (action, mode) = process_vim_key(VimMode::Normal, "h");
    assert!(matches!(action, VimAction::MoveCursor(CursorMove::Left)));
    assert_eq!(mode, VimMode::Normal);
}

#[test]
fn test_insert_char() {
    let (action, mode) = process_vim_key(VimMode::Insert, "a");
    assert!(matches!(action, VimAction::InsertChar('a')));
    assert_eq!(mode, VimMode::Insert);
}
