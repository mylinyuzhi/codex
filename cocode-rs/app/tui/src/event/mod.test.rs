use super::*;

#[test]
fn test_tui_command_display() {
    // Test that Display impl produces non-empty translated strings
    assert!(!TuiCommand::TogglePlanMode.to_string().is_empty());
    assert!(!TuiCommand::InsertChar('a').to_string().is_empty());
    assert!(!TuiCommand::Quit.to_string().is_empty());
    assert!(!TuiCommand::WordLeft.to_string().is_empty());
    assert!(!TuiCommand::WordRight.to_string().is_empty());
    assert!(!TuiCommand::DeleteWordBackward.to_string().is_empty());
    assert!(!TuiCommand::DeleteWordForward.to_string().is_empty());
    assert!(!TuiCommand::ShowHelp.to_string().is_empty());
    assert!(!TuiCommand::ToggleThinking.to_string().is_empty());
}

#[test]
fn test_tui_event_variants() {
    // Verify we can create all event variants
    let _key = TuiEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyModifiers::NONE,
    ));
    let _resize = TuiEvent::Resize {
        width: 80,
        height: 24,
    };
    let _draw = TuiEvent::Draw;
    let _tick = TuiEvent::Tick;
    let _paste = TuiEvent::Paste("test".to_string());
    let _command = TuiEvent::Command(TuiCommand::Quit);
}
