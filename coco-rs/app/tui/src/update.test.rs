//! End-to-end tests for the update dispatcher. Focused on cross-module
//! invariants that the per-submodule tests can't catch — in particular typed
//! slash-command routing from both `submit` and `QueueInput`, and the
//! clipboard-cache lifecycle around `ClearScreen`.

use pretty_assertions::assert_eq;
use tokio::sync::mpsc;

use super::handle_command;
use crate::command::ShutdownReason;
use crate::command::UserCommand;
use crate::display_settings::DisplaySettingEditability;
use crate::display_settings::DisplaySettings;
use crate::display_settings::SyntaxHighlighting;
use crate::events::TuiCommand;
use crate::state::AppState;
use crate::state::MemoryDialogEntry;
use crate::state::MemoryDialogRowKind;
use crate::state::MemoryDialogScope;
use crate::state::MemoryDialogState;
use crate::state::MessageContent;
use crate::state::ModalState;
use crate::state::PanePromptState;
use crate::state::SlashCommandName;
use crate::state::ui::ToastSeverity;

fn drained_channel() -> (mpsc::Sender<UserCommand>, mpsc::Receiver<UserCommand>) {
    mpsc::channel(8)
}

#[tokio::test]
async fn clear_screen_nulls_last_agent_markdown() {
    // Regression: without this, Ctrl+L (ClearScreen) would wipe the visible
    // transcript but leave the copy cache pointing at the now-invisible
    // response, so a subsequent Ctrl+O would surface content the user
    // just cleared.
    let mut state = AppState::new();
    state.session.last_agent_markdown = Some("yesterday's reply".to_string());
    state
        .session
        .messages
        .push(crate::state::session::ChatMessage::assistant_text(
            "t0",
            "yesterday's reply",
        ));
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::ClearScreen, &tx).await;

    assert!(
        state.session.messages.is_empty(),
        "ClearScreen should drop messages"
    );
    assert_eq!(
        state.session.last_agent_markdown, None,
        "ClearScreen must null the copy cache"
    );
}

#[test]
fn parse_slash_input_validates_command_names() {
    let (name, args) =
        super::edit::parse_slash_input("/ask hello there").expect("valid slash command");
    assert_eq!(name, "ask");
    assert_eq!(args, "hello there");

    assert_eq!(super::edit::parse_slash_input("plain text"), None);
    assert_eq!(super::edit::parse_slash_input("/"), None);
    assert_eq!(super::edit::parse_slash_input("//bad"), None);
    assert_eq!(
        SlashCommandName::new("bad name"),
        Err(crate::state::InvalidSlashCommandName)
    );
}

#[tokio::test]
async fn queue_input_of_slash_dispatches_typed_command_not_agent_queue() {
    // Regression: slash input while the agent is streaming must not leak
    // into the agent queue as plain text. The command layer owns all
    // slash-command behavior, so the TUI only emits a typed name + args.
    let mut state = AppState::new();
    state.ui.input.textarea.set_text("/copy now");
    state
        .ui
        .input
        .textarea
        .set_cursor(state.ui.input.text().len());

    let (tx, mut rx) = drained_channel();
    handle_command(&mut state, TuiCommand::QueueInput, &tx).await;

    assert!(
        state.session.queued_commands.is_empty(),
        "slash commands must not enter the agent queue"
    );
    match rx.try_recv() {
        Ok(UserCommand::ExecuteSlashCommand { name, args }) => {
            assert_eq!(name, "copy");
            assert_eq!(args, "now");
        }
        other => panic!("expected ExecuteSlashCommand on the wire, got {other:?}"),
    }
    assert!(state.ui.input.is_empty(), "input should have been consumed");
}

#[tokio::test]
async fn submit_slash_dispatches_typed_command_without_chat_echo() {
    let mut state = AppState::new();
    state.ui.input.textarea.set_text("/rewind last");
    state
        .ui
        .input
        .textarea
        .set_cursor(state.ui.input.text().len());

    let (tx, mut rx) = drained_channel();
    handle_command(&mut state, TuiCommand::SubmitInput, &tx).await;

    assert!(
        state.session.messages.is_empty(),
        "slash invocations are commands, not chat transcript entries"
    );
    match rx.try_recv() {
        Ok(UserCommand::ExecuteSlashCommand { name, args }) => {
            assert_eq!(name, "rewind");
            assert_eq!(args, "last");
        }
        other => panic!("expected ExecuteSlashCommand on the wire, got {other:?}"),
    }
}

#[tokio::test]
async fn queue_input_of_plain_text_still_queues() {
    let mut state = AppState::new();
    state.ui.input.textarea.set_text("write a haiku");
    state
        .ui
        .input
        .textarea
        .set_cursor(state.ui.input.text().len());

    let (tx, mut rx) = drained_channel();
    handle_command(&mut state, TuiCommand::QueueInput, &tx).await;

    // The TUI display is repopulated from the engine via the
    // `CommandQueued` notification round-trip (handled in
    // `server_notification_handler::protocol`), so the local store
    // stays empty until that event arrives. Asserting the channel
    // payload pins the wire-side contract.
    assert!(
        state.session.queued_commands.is_empty(),
        "no optimistic local push — display reconciles from the engine"
    );
    match rx.try_recv() {
        Ok(UserCommand::QueueCommand { prompt, images }) => {
            assert_eq!(prompt, "write a haiku");
            assert!(images.is_empty());
        }
        other => panic!("expected QueueCommand on the wire, got {other:?}"),
    }
}

#[test]
fn toggle_syntax_highlighting_does_not_mutate_when_higher_priority_setting_wins() {
    let mut state = AppState::new();
    state.ui.display_settings = DisplaySettings {
        syntax_highlighting: SyntaxHighlighting::Enabled,
        syntax_highlighting_editability: DisplaySettingEditability::OverriddenBy(
            coco_config::SettingSource::Project,
        ),
        show_thinking: false,
    };

    super::interaction::toggle_syntax_highlighting(&mut state);

    assert_eq!(
        state.ui.display_settings.syntax_highlighting,
        SyntaxHighlighting::Enabled
    );
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Warning);
    assert!(
        state.ui.toasts[0].message.contains("project"),
        "unexpected toast: {}",
        state.ui.toasts[0].message
    );
}

#[tokio::test]
async fn idle_ctrl_c_arms_exit_hint_without_interrupting() {
    let mut state = AppState::new();
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Interrupt, &tx).await;

    assert_eq!(
        state.ui.pending_exit_hint(),
        Some(crate::state::ExitKey::CtrlC)
    );
    assert!(
        !state.session.was_interrupted,
        "idle Ctrl+C should not show the interrupt banner"
    );
    assert!(
        rx.try_recv().is_err(),
        "idle Ctrl+C should not send UserCommand::Interrupt"
    );
}

#[tokio::test]
async fn busy_ctrl_c_interrupts_without_exit_hint() {
    let mut state = AppState::new();
    state.session.set_busy(true);
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Interrupt, &tx).await;

    assert_eq!(state.ui.pending_exit_hint(), None);
    assert!(state.session.was_interrupted);
    match rx.try_recv() {
        Ok(UserCommand::Interrupt) => {}
        other => panic!("expected Interrupt on the wire, got {other:?}"),
    }
}

#[tokio::test]
async fn escape_in_teammates_view_interrupts_focused_teammate_current_work() {
    let mut state = AppState::new();
    state.session.expanded_view = coco_types::ExpandedView::Teammates;
    state.session.focused_subagent_index = Some(0);
    state
        .session
        .subagents
        .push(crate::state::session::SubagentInstance {
            agent_id: "worker@team".into(),
            agent_type: "general".into(),
            description: "scan".into(),
            status: crate::state::session::SubagentStatus::Running,
            color: None,
            started_at_ms: None,
            token_usage: None,
        });
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Cancel, &tx).await;

    match rx.try_recv() {
        Ok(UserCommand::InterruptAgentCurrentWork { agent_id }) => {
            assert_eq!(agent_id, "worker@team");
        }
        other => panic!("expected InterruptAgentCurrentWork, got {other:?}"),
    }
}

#[tokio::test]
async fn double_ctrl_c_shutdown_carries_reason() {
    let mut state = AppState::new();
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Interrupt, &tx).await;
    handle_command(&mut state, TuiCommand::Interrupt, &tx).await;

    match rx.try_recv() {
        Ok(UserCommand::Shutdown { reason }) => {
            assert_eq!(reason, ShutdownReason::DoublePressCtrlC);
        }
        other => panic!("expected Shutdown(DoublePressCtrlC), got {other:?}"),
    }
    assert!(state.should_exit());
}

#[tokio::test]
async fn clear_screen_preserves_active_surface() {
    // Defensive: ClearScreen should be safe to invoke with a surface open;
    // the surface is user-owned and unrelated to chat content.
    let mut state = AppState::new();
    state.ui.show_modal(crate::state::ModalState::Help);
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::ClearScreen, &tx).await;

    assert!(state.session.messages.is_empty());
    // Surface is intentionally preserved — ClearScreen scopes to transcript.
    assert!(state.ui.has_active_surface());
}

// ── Plan mode state behavior ──

#[tokio::test]
async fn plan_exit_deny_renders_rejection_and_keeps_plan_mode() {
    use crate::state::PlanExitPromptState;
    use coco_types::PermissionMode;

    let mut state = AppState::new();
    state.session.permission_mode = PermissionMode::Plan;
    state
        .ui
        .push_prompt(PanePromptState::PlanExit(PlanExitPromptState {
            plan_content: Some("# Plan\n- do stuff".into()),
            ..Default::default()
        }));
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Deny, &tx).await;

    // Mode stays Plan (user chose to keep planning).
    assert_eq!(state.session.permission_mode, PermissionMode::Plan);
    // Surface dismissed.
    assert!(!state.ui.has_active_surface());
    // A "User rejected Claude's plan" system message was injected.
    let last = state
        .session
        .messages
        .last()
        .expect("rejection message must be pushed");
    let text = last.text_content();
    assert!(text.contains("rejected"), "got: {text}");
    assert!(
        text.contains("do stuff"),
        "should echo plan content: {text}"
    );
}

#[tokio::test]
async fn plan_exit_approve_accept_edits_switches_mode() {
    use crate::state::PlanExitPromptState;
    use crate::state::PlanExitTarget;
    use coco_types::PermissionMode;

    let mut state = AppState::new();
    state.session.permission_mode = PermissionMode::Plan;
    state
        .ui
        .push_prompt(PanePromptState::PlanExit(PlanExitPromptState {
            plan_content: Some("plan".into()),
            next_mode: PlanExitTarget::AcceptEdits,
        }));
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Approve, &tx).await;

    assert_eq!(state.session.permission_mode, PermissionMode::AcceptEdits);
    assert!(!state.ui.has_active_surface());
    // The runner is notified via SetPermissionMode so the engine's
    // config is updated for the next turn.
    let cmd = rx.try_recv().expect("SetPermissionMode must be sent");
    assert!(
        matches!(
            cmd,
            UserCommand::SetPermissionMode {
                mode: PermissionMode::AcceptEdits
            }
        ),
        "got: {cmd:?}"
    );
}

#[tokio::test]
async fn plan_exit_tab_cycles_through_targets_with_bypass_gate() {
    use crate::state::PlanExitPromptState;
    use crate::state::PlanExitTarget;

    // Capability-gate ON → cycle includes BypassPermissions.
    let mut state = AppState::new();
    state.session.bypass_permissions_available = true;
    state
        .ui
        .push_prompt(PanePromptState::PlanExit(PlanExitPromptState {
            plan_content: Some("plan".into()),
            next_mode: PlanExitTarget::RestorePrePlan,
        }));
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::SurfaceNext, &tx).await;
    let Some(PanePromptState::PlanExit(p)) = state.ui.interaction.active_prompt.as_ref() else {
        panic!("state should still be PlanExit")
    };
    assert_eq!(p.next_mode, PlanExitTarget::AcceptEdits);

    handle_command(&mut state, TuiCommand::SurfaceNext, &tx).await;
    let Some(PanePromptState::PlanExit(p)) = state.ui.interaction.active_prompt.as_ref() else {
        panic!()
    };
    assert_eq!(p.next_mode, PlanExitTarget::BypassPermissions);

    handle_command(&mut state, TuiCommand::SurfaceNext, &tx).await;
    let Some(PanePromptState::PlanExit(p)) = state.ui.interaction.active_prompt.as_ref() else {
        panic!()
    };
    assert_eq!(p.next_mode, PlanExitTarget::RestorePrePlan);
}

#[tokio::test]
async fn plan_exit_tab_excludes_bypass_when_gate_off() {
    use crate::state::PlanExitPromptState;
    use crate::state::PlanExitTarget;

    // Capability-gate OFF → cycle skips BypassPermissions entirely.
    let mut state = AppState::new();
    state.session.bypass_permissions_available = false;
    state
        .ui
        .push_prompt(PanePromptState::PlanExit(PlanExitPromptState {
            plan_content: Some("plan".into()),
            next_mode: PlanExitTarget::RestorePrePlan,
        }));
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::SurfaceNext, &tx).await;
    let Some(PanePromptState::PlanExit(p)) = state.ui.interaction.active_prompt.as_ref() else {
        panic!()
    };
    assert_eq!(p.next_mode, PlanExitTarget::AcceptEdits);

    // Wraps back to Restore — Bypass is not offered.
    handle_command(&mut state, TuiCommand::SurfaceNext, &tx).await;
    let Some(PanePromptState::PlanExit(p)) = state.ui.interaction.active_prompt.as_ref() else {
        panic!()
    };
    assert_eq!(p.next_mode, PlanExitTarget::RestorePrePlan);
}

#[tokio::test]
async fn cycle_into_bypass_shows_confirmation_modal() {
    use coco_types::PermissionMode;

    let mut state = AppState::new();
    state.session.bypass_permissions_available = true;
    state.session.permission_mode = PermissionMode::Plan; // next = Bypass
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::CyclePermissionMode, &tx).await;

    // Mode must NOT change until the user confirms.
    assert_eq!(state.session.permission_mode, PermissionMode::Plan);
    assert!(
        matches!(
            state.ui.modal.as_ref(),
            Some(ModalState::BypassPermissions(_))
        ),
        "BypassPermissionsState should be shown"
    );
    assert!(rx.try_recv().is_err(), "should not flip mode until approve");
}

#[tokio::test]
async fn approve_bypass_modal_flips_mode_and_toasts() {
    use crate::state::BypassPermissionsState;
    use coco_types::PermissionMode;

    let mut state = AppState::new();
    state.session.bypass_permissions_available = true;
    state
        .ui
        .show_modal(ModalState::BypassPermissions(BypassPermissionsState {
            current_mode: "Plan".into(),
        }));
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Approve, &tx).await;

    assert_eq!(
        state.session.permission_mode,
        PermissionMode::BypassPermissions
    );
    assert!(!state.ui.has_active_surface());
    let toasted = state
        .ui
        .toasts
        .iter()
        .any(|t| matches!(t.severity, ToastSeverity::Warning));
    assert!(toasted, "approve should raise a warning toast");
    let cmd = rx.try_recv().expect("SetPermissionMode must be sent");
    assert!(
        matches!(
            cmd,
            UserCommand::SetPermissionMode {
                mode: PermissionMode::BypassPermissions
            }
        ),
        "got: {cmd:?}"
    );
}

#[tokio::test]
async fn deny_bypass_modal_keeps_mode() {
    use crate::state::BypassPermissionsState;
    use coco_types::PermissionMode;

    let mut state = AppState::new();
    state.session.bypass_permissions_available = true;
    state.session.permission_mode = PermissionMode::Plan;
    state
        .ui
        .show_modal(ModalState::BypassPermissions(BypassPermissionsState {
            current_mode: "Plan".into(),
        }));
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Deny, &tx).await;

    assert_eq!(state.session.permission_mode, PermissionMode::Plan);
    assert!(!state.ui.has_active_surface());
    assert!(
        rx.try_recv().is_err(),
        "deny must not emit SetPermissionMode"
    );
}

#[tokio::test]
async fn cycle_into_auto_shows_opt_in() {
    use coco_types::PermissionMode;

    // Only auto available — cycle Plan → Auto since bypass is gated off.
    let mut state = AppState::new();
    state.session.auto_mode_available = true;
    state.session.bypass_permissions_available = false;
    state.session.permission_mode = PermissionMode::Plan;
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::CyclePermissionMode, &tx).await;

    assert_eq!(state.session.permission_mode, PermissionMode::Plan);
    assert!(
        matches!(state.ui.modal.as_ref(), Some(ModalState::AutoModeOptIn(_))),
        "AutoModeOptIn state should be shown"
    );
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn cycle_into_safe_mode_applies_immediately() {
    use coco_types::PermissionMode;

    let mut state = AppState::new();
    state.session.permission_mode = PermissionMode::Default;
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::CyclePermissionMode, &tx).await;

    // Default → AcceptEdits with no confirmation state.
    assert_eq!(state.session.permission_mode, PermissionMode::AcceptEdits);
    assert!(!state.ui.has_active_surface());
    let toasted = state
        .ui
        .toasts
        .iter()
        .any(|t| matches!(t.severity, ToastSeverity::Info));
    assert!(toasted, "safe mode change should raise an info toast");
    let cmd = rx.try_recv().expect("SetPermissionMode must be sent");
    assert!(matches!(
        cmd,
        UserCommand::SetPermissionMode {
            mode: PermissionMode::AcceptEdits
        }
    ));
}

#[tokio::test]
async fn toggle_plan_mode_raises_toast() {
    use coco_types::PermissionMode;

    let mut state = AppState::new();
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::TogglePlanMode, &tx).await;
    assert_eq!(state.session.permission_mode, PermissionMode::Plan);
    let on_toast = state
        .ui
        .toasts
        .iter()
        .any(|t| t.message.to_lowercase().contains("plan mode on"));
    assert!(on_toast, "plan-on toast should mention plan mode on");

    handle_command(&mut state, TuiCommand::TogglePlanMode, &tx).await;
    assert_eq!(state.session.permission_mode, PermissionMode::Default);
}

// ─────────────────── TS-behavior tests: Ctrl+C / ESC / Ctrl+E ──────────────────

#[tokio::test]
async fn ctrl_c_with_text_clears_input_and_saves_to_history() {
    // Mirrors TS `useTextInput.ts:108-120` third callback: Ctrl+C with
    // text present clears the input AND records it into history so the
    // user can recover it with Up. Per `update/exit.rs::on_interrupt`,
    // the exit hint is still pre-armed so a second Ctrl+C exits.
    let mut state = AppState::new();
    state.ui.input.textarea.set_text("draft text");
    state.ui.input.textarea.set_cursor(10);
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Interrupt, &tx).await;

    assert!(state.ui.input.is_empty(), "input should have been cleared");
    assert!(
        state
            .ui
            .input
            .history
            .iter()
            .any(|h| h.text == "draft text"),
        "draft must be in history",
    );
    // Tracker armed so the next Ctrl+C goes through the Quit path.
    assert!(state.ui.ctrl_c_tracker.pending().is_some());
}

#[tokio::test]
async fn ctrl_c_idle_empty_arms_exit_then_quits() {
    // Mirrors TS `useExitOnCtrlCD`: with empty input the first Ctrl+C
    // only arms a hint; a second within the window exits.
    let mut state = AppState::new();
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Interrupt, &tx).await;
    assert!(state.ui.ctrl_c_tracker.pending().is_some());
    assert!(!state.session.was_interrupted);
    // Second press within the window should request shutdown.
    handle_command(&mut state, TuiCommand::Interrupt, &tx).await;
    // Quit drives `state.quit()`; we can't see process exit from a unit
    // test, but `running` flips to Done and no interrupt was sent.
    assert!(state.should_exit());
}

#[tokio::test]
async fn esc_with_text_first_press_shows_toast() {
    // TS `useTextInput.ts:126-153` first callback: when input is
    // non-empty, single Esc shows a toast and arms the double-press.
    let mut state = AppState::new();
    state.ui.input.textarea.set_text("draft");
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Cancel, &tx).await;

    assert_eq!(
        state.ui.input.text(),
        "draft",
        "text must NOT clear on single Esc"
    );
    assert!(
        state.ui.toasts.iter().any(|t| t.message.contains("again")),
        "single Esc should toast 'Esc again to clear'",
    );
}

#[tokio::test]
async fn esc_double_press_clears_input_and_records_history() {
    // Double-press Esc within the window clears input + records history.
    let mut state = AppState::new();
    state.ui.input.textarea.set_text("draft");
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Cancel, &tx).await;
    handle_command(&mut state, TuiCommand::Cancel, &tx).await;

    assert!(state.ui.input.is_empty(), "double-Esc clears input");
    assert!(state.ui.input.history.iter().any(|h| h.text == "draft"));
}

#[tokio::test]
async fn esc_on_memory_dialog_records_transcript_result() {
    let mut state = AppState::new();
    state
        .ui
        .show_modal(ModalState::MemoryDialog(MemoryDialogState {
            entries: vec![MemoryDialogEntry {
                path: std::path::PathBuf::from("/tmp/coco-memory-test/CLAUDE.md"),
                label: "Project memory".to_string(),
                scope: MemoryDialogScope::Project,
                row_kind: MemoryDialogRowKind::File {
                    exists: false,
                    read_only: false,
                },
            }],
            selected: 0,
        }));
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Cancel, &tx).await;

    assert!(!state.ui.has_active_surface(), "memory dialog dismissed");
    assert!(state.ui.toasts.iter().any(|t| {
        t.severity == ToastSeverity::Info && t.message.contains("Cancelled memory editing")
    }));
    assert!(state.session.messages.iter().any(|m| {
        matches!(
            &m.content,
            MessageContent::SystemText(text) if text.contains("Cancelled memory editing")
        )
    }));
}

#[tokio::test]
async fn ctrl_e_moves_cursor_to_end_not_external_editor() {
    // Regression: bare Ctrl+E previously triggered OpenExternalEditor in
    // the legacy global cascade, shadowing readline's end-of-line. The
    // user now expects Ctrl+E → CursorEnd via `map_input_key`.
    let mut state = AppState::new();
    state.ui.input.textarea.set_text("hello");
    state.ui.input.textarea.set_cursor(0);
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::CursorEnd, &tx).await;

    assert_eq!(state.ui.input.textarea.cursor(), 5);
}

#[tokio::test]
async fn open_external_editor_sends_prompt_editor_command() {
    let mut state = AppState::new();
    state.ui.input.set_text("draft prompt");
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::OpenExternalEditor, &tx).await;

    let UserCommand::OpenPromptEditor { initial_content } =
        rx.try_recv().expect("prompt editor command sent")
    else {
        panic!("expected OpenPromptEditor")
    };
    assert_eq!(initial_content, "draft prompt");
}

#[tokio::test]
async fn open_plan_editor_sends_plan_editor_command() {
    let mut state = AppState::new();
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::OpenPlanEditor, &tx).await;

    let UserCommand::OpenPlanEditor = rx.try_recv().expect("plan editor command sent") else {
        panic!("expected OpenPlanEditor")
    };
    assert!(state.ui.toasts.is_empty());
}

#[tokio::test]
async fn ctrl_a_moves_cursor_to_start_visually_correct_for_cjk() {
    // After typing CJK input, Ctrl+A must move cursor to byte 0. The
    // render-layer test (snapshot) covers the column-0 visual; here we
    // just confirm the state-level position.
    let mut state = AppState::new();
    state.ui.input.textarea.set_text("你好世界");
    state.ui.input.textarea.set_cursor(12); // end (4 chars × 3 bytes)
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::CursorHome, &tx).await;

    assert_eq!(state.ui.input.textarea.cursor(), 0);
}
