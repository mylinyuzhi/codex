use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

use crate::action::Action;
use crate::context::KeybindingContext;
use crate::key::KeyCombo;
use crate::key::KeySequence;
use crate::resolver::Binding;
use crate::resolver::BindingResolver;
use crate::test_helpers::make_key_event;

use super::*;

fn make_chord_resolver() -> BindingResolver {
    BindingResolver::new(vec![
        Binding {
            context: KeybindingContext::Chat,
            sequence: KeySequence::single(KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c'))),
            action: Action::AppInterrupt,
        },
        Binding {
            context: KeybindingContext::Chat,
            sequence: KeySequence {
                keys: vec![
                    KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('k')),
                    KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c')),
                ],
            },
            action: Action::ExtClearScreen,
        },
    ])
}

#[test]
fn test_single_key_match_no_chord() {
    let resolver = make_chord_resolver();
    let mut matcher = ChordMatcher::new();
    let contexts = [KeybindingContext::Chat];

    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('c'));
    let result = matcher.process_key(&event, &resolver, &contexts);
    assert_eq!(result, ChordResult::Matched(Action::AppInterrupt));
    assert!(!matcher.is_pending());
}

#[test]
fn test_chord_prefix_then_match() {
    let resolver = make_chord_resolver();
    let mut matcher = ChordMatcher::new();
    let contexts = [KeybindingContext::Chat];

    let event1 = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('k'));
    let result1 = matcher.process_key(&event1, &resolver, &contexts);
    assert_eq!(result1, ChordResult::PrefixMatch);
    assert!(matcher.is_pending());

    let event2 = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('c'));
    let result2 = matcher.process_key(&event2, &resolver, &contexts);
    assert_eq!(result2, ChordResult::Matched(Action::ExtClearScreen));
    assert!(!matcher.is_pending());
}

#[test]
fn test_escape_cancels_chord() {
    let resolver = make_chord_resolver();
    let mut matcher = ChordMatcher::new();
    let contexts = [KeybindingContext::Chat];

    let event1 = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('k'));
    matcher.process_key(&event1, &resolver, &contexts);
    assert!(matcher.is_pending());

    let esc = make_key_event(KeyModifiers::NONE, KeyCode::Esc);
    let result = matcher.process_key(&esc, &resolver, &contexts);
    assert_eq!(result, ChordResult::Cancelled);
    assert!(!matcher.is_pending());
}

#[test]
fn test_wrong_second_key_cancels_chord() {
    let resolver = make_chord_resolver();
    let mut matcher = ChordMatcher::new();
    let contexts = [KeybindingContext::Chat];

    let event1 = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('k'));
    matcher.process_key(&event1, &resolver, &contexts);
    assert!(matcher.is_pending());

    let event2 = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('x'));
    let result = matcher.process_key(&event2, &resolver, &contexts);
    assert_eq!(result, ChordResult::Cancelled);
    assert!(!matcher.is_pending());
}

#[test]
fn test_no_match_for_unbound_key() {
    let resolver = make_chord_resolver();
    let mut matcher = ChordMatcher::new();
    let contexts = [KeybindingContext::Chat];

    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('z'));
    let result = matcher.process_key(&event, &resolver, &contexts);
    assert_eq!(result, ChordResult::NoMatch);
}

#[test]
fn test_esc_esc_chord() {
    // Verify the Esc-Esc chord pattern works correctly.
    let resolver = BindingResolver::new(vec![
        // Single Esc → ChatCancel.
        Binding {
            context: KeybindingContext::Chat,
            sequence: KeySequence::single(KeyCombo::new(KeyModifiers::NONE, KeyCode::Esc)),
            action: Action::ChatCancel,
        },
        // Esc Esc chord → ShowRewindSelector.
        Binding {
            context: KeybindingContext::Chat,
            sequence: KeySequence {
                keys: vec![
                    KeyCombo::new(KeyModifiers::NONE, KeyCode::Esc),
                    KeyCombo::new(KeyModifiers::NONE, KeyCode::Esc),
                ],
            },
            action: Action::ExtShowRewindSelector,
        },
    ]);

    let mut matcher = ChordMatcher::new();
    let contexts = [KeybindingContext::Chat];

    // First Esc: should be a prefix match (chord pending).
    let esc1 = make_key_event(KeyModifiers::NONE, KeyCode::Esc);
    let result1 = matcher.process_key(&esc1, &resolver, &contexts);
    assert_eq!(result1, ChordResult::PrefixMatch);
    assert!(matcher.is_pending());

    // Second Esc: should complete the chord.
    let esc2 = make_key_event(KeyModifiers::NONE, KeyCode::Esc);
    let result2 = matcher.process_key(&esc2, &resolver, &contexts);
    assert_eq!(result2, ChordResult::Matched(Action::ExtShowRewindSelector));
    assert!(!matcher.is_pending());
}

#[test]
fn test_check_timeout_no_pending() {
    let mut matcher = ChordMatcher::new();
    assert!(matcher.check_timeout().is_none());
}

#[test]
fn test_check_timeout_returns_pending_keys() {
    // Simulate Esc-Esc chord scenario where single Esc times out.
    let resolver = BindingResolver::new(vec![
        Binding {
            context: KeybindingContext::Chat,
            sequence: KeySequence::single(KeyCombo::new(KeyModifiers::NONE, KeyCode::Esc)),
            action: Action::ChatCancel,
        },
        Binding {
            context: KeybindingContext::Chat,
            sequence: KeySequence {
                keys: vec![
                    KeyCombo::new(KeyModifiers::NONE, KeyCode::Esc),
                    KeyCombo::new(KeyModifiers::NONE, KeyCode::Esc),
                ],
            },
            action: Action::ExtShowRewindSelector,
        },
    ]);

    let mut matcher = ChordMatcher::new();
    let contexts = [KeybindingContext::Chat];

    // First Esc starts chord.
    let esc = make_key_event(KeyModifiers::NONE, KeyCode::Esc);
    let result = matcher.process_key(&esc, &resolver, &contexts);
    assert_eq!(result, ChordResult::PrefixMatch);
    assert!(matcher.is_pending());

    // Force timeout by setting started_at to the past.
    matcher.started_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(2));

    // check_timeout should return the pending [Esc] keys.
    let timed_out = matcher.check_timeout();
    assert!(timed_out.is_some());
    let keys = timed_out.expect("should have timed-out keys");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].code, KeyCode::Esc);
    assert!(!matcher.is_pending());
}
