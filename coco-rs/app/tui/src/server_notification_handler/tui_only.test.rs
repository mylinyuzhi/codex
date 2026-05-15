//! Tests for the [`super::handle`] dispatch on TUI-only events that
//! mutate session state directly. Covers wiring for `AvailableCommandsRefreshed`
//! — the hot-reload path used by `/reload-plugins` to push a fresh
//! command catalogue into `state.session.available_commands`.

use pretty_assertions::assert_eq;

use coco_types::SlashCommandInfo;
use coco_types::TuiOnlyEvent;

use super::handle;
use crate::state::AppState;
use crate::state::SuggestionKind;

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
    state.session.available_commands = vec![slash("old-cmd")];

    let consumed = handle(
        &mut state,
        TuiOnlyEvent::AvailableCommandsRefreshed {
            commands: vec![slash("new-cmd-a"), slash("new-cmd-b")],
        },
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
    assert!(state.ui.active_suggestions.is_none());

    handle(
        &mut state,
        TuiOnlyEvent::AvailableCommandsRefreshed {
            commands: vec![slash("cmd")],
        },
    );

    assert_eq!(state.session.available_commands.len(), 1);
    assert!(state.ui.active_suggestions.is_none());
}
