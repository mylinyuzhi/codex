use super::*;
use pretty_assertions::assert_eq;

use crate::i18n::locale_test_guard;
use crate::state::rewind::SummarizeDirection;

fn message(id: &str, text: &str) -> RewindableMessage {
    RewindableMessage {
        message_id: id.to_string(),
        message_index: 0,
        display_text: text.to_string(),
        relative_time: "2 minutes ago".to_string(),
        permission_mode: None,
        diff_stats: None,
        can_restore_code: None,
        is_current_prompt: false,
    }
}

fn overlay_with_messages(messages: Vec<RewindableMessage>) -> RewindOverlay {
    RewindOverlay {
        phase: RewindPhase::MessageSelect,
        messages,
        selected: 0,
        option_selected: 0,
        available_options: Vec::new(),
        diff_stats: None,
        file_history_enabled: true,
        has_file_changes: false,
        allow_summarize_up_to: false,
        summarize_feedback: String::new(),
        pending_summarize: None,
        preselected: false,
    }
}

#[test]
fn rewind_message_select_renders_empty_state_when_only_current_row_exists() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut current = message("", "(current)");
    current.is_current_prompt = true;
    let overlay = overlay_with_messages(vec![current]);

    let (title, body, border) = rewind_overlay_content(&overlay, &theme);

    assert_eq!(title, " Rewind ");
    assert_eq!(border, theme.accent);
    assert!(body.contains("Nothing to rewind to yet."));
    assert!(body.contains("Esc Close"));
}

#[test]
fn rewind_message_select_renders_diff_metadata_and_scroll_hint() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut messages = (0..9)
        .map(|i| message(&format!("msg-{i}"), &format!("message {i}")))
        .collect::<Vec<_>>();
    messages[6].diff_stats = Some(DiffStatsPreview {
        files_changed: 1,
        insertions: 3,
        deletions: 1,
        file_paths: vec!["src/main.rs".to_string()],
    });
    let mut overlay = overlay_with_messages(messages);
    overlay.selected = 6;

    let (_, body, _) = rewind_overlay_content(&overlay, &theme);

    assert!(body.contains("Restore the code and/or conversation"));
    assert!(body.contains("> message 6 (2 minutes ago)"));
    assert!(body.contains("main.rs changed +3 -1"));
    assert!(body.contains("(7/9)"));
    assert!(!body.contains("message 0"));
}

#[test]
fn rewind_restore_options_describes_code_restore_and_manual_warning() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut overlay = overlay_with_messages(vec![message("msg-1", "fix bug")]);
    overlay.phase = RewindPhase::RestoreOptions;
    overlay.available_options = vec![
        RestoreType::Both,
        RestoreType::ConversationOnly,
        RestoreType::Nevermind,
    ];
    overlay.diff_stats = Some(DiffStatsPreview {
        files_changed: 2,
        insertions: 10,
        deletions: 4,
        file_paths: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
    });
    overlay.has_file_changes = true;

    let (_, body, _) = rewind_overlay_content(&overlay, &theme);

    assert!(body.contains("Confirm you want to restore"));
    assert!(body.contains("> Restore code and conversation"));
    assert!(body.contains("The conversation will be forked."));
    assert!(body.contains("The code will be restored: +10 -4 in main.rs and lib.rs."));
    assert!(body.contains("Rewinding does not affect files edited manually or via bash."));
}

#[test]
fn rewind_restore_options_clamps_negative_selection() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut overlay = overlay_with_messages(vec![message("msg-1", "fix bug")]);
    overlay.phase = RewindPhase::RestoreOptions;
    overlay.selected = -5;
    overlay.option_selected = -2;
    overlay.available_options = vec![RestoreType::Both, RestoreType::Nevermind];

    let (_, body, _) = rewind_overlay_content(&overlay, &theme);

    assert!(body.contains("│ fix bug"));
    assert!(body.contains("> Restore code and conversation"));
}

#[test]
fn rewind_summarize_feedback_and_confirming_use_pending_state() {
    let _locale = locale_test_guard("en");
    let theme = Theme::default();
    let mut overlay = overlay_with_messages(vec![message("msg-1", "fix bug")]);
    overlay.phase = RewindPhase::SummarizeFeedback;

    let (_, empty_body, _) = rewind_overlay_content(&overlay, &theme);
    assert!(empty_body.contains("add context (optional)"));
    assert!(empty_body.contains("(empty submit cancels)"));

    overlay.summarize_feedback = "keep errors".to_string();
    let (_, typed_body, _) = rewind_overlay_content(&overlay, &theme);
    assert!(typed_body.contains("> keep errors"));

    overlay.phase = RewindPhase::Confirming;
    overlay.pending_summarize = Some(SummarizeDirection::From);
    let (_, confirming_body, _) = rewind_overlay_content(&overlay, &theme);
    assert!(confirming_body.contains("Summarizing"));
}
