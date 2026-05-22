use crate::state::AppState;
use crate::state::derive::test_helpers;
use crate::state::rewind::RestoreType;
use crate::state::rewind::RewindPhase;

use super::*;

/// Map a legacy test id ("msg-1") to the v5 UUID string that the
/// cell-push helper produces. `m.message_id` on rewind rows is the
/// stringified `cell.message_uuid`, so assertions that compare to a
/// human-readable test id must route through the same derivation.
fn test_id(s: &str) -> String {
    crate::state::derive::id_to_uuid(s).to_string()
}

fn make_state_with_messages(count: i32) -> AppState {
    let mut state = AppState::new();
    for i in 0..count {
        test_helpers::push_user_text(
            &mut state.session,
            &format!("msg-{i}"),
            &format!("Hello turn {i}"),
        );
        test_helpers::push_assistant_text(&mut state.session, "Hi there");
    }
    state
}

/// Helper: extract dispatch tuple from a `ConfirmOutcome::Dispatch`,
/// panicking with a message on any other variant.
fn expect_dispatch(outcome: ConfirmOutcome, ctx: &str) -> (String, RestoreType) {
    match outcome {
        ConfirmOutcome::Dispatch {
            message_id,
            restore,
        } => (message_id, restore),
        other => panic!("{ctx}: expected Dispatch, got {other:?}"),
    }
}

#[test]
fn test_build_rewind_state_appends_synthetic_current_row() {
    // TS `MessageSelector.tsx:60-66` appends a virtual `(current)` row
    // at the end of `messageOptions`. After the synthetic-row landing
    // there are N+1 entries: N real + 1 synthetic, with selection on
    // the synthetic row.
    let state = make_state_with_messages(3);
    let state = build_rewind_state(&state);

    assert_eq!(state.messages.len(), 4);
    let last = state.messages.last().unwrap();
    assert!(last.is_current_prompt);
    assert_eq!(last.message_id, "");
    assert_eq!(state.selected, 3);
    assert_eq!(state.phase, RewindPhase::MessageSelect);
    // The last *real* message is at index N-1 = 2.
    assert_eq!(state.messages[2].message_id, test_id("msg-2"));
    assert!(!state.messages[2].is_current_prompt);
}

#[test]
fn test_handle_rewind_nav() {
    let state = make_state_with_messages(5);
    let mut state = build_rewind_state(&state);
    // Synthetic current-prompt row at index 5 (5 real + synthetic).
    assert_eq!(state.selected, 5);

    handle_rewind_nav(&mut state, -1);
    assert_eq!(state.selected, 4); // last real

    handle_rewind_nav(&mut state, -10);
    assert_eq!(state.selected, 0); // first real

    handle_rewind_nav(&mut state, 100);
    assert_eq!(state.selected, 5); // back to synthetic
}

#[test]
fn test_handle_rewind_confirm_synthetic_row_dismisses() {
    // TS `MessageSelector.tsx:165` — `if (!messages.includes(message_0)) onClose()`.
    // Confirm on the virtual current-prompt row never dispatches; it
    // closes the state (equivalent to Esc).
    let state = make_state_with_messages(3);
    let mut state = build_rewind_state(&state);
    // Default selection lands on the synthetic row.
    let outcome = handle_rewind_confirm(&mut state);
    assert_eq!(outcome, ConfirmOutcome::Dismiss);
}

#[test]
fn test_handle_rewind_confirm_transitions_to_options() {
    let state = make_state_with_messages(3);
    let mut state = build_rewind_state(&state);
    state.file_history_enabled = true;

    // Move off the synthetic row to the last real message first.
    handle_rewind_nav(&mut state, -1);
    assert!(!state.messages[state.selected as usize].is_current_prompt);

    // Confirm in MessageSelect -> transitions to RestoreOptions when
    // file history is enabled (TS path that loads diffStatsForRestore).
    let outcome = handle_rewind_confirm(&mut state);
    assert_eq!(outcome, ConfirmOutcome::Phase);
    assert_eq!(state.phase, RewindPhase::RestoreOptions);
    assert!(!state.available_options.is_empty());
}

#[test]
fn test_handle_rewind_confirm_file_history_off_dispatches_directly() {
    // TS `MessageSelector.tsx:169-172`: when file history is disabled
    // the picker bypasses the option screen and immediately dispatches
    // a conversation rewind (`restoreConversationDirectly`).
    let state = make_state_with_messages(2);
    let mut state = build_rewind_state(&state);
    state.file_history_enabled = false;
    handle_rewind_nav(&mut state, -1); // off the synthetic row
    let outcome = handle_rewind_confirm(&mut state);
    let (msg_id, restore_type) = expect_dispatch(outcome, "file_history_off");
    assert_eq!(msg_id, test_id("msg-1"));
    assert_eq!(restore_type, RestoreType::ConversationOnly);
    assert_eq!(state.phase, RewindPhase::MessageSelect);
}

#[test]
fn test_handle_rewind_confirm_returns_selection() {
    let state = make_state_with_messages(2);
    let mut state = build_rewind_state(&state);
    state.file_history_enabled = true;
    state.has_file_changes = true;
    handle_rewind_nav(&mut state, -1); // off synthetic

    // Transition to RestoreOptions
    let phase_outcome = handle_rewind_confirm(&mut state);
    assert_eq!(phase_outcome, ConfirmOutcome::Phase);
    assert_eq!(state.phase, RewindPhase::RestoreOptions);

    // Confirm first option (Both)
    let outcome = handle_rewind_confirm(&mut state);
    let (msg_id, restore_type) = expect_dispatch(outcome, "first option");
    assert_eq!(msg_id, test_id("msg-1"));
    assert_eq!(restore_type, RestoreType::Both);
}

#[test]
fn test_handle_rewind_confirm_nevermind_returns_to_message_select() {
    let state = make_state_with_messages(2);
    let mut state = build_rewind_state(&state);
    state.file_history_enabled = true;
    state.has_file_changes = true;
    handle_rewind_nav(&mut state, -1);

    handle_rewind_confirm(&mut state); // -> RestoreOptions
    assert_eq!(state.phase, RewindPhase::RestoreOptions);

    // Move selection to the Nevermind option (always last).
    state.option_selected = (state.available_options.len() as i32) - 1;
    assert_eq!(
        state.available_options[state.option_selected as usize],
        RestoreType::Nevermind
    );

    let outcome = handle_rewind_confirm(&mut state);
    assert_eq!(outcome, ConfirmOutcome::Phase, "Nevermind never dispatches");
    assert_eq!(
        state.phase,
        RewindPhase::MessageSelect,
        "Nevermind returns to message picker, mirroring TS"
    );
    assert!(state.available_options.is_empty());
}

#[test]
fn test_handle_rewind_cancel_dismiss_from_message_select() {
    let state = make_state_with_messages(2);
    let mut state = build_rewind_state(&state);

    assert!(handle_rewind_cancel(&mut state));
}

#[test]
fn test_handle_rewind_cancel_goes_back_from_options() {
    let state = make_state_with_messages(2);
    let mut state = build_rewind_state(&state);
    state.file_history_enabled = true;
    handle_rewind_nav(&mut state, -1);

    handle_rewind_confirm(&mut state); // -> RestoreOptions
    assert_eq!(state.phase, RewindPhase::RestoreOptions);

    assert!(!handle_rewind_cancel(&mut state)); // -> back to MessageSelect
    assert_eq!(state.phase, RewindPhase::MessageSelect);
}

#[test]
fn test_summarize_feedback_empty_submit_cancels_to_options() {
    // TS `MessageSelector.tsx` summarize input declares
    // `allowEmptySubmitToCancel: true`. Empty submit must NOT dispatch
    // a rewind — it returns to the option list with feedback cleared.
    let state = make_state_with_messages(2);
    let mut state = build_rewind_state(&state);
    state.file_history_enabled = true;
    handle_rewind_nav(&mut state, -1);

    handle_rewind_confirm(&mut state); // -> RestoreOptions
    state.option_selected = state
        .available_options
        .iter()
        .position(|o| matches!(o, RestoreType::SummarizeFrom { .. }))
        .expect("SummarizeFrom must be offered") as i32;
    handle_rewind_confirm(&mut state); // -> SummarizeFeedback
    assert_eq!(state.phase, RewindPhase::SummarizeFeedback);
    assert!(state.pending_summarize.is_some());

    // Empty submit
    state.summarize_feedback.clear();
    let outcome = handle_rewind_confirm(&mut state);
    assert_eq!(outcome, ConfirmOutcome::Phase);
    assert_eq!(state.phase, RewindPhase::RestoreOptions);
    assert!(state.pending_summarize.is_none());
    assert!(state.summarize_feedback.is_empty());
}

#[test]
fn test_summarize_feedback_with_text_dispatches_rewind() {
    let state = make_state_with_messages(2);
    let mut state = build_rewind_state(&state);
    state.file_history_enabled = true;
    handle_rewind_nav(&mut state, -1);

    handle_rewind_confirm(&mut state);
    state.option_selected = state
        .available_options
        .iter()
        .position(|o| matches!(o, RestoreType::SummarizeFrom { .. }))
        .expect("SummarizeFrom must be offered") as i32;
    handle_rewind_confirm(&mut state);
    assert_eq!(state.phase, RewindPhase::SummarizeFeedback);

    state.summarize_feedback = "trim me  ".to_string();
    let outcome = handle_rewind_confirm(&mut state);
    let (msg_id, restore) = expect_dispatch(outcome, "summarize feedback");
    assert_eq!(msg_id, test_id("msg-1"));
    match restore {
        RestoreType::SummarizeFrom { feedback } => {
            assert_eq!(feedback.as_deref(), Some("trim me"));
        }
        other => panic!("expected SummarizeFrom, got {other:?}"),
    }
    // pending_summarize stays set so Confirming phase can show
    // "Summarizing…" while the engine processes the request.
    assert!(state.pending_summarize.is_some());
}

#[test]
fn test_strip_ide_context_tags_removes_blocks() {
    let input = "Hello <ide_opened_file path='a'>file body</ide_opened_file>\nworld";
    assert_eq!(strip_ide_context_tags(input), "Hello \nworld");
    let nested = "<ide_selection>x</ide_selection>only text";
    assert_eq!(strip_ide_context_tags(nested), "only text");
    let plain = "no tags here";
    assert_eq!(strip_ide_context_tags(plain), "no tags here");
}

#[test]
fn test_strip_prompt_xml_tags_drops_known_blocks() {
    let input = "<commit_analysis>x</commit_analysis>real content";
    assert_eq!(strip_prompt_xml_tags(input), "real content");
    let preserved = "<other_tag>kept</other_tag>";
    assert_eq!(strip_prompt_xml_tags(preserved), preserved);
}

// Coverage for the lossless-tail logic moved into the
// `on_turn_interrupted` matrix in `protocol.test.rs` — auto-restore is
// no longer a standalone helper. `find_last_user_message_index` +
// `messages_after_are_only_synthetic` are still covered by tests
// above + by the protocol-layer matrix.

// ── #3 preselectedMessage flow ───────────────────────────────────

#[test]
fn test_build_rewind_state_for_jumps_to_options_phase() {
    // TS `MessageSelector.tsx:42-44, 72-83`: when `preselectedMessage`
    // is provided, the picker skips the message-select phase entirely
    // and lands on the confirm screen with that message selected.
    let state = make_state_with_messages(3);
    let state = build_rewind_state_for(&state, Some("msg-1"));
    assert_eq!(state.phase, RewindPhase::RestoreOptions);
    assert!(state.preselected);
    let row = &state.messages[state.selected as usize];
    assert_eq!(row.message_id, test_id("msg-1"));
    assert!(!row.is_current_prompt);
    assert!(!state.available_options.is_empty());
}

#[test]
fn test_build_rewind_state_for_unknown_id_falls_back_to_pick_list() {
    let state = make_state_with_messages(2);
    let state = build_rewind_state_for(&state, Some("does-not-exist"));
    assert_eq!(state.phase, RewindPhase::MessageSelect);
    assert!(!state.preselected);
}

#[test]
fn test_build_rewind_state_for_none_matches_plain_builder() {
    let state = make_state_with_messages(2);
    let plain = build_rewind_state(&state);
    let via_for = build_rewind_state_for(&state, None);
    assert_eq!(plain.phase, via_for.phase);
    assert_eq!(plain.messages.len(), via_for.messages.len());
    assert_eq!(plain.preselected, via_for.preselected);
    assert!(!via_for.preselected);
}

#[test]
fn test_handle_rewind_cancel_preselected_dismisses_from_options() {
    // TS `MessageSelector.tsx:248-253`: when launched preselected, Esc
    // closes the state entirely — there's no message list to step
    // back to.
    let state = make_state_with_messages(2);
    let mut state = build_rewind_state_for(&state, Some("msg-1"));
    assert_eq!(state.phase, RewindPhase::RestoreOptions);
    assert!(state.preselected);

    assert!(
        handle_rewind_cancel(&mut state),
        "cancel must dismiss when preselected"
    );
}

#[test]
fn test_handle_rewind_confirm_nevermind_preselected_dismisses() {
    // TS `MessageSelector.tsx:185-188`: nevermind on a preselected
    // launch dismisses (TS line 186: `if (preselectedMessage) onClose()`).
    let state = make_state_with_messages(2);
    let mut state = build_rewind_state_for(&state, Some("msg-1"));
    state.option_selected = (state.available_options.len() as i32) - 1;
    assert_eq!(
        state.available_options[state.option_selected as usize],
        RestoreType::Nevermind
    );
    let outcome = handle_rewind_confirm(&mut state);
    assert_eq!(outcome, ConfirmOutcome::Dismiss);
}
