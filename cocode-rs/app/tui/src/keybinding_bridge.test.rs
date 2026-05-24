use cocode_keybindings::action::Action;
use cocode_keybindings::context::KeybindingContext;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;

use crate::event::TuiCommand;
use crate::state::AppState;

use super::*;

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Press)
}

// ===== active_contexts tests =====

#[test]
fn test_default_context_is_chat() {
    let state = AppState::new();
    let contexts = active_contexts(&state);
    assert!(contexts.contains(&KeybindingContext::Chat));
}

#[test]
fn test_help_overlay_context() {
    let mut state = AppState::new();
    state.ui.overlay = Some(Overlay::Help);
    let contexts = active_contexts(&state);
    assert_eq!(contexts, vec![KeybindingContext::Help]);
}

#[test]
fn test_confirmation_overlay_context() {
    let mut state = AppState::new();
    state.ui.overlay = Some(Overlay::Error("test".to_string()));
    let contexts = active_contexts(&state);
    assert_eq!(contexts, vec![KeybindingContext::Confirmation]);
}

// ===== action_to_command tests =====

#[test]
fn test_submit_when_not_streaming() {
    let state = AppState::new();
    let cmd = action_to_command(&Action::ChatSubmit, &state);
    assert_eq!(cmd, Some(TuiCommand::SubmitInput));
}

#[test]
fn test_submit_when_streaming_queues() {
    let mut state = AppState::new();
    // Simulate streaming state
    state.ui.streaming = Some(crate::state::StreamingState::new("test-turn".to_string()));
    let cmd = action_to_command(&Action::ChatSubmit, &state);
    assert_eq!(cmd, Some(TuiCommand::QueueInput));
}

#[test]
fn test_interrupt_action() {
    let state = AppState::new();
    assert_eq!(
        action_to_command(&Action::AppInterrupt, &state),
        Some(TuiCommand::Interrupt)
    );
}

#[test]
fn test_ext_actions_mapped() {
    let state = AppState::new();
    assert_eq!(
        action_to_command(&Action::ExtTogglePlanMode, &state),
        Some(TuiCommand::TogglePlanMode)
    );
    assert_eq!(
        action_to_command(&Action::ExtClearScreen, &state),
        Some(TuiCommand::ClearScreen)
    );
    assert_eq!(
        action_to_command(&Action::ExtShowHelp, &state),
        Some(TuiCommand::ShowHelp)
    );
}

#[test]
fn test_paste_action() {
    let state = AppState::new();
    assert_eq!(
        action_to_command(&Action::ChatImagePaste, &state),
        Some(TuiCommand::PasteFromClipboard)
    );
}

#[test]
fn test_autocomplete_accept_defaults_to_file() {
    let state = AppState::new();
    // No suggestions active → defaults to file suggestion command
    assert_eq!(
        action_to_command(&Action::AutocompleteAccept, &state),
        Some(TuiCommand::AcceptSuggestion)
    );
}

// ===== unhandled_key_to_command tests =====

#[test]
fn test_unhandled_plain_char_inserts() {
    let state = AppState::new();
    let event = key(KeyCode::Char('a'), KeyModifiers::NONE);
    assert_eq!(
        unhandled_key_to_command(&event, &state),
        Some(TuiCommand::InsertChar('a'))
    );
}

#[test]
fn test_unhandled_shift_char_inserts() {
    let state = AppState::new();
    let event = key(KeyCode::Char('A'), KeyModifiers::SHIFT);
    assert_eq!(
        unhandled_key_to_command(&event, &state),
        Some(TuiCommand::InsertChar('A'))
    );
}

#[test]
fn test_unhandled_ctrl_char_returns_none() {
    let state = AppState::new();
    // Ctrl+V with no binding should NOT become InsertChar('v')
    let event = key(KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert_eq!(unhandled_key_to_command(&event, &state), None);
}

#[test]
fn test_unhandled_alt_char_returns_none() {
    let state = AppState::new();
    let event = key(KeyCode::Char('x'), KeyModifiers::ALT);
    assert_eq!(unhandled_key_to_command(&event, &state), None);
}

#[test]
fn test_unhandled_overlay_backspace() {
    let mut state = AppState::new();
    state.ui.overlay = Some(Overlay::Help);
    let event = key(KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(
        unhandled_key_to_command(&event, &state),
        Some(TuiCommand::DeleteBackward)
    );
}

#[test]
fn test_unhandled_overlay_ctrl_char_filtered() {
    let mut state = AppState::new();
    state.ui.overlay = Some(Overlay::Help);
    // Ctrl+V in overlay should be None (handled by binding, not unhandled)
    let event = key(KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert_eq!(unhandled_key_to_command(&event, &state), None);
}
