use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

use crate::action::Action;
use crate::context::KeybindingContext;
use crate::test_helpers::make_key_event;

use super::*;

#[test]
fn test_defaults_only_resolves_ctrl_c() {
    let manager = KeybindingsManager::defaults_only();
    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('c'));
    let result = manager.process_key(&[KeybindingContext::Chat], &event);
    assert_eq!(result, KeybindingResult::Action(Action::AppInterrupt));
}

#[test]
fn test_defaults_only_resolves_enter() {
    let manager = KeybindingsManager::defaults_only();
    let event = make_key_event(KeyModifiers::NONE, KeyCode::Enter);
    let result = manager.process_key(&[KeybindingContext::Chat], &event);
    assert_eq!(result, KeybindingResult::Action(Action::ChatSubmit));
}

#[test]
fn test_defaults_only_autocomplete_tab() {
    let manager = KeybindingsManager::defaults_only();
    let event = make_key_event(KeyModifiers::NONE, KeyCode::Tab);
    let result = manager.process_key(&[KeybindingContext::Autocomplete], &event);
    assert_eq!(result, KeybindingResult::Action(Action::AutocompleteAccept));
}

#[test]
fn test_defaults_only_unbound_key() {
    let manager = KeybindingsManager::defaults_only();
    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('z'));
    let result = manager.process_key(&[KeybindingContext::Chat], &event);
    assert_eq!(result, KeybindingResult::Unhandled);
}

#[test]
fn test_defaults_only_confirmation_y() {
    let manager = KeybindingsManager::defaults_only();
    let event = make_key_event(KeyModifiers::NONE, KeyCode::Char('y'));
    let result = manager.process_key(&[KeybindingContext::Confirmation], &event);
    assert_eq!(result, KeybindingResult::Action(Action::ConfirmYes));
}

#[test]
fn test_chord_pending_state() {
    let manager = KeybindingsManager::defaults_only();
    assert!(!manager.is_chord_pending());
}

#[test]
fn test_display_text_for_action() {
    let manager = KeybindingsManager::defaults_only();
    let text = manager.display_text_for_action(&Action::AppInterrupt, &[KeybindingContext::Chat]);
    assert!(text.is_some());
}

#[test]
fn test_esc_esc_chord_via_manager() {
    let manager = KeybindingsManager::defaults_only();
    let contexts = [KeybindingContext::Chat];

    // First Esc → PendingChord (because Esc Esc chord exists as prefix).
    let esc1 = make_key_event(KeyModifiers::NONE, KeyCode::Esc);
    let result1 = manager.process_key(&contexts, &esc1);
    assert_eq!(result1, KeybindingResult::PendingChord);

    // Second Esc → Matched ExtShowRewindSelector.
    let esc2 = make_key_event(KeyModifiers::NONE, KeyCode::Esc);
    let result2 = manager.process_key(&contexts, &esc2);
    assert_eq!(
        result2,
        KeybindingResult::Action(Action::ExtShowRewindSelector)
    );
}

#[test]
fn test_esc_then_other_key_cancels_chord() {
    let manager = KeybindingsManager::defaults_only();
    let contexts = [KeybindingContext::Chat];

    // First Esc → PendingChord.
    let esc = make_key_event(KeyModifiers::NONE, KeyCode::Esc);
    let result1 = manager.process_key(&contexts, &esc);
    assert_eq!(result1, KeybindingResult::PendingChord);

    // Non-Esc key → ChordCancelled.
    let other = make_key_event(KeyModifiers::NONE, KeyCode::Char('a'));
    let result2 = manager.process_key(&contexts, &other);
    assert_eq!(result2, KeybindingResult::ChordCancelled);
}

#[test]
fn test_pending_chord_display() {
    let manager = KeybindingsManager::defaults_only();
    assert_eq!(manager.pending_chord_display(), None);

    // Start a chord.
    let contexts = [KeybindingContext::Chat];
    let esc = make_key_event(KeyModifiers::NONE, KeyCode::Esc);
    manager.process_key(&contexts, &esc);
    let display = manager.pending_chord_display();
    assert!(display.is_some());
    assert!(
        display.as_deref().unwrap().contains("Esc"),
        "pending display should contain 'Esc', got: {display:?}"
    );
}

#[test]
fn test_check_chord_timeout_no_pending() {
    let manager = KeybindingsManager::defaults_only();
    let contexts = [KeybindingContext::Chat];
    assert!(manager.check_chord_timeout(&contexts).is_none());
}

#[test]
fn test_defaults_resolves_cursor_movement() {
    let manager = KeybindingsManager::defaults_only();
    let contexts = [KeybindingContext::Chat];

    let left = make_key_event(KeyModifiers::NONE, KeyCode::Left);
    assert_eq!(
        manager.process_key(&contexts, &left),
        KeybindingResult::Action(Action::ExtCursorLeft)
    );

    let right = make_key_event(KeyModifiers::NONE, KeyCode::Right);
    assert_eq!(
        manager.process_key(&contexts, &right),
        KeybindingResult::Action(Action::ExtCursorRight)
    );

    let home = make_key_event(KeyModifiers::NONE, KeyCode::Home);
    assert_eq!(
        manager.process_key(&contexts, &home),
        KeybindingResult::Action(Action::ExtCursorHome)
    );

    let end = make_key_event(KeyModifiers::NONE, KeyCode::End);
    assert_eq!(
        manager.process_key(&contexts, &end),
        KeybindingResult::Action(Action::ExtCursorEnd)
    );
}

#[test]
fn test_defaults_resolves_page_navigation() {
    let manager = KeybindingsManager::defaults_only();
    let contexts = [KeybindingContext::Chat];

    let pgup = make_key_event(KeyModifiers::NONE, KeyCode::PageUp);
    assert_eq!(
        manager.process_key(&contexts, &pgup),
        KeybindingResult::Action(Action::ExtPageUp)
    );

    let pgdn = make_key_event(KeyModifiers::NONE, KeyCode::PageDown);
    assert_eq!(
        manager.process_key(&contexts, &pgdn),
        KeybindingResult::Action(Action::ExtPageDown)
    );
}

#[test]
fn test_defaults_resolves_ctrl_shift_t() {
    let manager = KeybindingsManager::defaults_only();
    let contexts = [KeybindingContext::Chat];

    let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
    let event = make_key_event(ctrl_shift, KeyCode::Char('T'));
    assert_eq!(
        manager.process_key(&contexts, &event),
        KeybindingResult::Action(Action::ExtToggleThinking)
    );
}

#[test]
fn test_defaults_resolves_approve_all() {
    let manager = KeybindingsManager::defaults_only();
    let contexts = [KeybindingContext::Confirmation];

    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('a'));
    assert_eq!(
        manager.process_key(&contexts, &event),
        KeybindingResult::Action(Action::ExtApproveAll)
    );
}

#[tokio::test]
async fn test_manager_with_user_config() {
    let dir = tempfile::TempDir::new().unwrap();
    let config_path = dir.path().join("keybindings.json");
    std::fs::write(
        &config_path,
        r#"{"bindings":[{"context":"Chat","bindings":{"ctrl+t":"app:toggleTodos"}}]}"#,
    )
    .unwrap();

    let manager = KeybindingsManager::new(dir.path().to_path_buf(), true);
    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('t'));
    let result = manager.process_key(&[KeybindingContext::Chat], &event);
    // User override should win over default (ext:cycleThinkingLevel).
    assert_eq!(result, KeybindingResult::Action(Action::AppToggleTodos));
}
