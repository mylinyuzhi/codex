//! Tests for the [`super::handle`] dispatch on TUI-only events that
//! mutate session state directly. Covers wiring for `AvailableCommandsRefreshed`
//! — the hot-reload path used by `/reload-plugins` to push a fresh
//! command catalogue into `state.session.available_commands`.

use pretty_assertions::assert_eq;

use coco_types::RewindDiffStatsPayload;
use coco_types::SdkSessionSummary;
use coco_types::SlashCommandInfo;
use coco_types::TuiOnlyEvent;

use super::handle;
use crate::command::SystemPushKind;
use crate::command::UserCommand;
use crate::state::AppState;
use crate::state::ModalState;
use crate::state::SuggestionKind;
use crate::state::derive::test_helpers;
use crate::state::rewind::RestoreType;
use crate::state::ui::ToastSeverity;

/// Channel pair scoped to one test. Caller drives `handle` with `&tx`
/// and observes `rx.try_recv()` for any dispatched
/// `UserCommand::PushSystemMessage { kind: Informational { .. } }`.
fn channel() -> (
    tokio::sync::mpsc::Sender<UserCommand>,
    tokio::sync::mpsc::Receiver<UserCommand>,
) {
    tokio::sync::mpsc::channel(16)
}

/// Probe: did the handler dispatch a `PushSystemMessage` whose
/// `Informational` body contains `needle`? Drains the channel; tests
/// that need more detail should call `rx.try_recv()` themselves.
fn dispatched_system_push_contains(
    rx: &mut tokio::sync::mpsc::Receiver<UserCommand>,
    needle: &str,
) -> bool {
    while let Ok(cmd) = rx.try_recv() {
        if let UserCommand::PushSystemMessage {
            kind: SystemPushKind::Informational { message, .. },
        } = &cmd
            && message.contains(needle)
        {
            return true;
        }
    }
    false
}

fn slash(name: &str) -> SlashCommandInfo {
    SlashCommandInfo {
        name: name.into(),
        description: None,
        aliases: Vec::new(),
        argument_hint: None,
    }
}

#[test]
fn available_commands_refreshed_overwrites_slot() {
    let mut state = AppState::new();
    let (tx, _rx) = channel();
    state.session.available_commands = vec![slash("old-cmd")];

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::AvailableCommandsRefreshed {
            commands: vec![slash("new-cmd-a"), slash("new-cmd-b")],
        },
        &tx,
    );

    assert!(consumed);
    let names: Vec<&str> = state
        .session
        .available_commands
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["new-cmd-a", "new-cmd-b"]);
}

#[test]
fn available_commands_refreshed_repopulates_open_popup() {
    // User had `/` popup open against the old catalogue. After reload,
    // the handler should re-run `refresh_suggestions` so the popup
    // mirrors the new list without waiting for another keystroke.
    let mut state = AppState::new();
    let (tx, _rx) = channel();
    state.session.available_commands = vec![slash("old-cmd")];
    state.ui.input.textarea.set_text("/");
    state.ui.input.textarea.set_cursor(1);
    crate::autocomplete::refresh_suggestions(&mut state);
    // Sanity check: the old list is shown.
    let initial_labels: Vec<String> = state
        .ui
        .active_suggestions
        .as_ref()
        .expect("popup installed")
        .items
        .iter()
        .map(|i| i.label.clone())
        .collect();
    assert_eq!(initial_labels, vec!["/old-cmd"]);

    handle(
        &mut state,
        TuiOnlyEvent::AvailableCommandsRefreshed {
            commands: vec![slash("fresh-cmd")],
        },
        &tx,
    );

    let sug = state
        .ui
        .active_suggestions
        .as_ref()
        .expect("popup re-installed after refresh");
    assert_eq!(sug.kind, SuggestionKind::SlashCommand);
    let labels: Vec<String> = sug.items.iter().map(|i| i.label.clone()).collect();
    assert_eq!(labels, vec!["/fresh-cmd"]);
}

#[test]
fn open_session_browser_populates_resume_picker() {
    let mut state = AppState::new();
    let (tx, _rx) = channel();

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::OpenSessionBrowser {
            sessions: vec![SdkSessionSummary {
                session_id: "s1".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                cwd: "/repo".to_string(),
                created_at: "2026-05-23T00:00:00Z".to_string(),
                updated_at: None,
                title: Some("Auth refactor".to_string()),
                message_count: 12,
                total_tokens: 345,
            }],
        },
        &tx,
    );

    assert!(consumed);
    assert_eq!(state.session.saved_sessions.len(), 1);
    assert_eq!(state.session.saved_sessions[0].label, "Auth refactor");
    let Some(ModalState::SessionBrowser(browser)) = state.ui.modal.as_ref() else {
        panic!("expected session browser modal");
    };
    assert_eq!(browser.sessions[0].id, "s1");
    assert_eq!(browser.sessions[0].label, "Auth refactor");
}

#[test]
fn rewind_completed_restores_prompt_before_message_truncation() {
    let mut state = AppState::new();
    let (tx, _rx) = channel();
    let user_uuid = test_helpers::push_user_text(&mut state.session, "u1", "continue this prompt");
    test_helpers::push_assistant_text(&mut state.session, "assistant tail");

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::RewindCompleted {
            target_message_id: user_uuid.to_string(),
            files_changed: 0,
        },
        &tx,
    );

    assert!(consumed);
    assert_eq!(state.ui.input.text(), "continue this prompt");
    assert!(state.session.conversation_id.is_some());
    assert_eq!(state.ui.toasts.len(), 1);
    assert!(
        state.session.transcript.len() > 1,
        "TUI-only completion must not truncate before the protocol event"
    );
}

// ── OpenRewindPicker — the slash-route entry point ─────────────────────

#[test]
fn open_rewind_picker_no_preselect_empty_session_opens_modal_with_inline_empty_state() {
    let mut state = AppState::new();
    let (tx, _rx) = channel();
    // Empty session: no real user messages. `build_rewind_state` still
    // appends the synthetic current-prompt row so the modal opens
    // (Esc has a target). The renderer's `picker_is_empty` check —
    // not the handler — surfaces the inline "Nothing to rewind to
    // yet." message. No toast is fired on this path.
    let consumed = handle(&mut state, TuiOnlyEvent::OpenRewindPicker, &tx);
    assert!(consumed);
    let Some(ModalState::Rewind(r)) = state.ui.modal.as_ref() else {
        panic!("expected Rewind modal even on empty session");
    };
    assert_eq!(r.messages.len(), 1, "only the synthetic current-prompt row");
    assert!(r.messages[0].is_synthetic());
    assert!(state.ui.toasts.is_empty(), "no toast on bare empty open");
}

#[test]
fn open_rewind_picker_no_preselect_opens_message_select_modal() {
    let mut state = AppState::new();
    let (tx, _rx) = channel();
    test_helpers::push_user_text(&mut state.session, "u1", "first prompt");
    test_helpers::push_assistant_text(&mut state.session, "reply");

    let consumed = handle(&mut state, TuiOnlyEvent::OpenRewindPicker, &tx);
    assert!(consumed);
    let Some(ModalState::Rewind(r)) = state.ui.modal.as_ref() else {
        panic!("expected Rewind modal, got {:?}", state.ui.modal);
    };
    assert_eq!(
        r.phase,
        crate::state::rewind::RewindPhase::MessageSelect,
        "bare /rewind opens MessageSelect (TS parity)"
    );
    assert!(!r.preselected);
    assert!(state.ui.toasts.is_empty());
}

#[test]
fn open_rewind_picker_batches_all_real_diff_stat_requests() {
    let mut state = AppState::new();
    state.session.file_history_enabled = true;
    let mut expected = Vec::new();
    for i in 0..45 {
        let id = test_helpers::push_user_text(&mut state.session, &format!("u{i}"), "prompt");
        expected.push(id);
    }
    let (tx, mut rx) = channel();

    let consumed = handle(&mut state, TuiOnlyEvent::OpenRewindPicker, &tx);
    assert!(consumed);
    let Some(ModalState::Rewind(r)) = state.ui.modal.as_ref() else {
        panic!("expected Rewind modal");
    };
    assert_eq!(
        r.messages.len(),
        46,
        "45 real rows plus synthetic current row"
    );

    let cmd = rx.try_recv().expect("batch request emitted");
    let UserCommand::RequestDiffStatsBatch { message_ids } = cmd else {
        panic!("expected RequestDiffStatsBatch, got {cmd:?}");
    };
    assert_eq!(message_ids, expected);
    assert!(rx.try_recv().is_err(), "only one batch command emitted");
}

fn diff_payload(
    _files_changed: i32,
    insertions: i64,
    deletions: i64,
    file_paths: Vec<&str>,
) -> coco_types::RewindDiffStatsPayload {
    coco_types::RewindDiffStatsPayload {
        insertions,
        deletions,
        file_paths: file_paths.iter().map(ToString::to_string).collect(),
    }
}

#[test]
fn preselected_rewind_options_wait_for_restore_preview_before_code_choices() {
    let mut state = AppState::new();
    state.session.file_history_enabled = true;
    let target = test_helpers::push_user_text(&mut state.session, "u1", "prompt");
    let rewind = crate::update_rewind::build_rewind_state_for_uuid(&state, target);
    assert_eq!(
        rewind.phase,
        crate::state::rewind::RewindPhase::RestoreOptions
    );
    assert!(
        !rewind
            .available_options
            .iter()
            .any(|o| matches!(o, RestoreType::Both | RestoreType::CodeOnly))
    );
    state.ui.show_modal(ModalState::Rewind(rewind));
    let (tx, _rx) = channel();

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::RewindRestorePreviewReady {
            message_id: target.to_string(),
            stats: Some(diff_payload(2, 4, 1, vec!["src/a.rs", "src/b.rs"])),
        },
        &tx,
    );
    assert!(consumed);
    let Some(ModalState::Rewind(r)) = state.ui.modal.as_ref() else {
        panic!("expected Rewind modal");
    };
    assert!(
        r.available_options
            .iter()
            .any(|o| matches!(o, RestoreType::Both))
    );
    assert!(
        r.available_options
            .iter()
            .any(|o| matches!(o, RestoreType::CodeOnly))
    );
}

#[test]
fn preselected_rewind_options_keep_code_choices_hidden_for_no_changes() {
    let mut state = AppState::new();
    state.session.file_history_enabled = true;
    let target = test_helpers::push_user_text(&mut state.session, "u1", "prompt");
    let rewind = crate::update_rewind::build_rewind_state_for_uuid(&state, target);
    state.ui.show_modal(ModalState::Rewind(rewind));
    let (tx, _rx) = channel();

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::RewindRestorePreviewReady {
            message_id: target.to_string(),
            stats: Some(diff_payload(0, 0, 0, vec![])),
        },
        &tx,
    );
    assert!(consumed);
    let Some(ModalState::Rewind(r)) = state.ui.modal.as_ref() else {
        panic!("expected Rewind modal");
    };
    assert!(
        !r.available_options
            .iter()
            .any(|o| matches!(o, RestoreType::Both | RestoreType::CodeOnly))
    );
}

#[test]
fn row_metadata_ready_marks_no_code_restore_when_snapshot_missing() {
    let mut state = AppState::new();
    state.session.file_history_enabled = true;
    let target = test_helpers::push_user_text(&mut state.session, "u1", "prompt");
    let rewind = crate::update_rewind::build_rewind_state(&state);
    state.ui.show_modal(ModalState::Rewind(rewind));
    let (tx, _rx) = channel();

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::RewindRowMetadataReady {
            rows: vec![coco_types::RewindRowMetadata {
                message_id: target.to_string(),
                metadata: None,
            }],
        },
        &tx,
    );

    assert!(consumed);
    let Some(ModalState::Rewind(r)) = state.ui.modal.as_ref() else {
        panic!("expected Rewind modal");
    };
    let row = r
        .messages
        .iter()
        .find(|m| m.message_id == Some(target))
        .expect("target row present");
    assert_eq!(row.can_restore_code, Some(false));
    assert!(row.diff_stats.is_none());
}

#[test]
fn row_metadata_ready_populates_per_row_stats() {
    let mut state = AppState::new();
    state.session.file_history_enabled = true;
    let target = test_helpers::push_user_text(&mut state.session, "u1", "prompt");
    let rewind = crate::update_rewind::build_rewind_state(&state);
    state.ui.show_modal(ModalState::Rewind(rewind));
    let (tx, _rx) = channel();

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::RewindRowMetadataReady {
            rows: vec![coco_types::RewindRowMetadata {
                message_id: target.to_string(),
                metadata: Some(diff_payload(1, 3, 1, vec!["src/local.rs"])),
            }],
        },
        &tx,
    );

    assert!(consumed);
    let Some(ModalState::Rewind(r)) = state.ui.modal.as_ref() else {
        panic!("expected Rewind modal");
    };
    let row = r
        .messages
        .iter()
        .find(|m| m.message_id == Some(target))
        .expect("target row present");
    let stats = row.diff_stats.as_ref().expect("row metadata populated");
    assert_eq!(row.can_restore_code, Some(true));
    assert_eq!(stats.files_changed(), 1);
    assert_eq!(stats.insertions, 3);
    assert_eq!(stats.deletions, 1);
    assert_eq!(stats.file_paths, vec!["src/local.rs"]);
    // Selected (synthetic) row is not the target — selected aggregates unchanged.
    assert!(r.diff_stats.is_none());
    assert!(r.diff_stats_message_id.is_none());
}

#[test]
fn restore_preview_ready_updates_selected_state_and_rebuilds_options() {
    let mut state = AppState::new();
    state.session.file_history_enabled = true;
    let target = test_helpers::push_user_text(&mut state.session, "u1", "prompt");
    let mut rewind = crate::update_rewind::build_rewind_state(&state);
    rewind.selected = 0;
    state.ui.show_modal(ModalState::Rewind(rewind));
    let (tx, _rx) = channel();

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::RewindRestorePreviewReady {
            message_id: target.to_string(),
            stats: Some(diff_payload(2, 7, 4, vec!["src/a.rs", "src/b.rs"])),
        },
        &tx,
    );

    assert!(consumed);
    let Some(ModalState::Rewind(r)) = state.ui.modal.as_ref() else {
        panic!("expected Rewind modal");
    };
    assert_eq!(r.diff_stats_message_id, Some(target));
    assert_eq!(
        r.diff_stats
            .as_ref()
            .map(RewindDiffStatsPayload::files_changed),
        Some(2)
    );
    assert!(r.has_file_changes);
}

#[test]
fn restore_preview_repositions_option_selected_after_rebuild() {
    // User had `ConversationOnly` focused (index 0 when no code
    // restore is offered). After RewindRestorePreviewReady prepends
    // `Both` / `CodeOnly`, the cursor must follow the original
    // RestoreType variant rather than stay at index 0.
    let mut state = AppState::new();
    state.session.file_history_enabled = true;
    let target = test_helpers::push_user_text(&mut state.session, "u1", "prompt");
    let mut rewind = crate::update_rewind::build_rewind_state_for_uuid(&state, target);
    assert_eq!(rewind.available_options[0], RestoreType::ConversationOnly);
    rewind.option_selected = 0;
    state.ui.show_modal(ModalState::Rewind(rewind));
    let (tx, _rx) = channel();

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::RewindRestorePreviewReady {
            message_id: target.to_string(),
            stats: Some(diff_payload(1, 3, 0, vec!["src/a.rs"])),
        },
        &tx,
    );

    assert!(consumed);
    let Some(ModalState::Rewind(r)) = state.ui.modal.as_ref() else {
        panic!("expected Rewind modal");
    };
    let focused = &r.available_options[r.option_selected as usize];
    assert_eq!(
        focused,
        &RestoreType::ConversationOnly,
        "cursor follows variant after rebuild"
    );
}

#[test]
fn available_commands_refreshed_with_no_open_popup_is_noop_for_popup_state() {
    // No `/` query in flight — handler still updates the catalogue but
    // doesn't conjure a popup out of nowhere.
    let mut state = AppState::new();
    let (tx, _rx) = channel();
    assert!(state.ui.active_suggestions.is_none());

    handle(
        &mut state,
        TuiOnlyEvent::AvailableCommandsRefreshed {
            commands: vec![slash("cmd")],
        },
        &tx,
    );

    assert_eq!(state.session.available_commands.len(), 1);
    assert!(state.ui.active_suggestions.is_none());
}

#[test]
fn memory_file_opened_is_toast_and_transcript_visible() {
    let mut state = AppState::new();
    let (tx, mut rx) = channel();
    let consumed = handle(
        &mut state,
        TuiOnlyEvent::MemoryFileOpened {
            path: "/tmp/CLAUDE.md".to_string(),
        },
        &tx,
    );

    assert!(consumed);
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Info);
    assert!(dispatched_system_push_contains(&mut rx, "/tmp/CLAUDE.md"));
}

#[test]
fn memory_file_open_failed_is_toast_and_transcript_visible() {
    let mut state = AppState::new();
    let (tx, mut rx) = channel();
    let consumed = handle(
        &mut state,
        TuiOnlyEvent::MemoryFileOpenFailed {
            path: "/tmp/CLAUDE.md".to_string(),
            error: "permission denied".to_string(),
        },
        &tx,
    );

    assert!(consumed);
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Warning);
    assert!(dispatched_system_push_contains(
        &mut rx,
        "permission denied"
    ));
}

#[test]
fn plan_file_opened_is_toast_and_transcript_visible() {
    let mut state = AppState::new();
    let (tx, mut rx) = channel();
    let consumed = handle(
        &mut state,
        TuiOnlyEvent::PlanFileOpened {
            path: "/tmp/plan.md".to_string(),
        },
        &tx,
    );

    assert!(consumed);
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Info);
    assert!(dispatched_system_push_contains(&mut rx, "/tmp/plan.md"));
}

#[test]
fn plan_file_open_failed_is_toast_and_transcript_visible() {
    let mut state = AppState::new();
    let (tx, mut rx) = channel();
    let consumed = handle(
        &mut state,
        TuiOnlyEvent::PlanFileOpenFailed {
            path: "/tmp/plan.md".to_string(),
            error: "editor missing".to_string(),
        },
        &tx,
    );

    assert!(consumed);
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Warning);
    assert!(dispatched_system_push_contains(&mut rx, "editor missing"));
}

#[test]
fn prompt_editor_completed_replaces_input_and_moves_cursor_to_end() {
    let mut state = AppState::new();
    let (tx, _rx) = channel();
    state.ui.input.set_text("old");
    state.ui.input.textarea.set_cursor(0);

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::PromptEditorCompleted {
            content: "edited prompt".to_string(),
            modified: true,
        },
        &tx,
    );

    assert!(consumed);
    assert_eq!(state.ui.input.text(), "edited prompt");
    assert_eq!(state.ui.input.textarea.cursor(), "edited prompt".len());
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Info);
}

#[test]
fn prompt_editor_failed_surfaces_warning_toast() {
    let mut state = AppState::new();
    let (tx, _rx) = channel();

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::PromptEditorFailed {
            error: "not found".to_string(),
        },
        &tx,
    );

    assert!(consumed);
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Warning);
    assert!(state.ui.toasts[0].message.contains("not found"));
}
