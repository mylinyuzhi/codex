use std::str::FromStr;

use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

use crate::test_helpers::make_key_event;

use super::*;

#[test]
fn test_parse_simple_key() {
    let seq = KeySequence::from_str("a").unwrap();
    assert_eq!(seq.keys().len(), 1);
    assert_eq!(seq.keys()[0].code, KeyCode::Char('a'));
    assert_eq!(seq.keys()[0].modifiers, KeyModifiers::NONE);
}

#[test]
fn test_parse_ctrl_key() {
    let seq = KeySequence::from_str("ctrl+c").unwrap();
    assert_eq!(seq.keys()[0].modifiers, KeyModifiers::CONTROL);
    assert_eq!(seq.keys()[0].code, KeyCode::Char('c'));
}

#[test]
fn test_parse_ctrl_shift_key() {
    let seq = KeySequence::from_str("ctrl+shift+t").unwrap();
    assert!(seq.keys()[0].modifiers.contains(KeyModifiers::CONTROL));
    assert!(seq.keys()[0].modifiers.contains(KeyModifiers::SHIFT));
    assert_eq!(seq.keys()[0].code, KeyCode::Char('t'));
}

#[test]
fn test_parse_modifier_aliases() {
    let seq = KeySequence::from_str("control+c").unwrap();
    assert!(seq.keys()[0].modifiers.contains(KeyModifiers::CONTROL));

    let seq = KeySequence::from_str("cmd+p").unwrap();
    assert!(seq.keys()[0].modifiers.contains(KeyModifiers::META));

    let seq = KeySequence::from_str("opt+v").unwrap();
    assert!(seq.keys()[0].modifiers.contains(KeyModifiers::ALT));

    let seq = KeySequence::from_str("super+p").unwrap();
    assert!(seq.keys()[0].modifiers.contains(KeyModifiers::META));

    let seq = KeySequence::from_str("win+p").unwrap();
    assert!(seq.keys()[0].modifiers.contains(KeyModifiers::META));
}

#[test]
fn test_parse_special_keys() {
    assert_eq!(
        KeySequence::from_str("enter").unwrap().keys()[0].code,
        KeyCode::Enter
    );
    assert_eq!(
        KeySequence::from_str("escape").unwrap().keys()[0].code,
        KeyCode::Esc
    );
    assert_eq!(
        KeySequence::from_str("esc").unwrap().keys()[0].code,
        KeyCode::Esc
    );
    assert_eq!(
        KeySequence::from_str("tab").unwrap().keys()[0].code,
        KeyCode::Tab
    );
    assert_eq!(
        KeySequence::from_str("space").unwrap().keys()[0].code,
        KeyCode::Char(' ')
    );
    assert_eq!(
        KeySequence::from_str("backspace").unwrap().keys()[0].code,
        KeyCode::Backspace
    );
    assert_eq!(
        KeySequence::from_str("delete").unwrap().keys()[0].code,
        KeyCode::Delete
    );
    assert_eq!(
        KeySequence::from_str("pageup").unwrap().keys()[0].code,
        KeyCode::PageUp
    );
    assert_eq!(
        KeySequence::from_str("pagedown").unwrap().keys()[0].code,
        KeyCode::PageDown
    );
    assert_eq!(
        KeySequence::from_str("home").unwrap().keys()[0].code,
        KeyCode::Home
    );
    assert_eq!(
        KeySequence::from_str("end").unwrap().keys()[0].code,
        KeyCode::End
    );
    assert_eq!(
        KeySequence::from_str("f1").unwrap().keys()[0].code,
        KeyCode::F(1)
    );
    assert_eq!(
        KeySequence::from_str("f12").unwrap().keys()[0].code,
        KeyCode::F(12)
    );
}

#[test]
fn test_parse_chord() {
    let seq = KeySequence::from_str("ctrl+k ctrl+c").unwrap();
    assert_eq!(seq.keys().len(), 2);
    assert!(seq.is_chord());
    assert_eq!(seq.keys()[0].code, KeyCode::Char('k'));
    assert!(seq.keys()[0].modifiers.contains(KeyModifiers::CONTROL));
    assert_eq!(seq.keys()[1].code, KeyCode::Char('c'));
    assert!(seq.keys()[1].modifiers.contains(KeyModifiers::CONTROL));
}

#[test]
fn test_parse_esc_esc_chord() {
    let seq = KeySequence::from_str("esc esc").unwrap();
    assert_eq!(seq.keys().len(), 2);
    assert!(seq.is_chord());
    assert_eq!(seq.keys()[0].code, KeyCode::Esc);
    assert_eq!(seq.keys()[1].code, KeyCode::Esc);
}

#[test]
fn test_parse_empty_fails() {
    assert!(KeySequence::from_str("").is_err());
}

#[test]
fn test_parse_invalid_key() {
    assert!(KeySequence::from_str("ctrl+").is_err());
    assert!(KeySequence::from_str("ctrl++c").is_err());
}

#[test]
fn test_combo_matches_exact() {
    let combo = KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c'));
    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('c'));
    assert!(combo.matches(&event));
}

#[test]
fn test_combo_matches_case_insensitive() {
    let combo = KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('t'));
    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('T'));
    assert!(combo.matches(&event));
}

#[test]
fn test_combo_alt_meta_equivalence() {
    // ALT combo should match META event.
    let combo = KeyCombo::new(KeyModifiers::ALT, KeyCode::Char('v'));
    let event = make_key_event(KeyModifiers::META, KeyCode::Char('v'));
    assert!(combo.matches(&event));

    // META combo should match ALT event.
    let combo = KeyCombo::new(KeyModifiers::META, KeyCode::Char('p'));
    let event = make_key_event(KeyModifiers::ALT, KeyCode::Char('p'));
    assert!(combo.matches(&event));
}

#[test]
fn test_combo_no_match_different_key() {
    let combo = KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c'));
    let event = make_key_event(KeyModifiers::CONTROL, KeyCode::Char('v'));
    assert!(!combo.matches(&event));
}

#[test]
fn test_combo_no_match_different_modifier() {
    let combo = KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c'));
    let event = make_key_event(KeyModifiers::ALT, KeyCode::Char('c'));
    assert!(!combo.matches(&event));
}

#[test]
fn test_prefix_match() {
    let seq = KeySequence::from_str("ctrl+k ctrl+c").unwrap();
    let prefix = vec![KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('k'))];
    assert!(seq.is_prefix_of(&prefix));
}

#[test]
fn test_prefix_match_full_sequence_not_prefix() {
    let seq = KeySequence::from_str("ctrl+k ctrl+c").unwrap();
    let full = vec![
        KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('k')),
        KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c')),
    ];
    assert!(!seq.is_prefix_of(&full));
}

#[test]
fn test_exact_match() {
    let seq = KeySequence::from_str("ctrl+k ctrl+c").unwrap();
    let full = vec![
        KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('k')),
        KeyCombo::new(KeyModifiers::CONTROL, KeyCode::Char('c')),
    ];
    assert!(seq.matches_exactly(&full));
}

#[test]
fn test_display_single_key() {
    let seq = KeySequence::from_str("ctrl+shift+t").unwrap();
    let display = seq.to_string();
    assert!(display.contains("Ctrl"));
    assert!(display.contains("Shift"));
}

#[test]
fn test_display_chord() {
    let seq = KeySequence::from_str("ctrl+k ctrl+c").unwrap();
    let display = seq.to_string();
    assert!(display.contains(' '));
}

#[test]
fn test_keys_accessor() {
    let seq = KeySequence::from_str("ctrl+c").unwrap();
    assert_eq!(seq.keys().len(), 1);
    assert_eq!(seq.keys()[0].code, KeyCode::Char('c'));
}
