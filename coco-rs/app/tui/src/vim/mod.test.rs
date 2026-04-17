//! Tests for vim mode.

use crate::state::ui::InputState;
use crate::vim::CommandState;
use crate::vim::Operator;
use crate::vim::PersistentState;
use crate::vim::TextObjScope;
use crate::vim::VimState;
use crate::vim::motions;
use crate::vim::operators;
use crate::vim::text_objects::{self};
use crate::vim::transitions::VimAction;
use crate::vim::transitions::{self};

fn input_with(text: &str, cursor: i32) -> InputState {
    let mut input = InputState::new();
    input.text = text.to_string();
    input.cursor = cursor;
    input
}

#[test]
fn test_word_forward() {
    assert_eq!(motions::word_forward("hello world", 0), 6);
    assert_eq!(motions::word_forward("hello world", 6), 11);
}

#[test]
fn test_word_backward() {
    assert_eq!(motions::word_backward("hello world", 11), 6);
    assert_eq!(motions::word_backward("hello world", 6), 0);
}

#[test]
fn test_word_end() {
    assert_eq!(motions::word_end("hello world", 0), 4);
    assert_eq!(motions::word_end("hello world", 5), 10);
}

#[test]
fn test_find_char() {
    assert_eq!(motions::find_char_forward("hello world", 0, 'o'), Some(4));
    assert_eq!(motions::find_char_forward("hello world", 5, 'o'), Some(7));
    assert_eq!(motions::find_char_backward("hello world", 7, 'l'), Some(3));
}

#[test]
fn test_text_object_word() {
    // iw on "hello world" at pos 2 (inside "hello")
    let result = text_objects::word("hello world", 2, TextObjScope::Inner);
    assert_eq!(result, Some((0, 4)));

    // aw includes trailing space
    let result = text_objects::word("hello world", 2, TextObjScope::Around);
    assert_eq!(result, Some((0, 5)));
}

#[test]
fn test_text_object_quoted() {
    let text = r#"say "hello" world"#;
    let result = text_objects::quoted(text, 6, '"', TextObjScope::Inner);
    assert_eq!(result, Some((5, 9)));

    let result = text_objects::quoted(text, 6, '"', TextObjScope::Around);
    assert_eq!(result, Some((4, 10)));
}

#[test]
fn test_text_object_bracket() {
    let text = "fn(a, b)";
    let result = text_objects::bracket(text, 4, '(', ')', TextObjScope::Inner);
    assert_eq!(result, Some((3, 6)));

    let result = text_objects::bracket(text, 4, '(', ')', TextObjScope::Around);
    assert_eq!(result, Some((2, 7)));
}

#[test]
fn test_operator_delete_word() {
    let mut input = input_with("hello world", 0);
    let mut persistent = PersistentState::default();
    let deleted = operators::apply_operator(&mut input, Operator::Delete, 0, 4, &mut persistent);
    assert_eq!(deleted, "hello");
    assert_eq!(input.text, " world");
    assert_eq!(persistent.register, "hello");
}

#[test]
fn test_operator_yank() {
    let mut input = input_with("hello world", 0);
    let mut persistent = PersistentState::default();
    operators::apply_operator(&mut input, Operator::Yank, 0, 4, &mut persistent);
    // Text unchanged
    assert_eq!(input.text, "hello world");
    assert_eq!(persistent.register, "hello");
}

#[test]
fn test_put_after() {
    let mut input = input_with("hello", 4);
    let persistent = PersistentState {
        register: " world".to_string(),
        ..Default::default()
    };
    operators::put_after(&mut input, &persistent);
    assert_eq!(input.text, "hello world");
}

#[test]
fn test_replace_char() {
    let mut input = input_with("hello", 0);
    operators::replace_char(&mut input, 'H');
    assert_eq!(input.text, "Hello");
}

#[test]
fn test_normal_mode_motion_h_l() {
    let mut input = input_with("hello", 2);
    let mut cmd = CommandState::Idle;
    let mut persistent = PersistentState::default();

    transitions::process_normal_key('h', &mut input, &mut cmd, &mut persistent);
    assert_eq!(input.cursor, 1);

    transitions::process_normal_key('l', &mut input, &mut cmd, &mut persistent);
    assert_eq!(input.cursor, 2);
}

#[test]
fn test_normal_mode_insert_keys() {
    let mut input = input_with("hello", 0);
    let mut cmd = CommandState::Idle;
    let mut persistent = PersistentState::default();

    let action = transitions::process_normal_key('i', &mut input, &mut cmd, &mut persistent);
    assert!(matches!(action, VimAction::EnterInsert));

    let action = transitions::process_normal_key('A', &mut input, &mut cmd, &mut persistent);
    assert!(matches!(action, VimAction::EnterInsertEnd));
}

#[test]
fn test_vim_state_mode_label() {
    let state = VimState::new();
    assert_eq!(state.mode_label(), "NORMAL");

    let mut state = VimState::new();
    state.enter_insert();
    assert_eq!(state.mode_label(), "INSERT");
}

#[test]
fn test_delete_word_via_dw() {
    let mut input = input_with("hello world", 0);
    let mut cmd = CommandState::Idle;
    let mut persistent = PersistentState::default();

    // 'd' enters operator pending
    transitions::process_normal_key('d', &mut input, &mut cmd, &mut persistent);
    assert!(matches!(cmd, CommandState::OperatorPending { .. }));

    // 'w' applies delete word
    transitions::process_normal_key('w', &mut input, &mut cmd, &mut persistent);
    assert_eq!(input.text, "world");
    assert!(matches!(cmd, CommandState::Idle));
}
