use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

use crate::action::Action;
use crate::context::KeybindingContext;
use crate::key::KeyCombo;
use crate::key::KeySequence;
use crate::test_helpers::make_binding;
use crate::test_helpers::make_key_event;

use super::*;

#[test]
fn test_resolve_single_key() {
    let resolver = BindingResolver::new(vec![make_binding(
        KeybindingContext::Chat,
        KeyModifiers::CONTROL,
        KeyCode::Char('c'),
        Action::AppInterrupt,
    )]);

    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('c'));
    let result = resolver.resolve_single(&[KeybindingContext::Chat], &event);
    assert_eq!(result, Some(Action::AppInterrupt));
}

#[test]
fn test_resolve_global_fallback() {
    let resolver = BindingResolver::new(vec![make_binding(
        KeybindingContext::Global,
        KeyModifiers::CONTROL,
        KeyCode::Char('c'),
        Action::AppInterrupt,
    )]);

    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('c'));
    let result = resolver.resolve_single(&[KeybindingContext::Chat], &event);
    assert_eq!(result, Some(Action::AppInterrupt));
}

#[test]
fn test_context_specific_overrides_global() {
    let resolver = BindingResolver::new(vec![
        make_binding(
            KeybindingContext::Global,
            KeyModifiers::NONE,
            KeyCode::Esc,
            Action::ChatCancel,
        ),
        make_binding(
            KeybindingContext::Help,
            KeyModifiers::NONE,
            KeyCode::Esc,
            Action::HelpClose,
        ),
    ]);

    let event = make_key_event(KeyModifiers::NONE, KeyCode::Esc);
    let result = resolver.resolve_single(&[KeybindingContext::Help], &event);
    assert_eq!(result, Some(Action::HelpClose));
}

#[test]
fn test_last_match_wins() {
    let resolver = BindingResolver::new(vec![
        make_binding(
            KeybindingContext::Chat,
            KeyModifiers::CONTROL,
            KeyCode::Char('t'),
            Action::ExtCycleThinkingLevel,
        ),
        make_binding(
            KeybindingContext::Chat,
            KeyModifiers::CONTROL,
            KeyCode::Char('t'),
            Action::AppToggleTodos,
        ),
    ]);

    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('t'));
    let result = resolver.resolve_single(&[KeybindingContext::Chat], &event);
    assert_eq!(result, Some(Action::AppToggleTodos));
}

#[test]
fn test_no_match() {
    let resolver = BindingResolver::new(vec![make_binding(
        KeybindingContext::Chat,
        KeyModifiers::CONTROL,
        KeyCode::Char('c'),
        Action::AppInterrupt,
    )]);

    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('z'));
    let result = resolver.resolve_single(&[KeybindingContext::Chat], &event);
    assert_eq!(result, None);
}

#[test]
fn test_wrong_context_no_match() {
    let resolver = BindingResolver::new(vec![make_binding(
        KeybindingContext::Help,
        KeyModifiers::NONE,
        KeyCode::Esc,
        Action::HelpClose,
    )]);

    let event = make_key_event(KeyModifiers::NONE, KeyCode::Esc);
    let result = resolver.resolve_single(&[KeybindingContext::Autocomplete], &event);
    assert_eq!(result, None);
}

#[test]
fn test_empty_binding_table() {
    let resolver = BindingResolver::new(vec![]);
    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('c'));
    let result = resolver.resolve_single(&[KeybindingContext::Chat], &event);
    assert_eq!(result, None);
}

#[test]
fn test_multiple_active_contexts() {
    let resolver = BindingResolver::new(vec![
        make_binding(
            KeybindingContext::Chat,
            KeyModifiers::NONE,
            KeyCode::Tab,
            Action::ExtTogglePlanMode,
        ),
        make_binding(
            KeybindingContext::Autocomplete,
            KeyModifiers::NONE,
            KeyCode::Tab,
            Action::AutocompleteAccept,
        ),
    ]);

    // Both contexts active — last match wins (Autocomplete binding comes after Chat).
    let event = make_key_event(KeyModifiers::NONE, KeyCode::Tab);
    let result = resolver.resolve_single(
        &[KeybindingContext::Chat, KeybindingContext::Autocomplete],
        &event,
    );
    assert_eq!(result, Some(Action::AutocompleteAccept));
}

#[test]
fn test_display_text_for_action() {
    let resolver = BindingResolver::new(vec![make_binding(
        KeybindingContext::Chat,
        KeyModifiers::CONTROL,
        KeyCode::Char('c'),
        Action::AppInterrupt,
    )]);

    let text = resolver.display_text_for_action(&Action::AppInterrupt, &[KeybindingContext::Chat]);
    assert!(text.is_some());
    let text = text.unwrap();
    assert!(text.contains("Ctrl"), "expected Ctrl in '{text}'");
}

#[test]
fn test_bindings_for_context() {
    let resolver = BindingResolver::new(vec![
        make_binding(
            KeybindingContext::Chat,
            KeyModifiers::CONTROL,
            KeyCode::Char('c'),
            Action::AppInterrupt,
        ),
        make_binding(
            KeybindingContext::Help,
            KeyModifiers::NONE,
            KeyCode::Esc,
            Action::HelpClose,
        ),
    ]);

    assert_eq!(
        resolver.bindings_for_context(KeybindingContext::Chat).len(),
        1
    );
    assert_eq!(
        resolver.bindings_for_context(KeybindingContext::Help).len(),
        1
    );
}

#[test]
fn test_has_any_chords() {
    let resolver = BindingResolver::new(vec![make_binding(
        KeybindingContext::Chat,
        KeyModifiers::CONTROL,
        KeyCode::Char('c'),
        Action::AppInterrupt,
    )]);
    assert!(!resolver.has_any_chords());

    let resolver_with_chord = BindingResolver::new(vec![Binding {
        context: KeybindingContext::Chat,
        sequence: KeySequence {
            keys: vec![
                KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('k')),
                KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c')),
            ],
        },
        action: Action::ExtClearScreen,
    }]);
    assert!(resolver_with_chord.has_any_chords());
}
