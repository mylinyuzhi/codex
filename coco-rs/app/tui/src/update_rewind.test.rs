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
fn test_build_rewind_overlay_appends_synthetic_current_row() {
    // TS `MessageSelector.tsx:60-66` appends a virtual `(current)` row
    // at the end of `messageOptions`. After the synthetic-row landing
    // there are N+1 entries: N real + 1 synthetic, with selection on
    // the synthetic row.
    let state = make_state_with_messages(3);
    let overlay = build_rewind_overlay(&state);

    assert_eq!(overlay.messages.len(), 4);
    let last = overlay.messages.last().unwrap();
    assert!(last.is_current_prompt);
    assert_eq!(last.message_id, "");
    assert_eq!(overlay.selected, 3);
    assert_eq!(overlay.phase, RewindPhase::MessageSelect);
    // The last *real* message is at index N-1 = 2.
    assert_eq!(overlay.messages[2].message_id, "msg-2");
    assert!(!overlay.messages[2].is_current_prompt);
}

#[test]
fn test_handle_rewind_nav() {
    let state = make_state_with_messages(5);
    let mut overlay = build_rewind_overlay(&state);
    // Synthetic current-prompt row at index 5 (5 real + synthetic).
    assert_eq!(overlay.selected, 5);

    handle_rewind_nav(&mut overlay, -1);
    assert_eq!(overlay.selected, 4); // last real

    handle_rewind_nav(&mut overlay, -10);
    assert_eq!(overlay.selected, 0); // first real

    handle_rewind_nav(&mut overlay, 100);
    assert_eq!(overlay.selected, 5); // back to synthetic
}

#[test]
fn test_handle_rewind_confirm_synthetic_row_dismisses() {
    // TS `MessageSelector.tsx:165` — `if (!messages.includes(message_0)) onClose()`.
    // Confirm on the virtual current-prompt row never dispatches; it
    // closes the overlay (equivalent to Esc).
    let state = make_state_with_messages(3);
    let mut overlay = build_rewind_overlay(&state);
    // Default selection lands on the synthetic row.
    let outcome = handle_rewind_confirm(&mut overlay);
    assert_eq!(outcome, ConfirmOutcome::Dismiss);
}

#[test]
fn test_handle_rewind_confirm_transitions_to_options() {
    let state = make_state_with_messages(3);
    let mut overlay = build_rewind_overlay(&state);
    overlay.file_history_enabled = true;

    // Move off the synthetic row to the last real message first.
    handle_rewind_nav(&mut overlay, -1);
    assert!(!overlay.messages[overlay.selected as usize].is_current_prompt);

    // Confirm in MessageSelect -> transitions to RestoreOptions when
    // file history is enabled (TS path that loads diffStatsForRestore).
    let outcome = handle_rewind_confirm(&mut overlay);
    assert_eq!(outcome, ConfirmOutcome::Phase);
    assert_eq!(overlay.phase, RewindPhase::RestoreOptions);
    assert!(!overlay.available_options.is_empty());
}

#[test]
fn test_handle_rewind_confirm_file_history_off_dispatches_directly() {
    // TS `MessageSelector.tsx:169-172`: when file history is disabled
    // the picker bypasses the option screen and immediately dispatches
    // a conversation rewind (`restoreConversationDirectly`).
    let state = make_state_with_messages(2);
    let mut overlay = build_rewind_overlay(&state);
    overlay.file_history_enabled = false;
    handle_rewind_nav(&mut overlay, -1); // off the synthetic row
    let outcome = handle_rewind_confirm(&mut overlay);
    let (msg_id, restore_type) = expect_dispatch(outcome, "file_history_off");
    assert_eq!(msg_id, "msg-1");
    assert_eq!(restore_type, RestoreType::ConversationOnly);
    assert_eq!(overlay.phase, RewindPhase::MessageSelect);
}

#[test]
fn test_handle_rewind_confirm_returns_selection() {
    let state = make_state_with_messages(2);
    let mut overlay = build_rewind_overlay(&state);
    overlay.file_history_enabled = true;
    overlay.has_file_changes = true;
    handle_rewind_nav(&mut overlay, -1); // off synthetic

    // Transition to RestoreOptions
    let phase_outcome = handle_rewind_confirm(&mut overlay);
    assert_eq!(phase_outcome, ConfirmOutcome::Phase);
    assert_eq!(overlay.phase, RewindPhase::RestoreOptions);

    // Confirm first option (Both)
    let outcome = handle_rewind_confirm(&mut overlay);
    let (msg_id, restore_type) = expect_dispatch(outcome, "first option");
    assert_eq!(msg_id, "msg-1");
    assert_eq!(restore_type, RestoreType::Both);
}

#[test]
fn test_handle_rewind_confirm_nevermind_returns_to_message_select() {
    let state = make_state_with_messages(2);
    let mut overlay = build_rewind_overlay(&state);
    overlay.file_history_enabled = true;
    overlay.has_file_changes = true;
    handle_rewind_nav(&mut overlay, -1);

    handle_rewind_confirm(&mut overlay); // -> RestoreOptions
    assert_eq!(overlay.phase, RewindPhase::RestoreOptions);

    // Move selection to the Nevermind option (always last).
    overlay.option_selected = (overlay.available_options.len() as i32) - 1;
    assert_eq!(
        overlay.available_options[overlay.option_selected as usize],
        RestoreType::Nevermind
    );

    let outcome = handle_rewind_confirm(&mut overlay);
    assert_eq!(outcome, ConfirmOutcome::Phase, "Nevermind never dispatches");
    assert_eq!(
        overlay.phase,
        RewindPhase::MessageSelect,
        "Nevermind returns to message picker, mirroring TS"
    );
    assert!(overlay.available_options.is_empty());
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
    overlay.file_history_enabled = true;
    handle_rewind_nav(&mut overlay, -1);

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
    // 3 real messages + 1 synthetic = 4 entries, fits inside MAX_VISIBLE.
    assert_eq!(end, 4);
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

#[test]
fn test_summarize_feedback_empty_submit_cancels_to_options() {
    // TS `MessageSelector.tsx` summarize input declares
    // `allowEmptySubmitToCancel: true`. Empty submit must NOT dispatch
    // a rewind — it returns to the option list with feedback cleared.
    let state = make_state_with_messages(2);
    let mut overlay = build_rewind_overlay(&state);
    overlay.file_history_enabled = true;
    handle_rewind_nav(&mut overlay, -1);

    handle_rewind_confirm(&mut overlay); // -> RestoreOptions
    overlay.option_selected = overlay
        .available_options
        .iter()
        .position(|o| matches!(o, RestoreType::SummarizeFrom { .. }))
        .expect("SummarizeFrom must be offered") as i32;
    handle_rewind_confirm(&mut overlay); // -> SummarizeFeedback
    assert_eq!(overlay.phase, RewindPhase::SummarizeFeedback);
    assert!(overlay.pending_summarize.is_some());

    // Empty submit
    overlay.summarize_feedback.clear();
    let outcome = handle_rewind_confirm(&mut overlay);
    assert_eq!(outcome, ConfirmOutcome::Phase);
    assert_eq!(overlay.phase, RewindPhase::RestoreOptions);
    assert!(overlay.pending_summarize.is_none());
    assert!(overlay.summarize_feedback.is_empty());
}

#[test]
fn test_summarize_feedback_with_text_dispatches_rewind() {
    let state = make_state_with_messages(2);
    let mut overlay = build_rewind_overlay(&state);
    overlay.file_history_enabled = true;
    handle_rewind_nav(&mut overlay, -1);

    handle_rewind_confirm(&mut overlay);
    overlay.option_selected = overlay
        .available_options
        .iter()
        .position(|o| matches!(o, RestoreType::SummarizeFrom { .. }))
        .expect("SummarizeFrom must be offered") as i32;
    handle_rewind_confirm(&mut overlay);
    assert_eq!(overlay.phase, RewindPhase::SummarizeFeedback);

    overlay.summarize_feedback = "trim me  ".to_string();
    let outcome = handle_rewind_confirm(&mut overlay);
    let (msg_id, restore) = expect_dispatch(outcome, "summarize feedback");
    assert_eq!(msg_id, "msg-1");
    match restore {
        RestoreType::SummarizeFrom { feedback } => {
            assert_eq!(feedback.as_deref(), Some("trim me"));
        }
        other => panic!("expected SummarizeFrom, got {other:?}"),
    }
    // pending_summarize stays set so Confirming phase can show
    // "Summarizing…" while the engine processes the request.
    assert!(overlay.pending_summarize.is_some());
}

#[test]
fn test_picker_is_empty_with_only_synthetic_row() {
    // TS `hasMessagesToSelect = messageOptions.length > 1` (line 71).
    // No real user messages → picker considered empty even though the
    // synthetic row is present.
    let state = AppState::new();
    let overlay = build_rewind_overlay(&state);
    assert_eq!(overlay.messages.len(), 1);
    assert!(overlay.messages[0].is_current_prompt);
    assert!(picker_is_empty(&overlay));
}

#[test]
fn test_picker_is_empty_false_with_real_messages() {
    let state = make_state_with_messages(1);
    let overlay = build_rewind_overlay(&state);
    assert!(!picker_is_empty(&overlay));
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
fn test_build_rewind_overlay_for_jumps_to_options_phase() {
    // TS `MessageSelector.tsx:42-44, 72-83`: when `preselectedMessage`
    // is provided, the picker skips the message-select phase entirely
    // and lands on the confirm screen with that message selected.
    let state = make_state_with_messages(3);
    let overlay = build_rewind_overlay_for(&state, Some("msg-1"));
    assert_eq!(overlay.phase, RewindPhase::RestoreOptions);
    assert!(overlay.preselected);
    let row = &overlay.messages[overlay.selected as usize];
    assert_eq!(row.message_id, "msg-1");
    assert!(!row.is_current_prompt);
    assert!(!overlay.available_options.is_empty());
}

#[test]
fn test_build_rewind_overlay_for_unknown_id_falls_back_to_pick_list() {
    let state = make_state_with_messages(2);
    let overlay = build_rewind_overlay_for(&state, Some("does-not-exist"));
    assert_eq!(overlay.phase, RewindPhase::MessageSelect);
    assert!(!overlay.preselected);
}

#[test]
fn test_build_rewind_overlay_for_none_matches_plain_builder() {
    let state = make_state_with_messages(2);
    let plain = build_rewind_overlay(&state);
    let via_for = build_rewind_overlay_for(&state, None);
    assert_eq!(plain.phase, via_for.phase);
    assert_eq!(plain.messages.len(), via_for.messages.len());
    assert_eq!(plain.preselected, via_for.preselected);
    assert!(!via_for.preselected);
}

#[test]
fn test_handle_rewind_cancel_preselected_dismisses_from_options() {
    // TS `MessageSelector.tsx:248-253`: when launched preselected, Esc
    // closes the overlay entirely — there's no message list to step
    // back to.
    let state = make_state_with_messages(2);
    let mut overlay = build_rewind_overlay_for(&state, Some("msg-1"));
    assert_eq!(overlay.phase, RewindPhase::RestoreOptions);
    assert!(overlay.preselected);

    assert!(
        handle_rewind_cancel(&mut overlay),
        "cancel must dismiss when preselected"
    );
}

#[test]
fn test_handle_rewind_confirm_nevermind_preselected_dismisses() {
    // TS `MessageSelector.tsx:185-188`: nevermind on a preselected
    // launch dismisses (TS line 186: `if (preselectedMessage) onClose()`).
    let state = make_state_with_messages(2);
    let mut overlay = build_rewind_overlay_for(&state, Some("msg-1"));
    overlay.option_selected = (overlay.available_options.len() as i32) - 1;
    assert_eq!(
        overlay.available_options[overlay.option_selected as usize],
        RestoreType::Nevermind
    );
    let outcome = handle_rewind_confirm(&mut overlay);
    assert_eq!(outcome, ConfirmOutcome::Dismiss);
}
