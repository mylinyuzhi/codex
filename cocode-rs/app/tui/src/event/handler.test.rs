use super::*;
use crossterm::event::KeyEventKind;

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Press)
}

#[test]
fn test_tab_toggles_plan_mode() {
    let event = key(KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(
        handle_key_event(event, false),
        Some(TuiCommand::TogglePlanMode)
    );
}

#[test]
fn test_ctrl_t_cycles_thinking() {
    let event = key(KeyCode::Char('t'), KeyModifiers::CONTROL);
    assert_eq!(
        handle_key_event(event, false),
        Some(TuiCommand::CycleThinkingLevel)
    );
}

#[test]
fn test_ctrl_m_cycles_model() {
    let event = key(KeyCode::Char('m'), KeyModifiers::CONTROL);
    assert_eq!(handle_key_event(event, false), Some(TuiCommand::CycleModel));
}

#[test]
fn test_ctrl_c_interrupts() {
    let event = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(handle_key_event(event, false), Some(TuiCommand::Interrupt));
}

#[test]
fn test_enter_submits() {
    let event = key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        handle_key_event(event, false),
        Some(TuiCommand::SubmitInput)
    );
}

#[test]
fn test_shift_enter_inserts_newline() {
    // Shift+Enter inserts newline (aligned with Claude Code behavior)
    let event = key(KeyCode::Enter, KeyModifiers::SHIFT);
    assert_eq!(
        handle_key_event(event, false),
        Some(TuiCommand::InsertNewline)
    );
}

#[test]
fn test_alt_enter_inserts_newline() {
    // Alt+Enter inserts newline for multi-line input
    let event = key(KeyCode::Enter, KeyModifiers::ALT);
    assert_eq!(
        handle_key_event(event, false),
        Some(TuiCommand::InsertNewline)
    );
}

#[test]
fn test_char_inserts() {
    let event = key(KeyCode::Char('a'), KeyModifiers::NONE);
    assert_eq!(
        handle_key_event(event, false),
        Some(TuiCommand::InsertChar('a'))
    );
}

#[test]
fn test_overlay_y_approves() {
    let event = key(KeyCode::Char('y'), KeyModifiers::NONE);
    assert_eq!(handle_key_event(event, true), Some(TuiCommand::Approve));
}

#[test]
fn test_overlay_n_denies() {
    let event = key(KeyCode::Char('n'), KeyModifiers::NONE);
    assert_eq!(handle_key_event(event, true), Some(TuiCommand::Deny));
}

#[test]
fn test_escape_cancels() {
    let event = key(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(handle_key_event(event, false), Some(TuiCommand::Cancel));
}

#[test]
fn test_ctrl_left_word_left() {
    let event = key(KeyCode::Left, KeyModifiers::CONTROL);
    assert_eq!(handle_key_event(event, false), Some(TuiCommand::WordLeft));
}

#[test]
fn test_ctrl_right_word_right() {
    let event = key(KeyCode::Right, KeyModifiers::CONTROL);
    assert_eq!(handle_key_event(event, false), Some(TuiCommand::WordRight));
}

#[test]
fn test_ctrl_backspace_delete_word() {
    let event = key(KeyCode::Backspace, KeyModifiers::CONTROL);
    assert_eq!(
        handle_key_event(event, false),
        Some(TuiCommand::DeleteWordBackward)
    );
}

#[test]
fn test_ctrl_delete_delete_word_forward() {
    let event = key(KeyCode::Delete, KeyModifiers::CONTROL);
    assert_eq!(
        handle_key_event(event, false),
        Some(TuiCommand::DeleteWordForward)
    );
}

#[test]
fn test_f1_shows_help() {
    let event = key(KeyCode::F(1), KeyModifiers::NONE);
    assert_eq!(handle_key_event(event, false), Some(TuiCommand::ShowHelp));
}

#[test]
fn test_question_mark_shows_help() {
    let event = key(KeyCode::Char('?'), KeyModifiers::SHIFT);
    assert_eq!(handle_key_event(event, false), Some(TuiCommand::ShowHelp));
}

#[test]
fn test_ctrl_shift_t_toggles_thinking() {
    let event = key(
        KeyCode::Char('T'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert_eq!(
        handle_key_event(event, false),
        Some(TuiCommand::ToggleThinking)
    );
}

// ========== Streaming-aware tests ==========

#[test]
fn test_enter_while_streaming_queues_input() {
    let event = key(KeyCode::Enter, KeyModifiers::NONE);
    // When streaming, Enter should queue instead of submit
    assert_eq!(
        handle_key_event_full(event, false, false, false, false, true),
        Some(TuiCommand::QueueInput)
    );
}

#[test]
fn test_enter_while_not_streaming_submits() {
    let event = key(KeyCode::Enter, KeyModifiers::NONE);
    // When not streaming, Enter should submit
    assert_eq!(
        handle_key_event_full(event, false, false, false, false, false),
        Some(TuiCommand::SubmitInput)
    );
}

#[test]
fn test_ctrl_enter_matches_enter_behavior() {
    let event = key(KeyCode::Enter, KeyModifiers::CONTROL);
    // Ctrl+Enter behaves the same as Enter: queue when streaming, submit otherwise
    assert_eq!(
        handle_key_event_full(event, false, false, false, false, true),
        Some(TuiCommand::QueueInput)
    );
    assert_eq!(
        handle_key_event_full(event, false, false, false, false, false),
        Some(TuiCommand::SubmitInput)
    );
}

#[test]
fn test_shift_enter_inserts_newline_regardless_of_streaming() {
    let event = key(KeyCode::Enter, KeyModifiers::SHIFT);
    // Shift+Enter inserts newline regardless of streaming state
    assert_eq!(
        handle_key_event_full(event, false, false, false, false, true),
        Some(TuiCommand::InsertNewline)
    );
    assert_eq!(
        handle_key_event_full(event, false, false, false, false, false),
        Some(TuiCommand::InsertNewline)
    );
}
