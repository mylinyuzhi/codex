use super::dispatch_action;
use crate::events::TuiCommand;
use crate::state::AppState;
use coco_keybindings::KeybindingAction;

fn fresh_state() -> AppState {
    AppState::default()
}

#[test]
fn app_interrupt_maps_to_interrupt() {
    let state = fresh_state();
    let cmd = dispatch_action(&KeybindingAction::AppInterrupt, &state).unwrap();
    assert!(matches!(cmd, TuiCommand::Interrupt));
}

#[test]
fn chat_submit_queues_when_streaming() {
    let mut state = fresh_state();
    state.ui.streaming = Some(crate::state::StreamingState::default());
    let cmd = dispatch_action(&KeybindingAction::ChatSubmit, &state).unwrap();
    assert!(matches!(cmd, TuiCommand::QueueInput));
}

#[test]
fn chat_submit_submits_when_idle() {
    let state = fresh_state();
    let cmd = dispatch_action(&KeybindingAction::ChatSubmit, &state).unwrap();
    assert!(matches!(cmd, TuiCommand::SubmitInput));
}

#[test]
fn confirm_yes_maps_to_approve() {
    let state = fresh_state();
    let cmd = dispatch_action(&KeybindingAction::ConfirmYes, &state).unwrap();
    assert!(matches!(cmd, TuiCommand::Approve));
}

#[test]
fn select_accept_maps_to_overlay_confirm() {
    let state = fresh_state();
    let cmd = dispatch_action(&KeybindingAction::SelectAccept, &state).unwrap();
    assert!(matches!(cmd, TuiCommand::OverlayConfirm));
}

#[test]
fn command_action_synthesizes_slash_command() {
    let state = fresh_state();
    // `command:help` user-binding → submit `/help` through the same
    // slash-command runner the agent driver uses.
    let cmd = dispatch_action(&KeybindingAction::Command("help".into()), &state).unwrap();
    match cmd {
        TuiCommand::ExecuteSlashCommand(name) => assert_eq!(name, "help"),
        other => panic!("expected ExecuteSlashCommand(\"help\"), got {other:?}"),
    }
}

#[test]
fn chat_stash_maps_to_stash_input_draft() {
    let state = fresh_state();
    let cmd = dispatch_action(&KeybindingAction::ChatStash, &state).unwrap();
    assert!(matches!(cmd, TuiCommand::StashInputDraft));
}

#[test]
fn app_toggle_todos_maps_to_toggle_expanded_tasks_view() {
    let state = fresh_state();
    let cmd = dispatch_action(&KeybindingAction::AppToggleTodos, &state).unwrap();
    assert!(matches!(cmd, TuiCommand::ToggleExpandedTasksView));
}

#[test]
fn app_toggle_transcript_maps_to_toggle_transcript() {
    let state = fresh_state();
    let cmd = dispatch_action(&KeybindingAction::AppToggleTranscript, &state).unwrap();
    assert!(matches!(cmd, TuiCommand::ToggleTranscript));
}

#[test]
fn app_toggle_teammate_preview_maps_to_toggle_teammate_message_preview() {
    let state = fresh_state();
    let cmd = dispatch_action(&KeybindingAction::AppToggleTeammatePreview, &state).unwrap();
    assert!(matches!(cmd, TuiCommand::ToggleTeammateMessagePreview));
}

#[test]
fn feature_gated_actions_silently_no_op() {
    // TS-mirror: when a feature isn't ported, the action returns
    // None so the keystroke is swallowed silently (matches TS where
    // useKeybinding is never registered for unported features).
    let state = fresh_state();
    let gated = [
        KeybindingAction::ChatUndo,
        KeybindingAction::ChatMessageActions,
        KeybindingAction::AppToggleBrief,
        KeybindingAction::AppToggleTerminal,
        KeybindingAction::PermissionToggleDebug,
        KeybindingAction::AttachmentsRemove,
        KeybindingAction::PluginToggle,
        KeybindingAction::PluginInstall,
        KeybindingAction::VoicePushToTalk,
        KeybindingAction::SettingsSearch,
        KeybindingAction::SettingsRetry,
        KeybindingAction::MessageActionsPrev,
        KeybindingAction::MessageActionsNext,
        KeybindingAction::MessageActionsEnter,
    ];
    for action in gated {
        assert!(
            dispatch_action(&action, &state).is_none(),
            "{action:?} should return None — TS feature-gated action with no coco-rs surface",
        );
    }
}

#[test]
fn theme_toggle_syntax_highlighting_maps_to_toggle_command() {
    let state = fresh_state();
    let cmd = dispatch_action(&KeybindingAction::ThemeToggleSyntaxHighlighting, &state).unwrap();
    assert!(matches!(cmd, TuiCommand::ToggleSyntaxHighlighting));
}
