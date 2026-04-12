use crate::state::AppState;
use crate::state::ChatMessage;
use crate::state::rewind::RestoreType;
use crate::state::rewind::RewindPhase;

use super::*;

fn make_state_with_messages(count: i32) -> AppState {
    let mut state = AppState::new();
    for i in 0..count {
        state.session.add_message(ChatMessage::user_text(
            format!("msg-{i}"),
            format!("Hello turn {i}"),
        ));
        state
            .session
            .add_message(ChatMessage::assistant_text(format!("resp-{i}"), "Hi there"));
    }
    state
}

#[test]
fn test_build_rewind_overlay_extracts_user_messages() {
    let state = make_state_with_messages(3);
    let overlay = build_rewind_overlay(&state);

    assert_eq!(overlay.messages.len(), 3);
    assert_eq!(overlay.messages[0].turn_label, "Turn 1");
    assert_eq!(overlay.messages[2].turn_label, "Turn 3");
    // Selected defaults to last
    assert_eq!(overlay.selected, 2);
    assert_eq!(overlay.phase, RewindPhase::MessageSelect);
}

#[test]
fn test_handle_rewind_nav() {
    let state = make_state_with_messages(5);
    let mut overlay = build_rewind_overlay(&state);
    assert_eq!(overlay.selected, 4);

    handle_rewind_nav(&mut overlay, -1);
    assert_eq!(overlay.selected, 3);

    handle_rewind_nav(&mut overlay, -10);
    assert_eq!(overlay.selected, 0);

    handle_rewind_nav(&mut overlay, 100);
    assert_eq!(overlay.selected, 4);
}

#[test]
fn test_handle_rewind_confirm_transitions_to_options() {
    let state = make_state_with_messages(3);
    let mut overlay = build_rewind_overlay(&state);

    // Confirm in MessageSelect -> transitions to RestoreOptions
    let result = handle_rewind_confirm(&mut overlay);
    assert!(result.is_none());
    assert_eq!(overlay.phase, RewindPhase::RestoreOptions);
    assert!(!overlay.available_options.is_empty());
}

#[test]
fn test_handle_rewind_confirm_returns_selection() {
    let state = make_state_with_messages(2);
    let mut overlay = build_rewind_overlay(&state);
    overlay.file_history_enabled = true;
    overlay.has_file_changes = true;

    // Transition to RestoreOptions
    handle_rewind_confirm(&mut overlay);
    assert_eq!(overlay.phase, RewindPhase::RestoreOptions);

    // Confirm first option (Both)
    let result = handle_rewind_confirm(&mut overlay);
    assert!(result.is_some());
    let (msg_id, restore_type) = result.expect("should have selection");
    assert_eq!(msg_id, "msg-1");
    assert_eq!(restore_type, RestoreType::Both);
}

#[test]
fn test_handle_rewind_cancel_dismiss_from_message_select() {
    let state = make_state_with_messages(2);
    let mut overlay = build_rewind_overlay(&state);

    assert!(handle_rewind_cancel(&mut overlay));
}

#[test]
fn test_handle_rewind_cancel_goes_back_from_options() {
    let state = make_state_with_messages(2);
    let mut overlay = build_rewind_overlay(&state);

    handle_rewind_confirm(&mut overlay); // -> RestoreOptions
    assert_eq!(overlay.phase, RewindPhase::RestoreOptions);

    assert!(!handle_rewind_cancel(&mut overlay)); // -> back to MessageSelect
    assert_eq!(overlay.phase, RewindPhase::MessageSelect);
}

#[test]
fn test_visible_range_small_list() {
    let state = make_state_with_messages(3);
    let overlay = build_rewind_overlay(&state);
    let (start, end) = visible_range(&overlay);
    assert_eq!(start, 0);
    assert_eq!(end, 3);
}

#[test]
fn test_visible_range_large_list() {
    let state = make_state_with_messages(20);
    let mut overlay = build_rewind_overlay(&state);
    overlay.selected = 10;
    let (start, end) = visible_range(&overlay);
    assert_eq!(end - start, 7);
    assert!(start <= 10);
    assert!(end > 10);
}
