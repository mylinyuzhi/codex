//! Tests for the [`super::handle`] dispatch on TUI-only events that
//! mutate session state directly. Covers wiring for `AvailableCommandsRefreshed`
//! — the hot-reload path used by `/reload-plugins` to push a fresh
//! command catalogue into `state.session.available_commands`.

use pretty_assertions::assert_eq;

use coco_types::SlashCommandInfo;
use coco_types::TuiOnlyEvent;

use super::handle;
use crate::command::SystemPushKind;
use crate::command::UserCommand;
use crate::state::AppState;
use crate::state::SuggestionKind;
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
