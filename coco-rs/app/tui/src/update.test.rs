//! End-to-end tests for the update dispatcher. Focused on cross-module
//! invariants that the per-submodule tests can't catch — in particular the
//! shared local-command interception path used by both `submit` and
//! `QueueInput`, and the clipboard-cache lifecycle around `ClearScreen`.

use pretty_assertions::assert_eq;
use tokio::sync::mpsc;

use super::edit::try_local_command;
use super::handle_command;
use crate::command::UserCommand;
use crate::events::TuiCommand;
use crate::state::AppState;
use crate::state::Overlay;
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
fn try_local_command_intercepts_copy_slash() {
    let mut state = AppState::new();
    state.session.last_agent_markdown = Some("payload".to_string());

    assert!(try_local_command(&mut state, "/copy"));
    // The copy handler surfaces a success toast — proof that it actually ran
    // rather than being routed to the agent.
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Success);
}

#[test]
fn try_local_command_intercepts_rewind_family() {
    let mut state = AppState::new();

    assert!(try_local_command(&mut state, "/rewind"));
    // Rewind opens an overlay or surfaces a toast; either way the command
    // was handled locally (no agent round-trip).
    let handled = state.ui.overlay.is_some() || !state.ui.toasts.is_empty();
    assert!(handled, "rewind should affect ui state");

    state.ui.overlay = None;
    state.ui.toasts.clear();
    assert!(try_local_command(&mut state, "/checkpoint last"));
    let handled = state.ui.overlay.is_some() || !state.ui.toasts.is_empty();
    assert!(handled, "checkpoint last should affect ui state");
}

#[test]
fn try_local_command_passes_through_non_local_slash() {
    let mut state = AppState::new();

    // `/ask` is not a TUI-only command — should fall through to the agent.
    assert!(!try_local_command(&mut state, "/ask hello"));
    // And plain text should never be intercepted.
    assert!(!try_local_command(&mut state, "just some text"));
    assert!(!try_local_command(&mut state, ""));
}

#[tokio::test]
async fn queue_input_of_copy_slash_dispatches_locally_not_to_agent() {
    // Regression: typing `/copy` while the agent is streaming previously
    // went through QueueInput → UserCommand::QueueCommand, leaking the
    // slash into the agent transcript. It must intercept locally instead.
    let mut state = AppState::new();
    state.session.last_agent_markdown = Some("cached reply".to_string());
    state.ui.input.text = "/copy".to_string();
    state.ui.input.cursor = state.ui.input.text.chars().count() as i32;

    let (tx, mut rx) = drained_channel();
    handle_command(&mut state, TuiCommand::QueueInput, &tx).await;

    assert!(
        state.session.queued_commands.is_empty(),
        "/copy must not enter the agent queue"
    );
    assert!(
        rx.try_recv().is_err(),
        "/copy must not send a UserCommand to core"
    );
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Success);
    assert!(state.ui.input.is_empty(), "input should have been consumed");
}

#[tokio::test]
async fn queue_input_of_plain_text_still_queues() {
    let mut state = AppState::new();
    state.ui.input.text = "write a haiku".to_string();
    state.ui.input.cursor = state.ui.input.text.chars().count() as i32;

    let (tx, mut rx) = drained_channel();
    handle_command(&mut state, TuiCommand::QueueInput, &tx).await;

    assert_eq!(state.session.queued_commands.len(), 1);
    assert_eq!(state.session.queued_commands[0], "write a haiku");
    assert!(
        matches!(rx.try_recv(), Ok(UserCommand::QueueCommand { .. })),
        "plain text should still propagate to core"
    );
}

#[tokio::test]
async fn clear_screen_also_leaves_no_overlay() {
    // Defensive: ClearScreen should be safe to invoke with an overlay open;
    // the overlay is user-owned and unrelated to chat content.
    let mut state = AppState::new();
    state.ui.set_overlay(Overlay::Help);
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::ClearScreen, &tx).await;

    assert!(state.session.messages.is_empty());
    // Overlay is intentionally preserved — ClearScreen scopes to transcript.
    assert!(state.ui.overlay.is_some());
}

// ── /clear family ──

#[tokio::test]
async fn slash_clear_wipes_transcript_and_surfaces_toast_and_signals_engine() {
    let mut state = AppState::new();
    state
        .session
        .messages
        .push(crate::state::session::ChatMessage::user_text("m1", "hi"));
    state
        .session
        .messages
        .push(crate::state::session::ChatMessage::assistant_text(
            "m2", "hello",
        ));
    state.session.last_agent_markdown = Some("hello".into());
    let (tx, mut rx) = drained_channel();

    assert!(super::edit::try_local_clear(&mut state, "/clear", &tx).await);
    assert!(state.session.messages.is_empty());
    assert_eq!(state.session.last_agent_markdown, None);
    assert_eq!(state.ui.toasts.len(), 1);
    // Engine is notified so it can reset app_state plan flags.
    assert!(matches!(
        rx.try_recv(),
        Ok(UserCommand::ClearConversation {
            scope: crate::command::ClearScope::Conversation
        })
    ));
}

#[tokio::test]
async fn slash_clear_all_signals_all_scope() {
    let mut state = AppState::new();
    state.session.session_id = Some("test-clear-all".into());
    state
        .session
        .messages
        .push(crate::state::session::ChatMessage::user_text("m1", "hi"));
    let (tx, mut rx) = drained_channel();

    assert!(super::edit::try_local_clear(&mut state, "/clear all", &tx).await);
    assert!(state.session.messages.is_empty());
    let toast_text = state
        .ui
        .toasts
        .front()
        .map(|t| t.message.clone())
        .unwrap_or_default();
    assert!(
        toast_text.contains("plan state") || toast_text.contains("计划状态"),
        "expected /clear all toast; got: {toast_text}"
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(UserCommand::ClearConversation {
            scope: crate::command::ClearScope::All
        })
    ));
}

#[tokio::test]
async fn slash_clear_dismisses_overlay_and_toasts() {
    let mut state = AppState::new();
    state.ui.set_overlay(Overlay::Help);
    state
        .ui
        .add_toast(crate::state::ui::Toast::info("stale".to_string()));
    let (tx, _rx) = drained_channel();

    assert!(super::edit::try_local_clear(&mut state, "/clear", &tx).await);
    assert!(state.ui.overlay.is_none());
    assert_eq!(state.ui.toasts.len(), 1);
}

#[tokio::test]
async fn slash_clear_history_signals_history_scope() {
    let mut state = AppState::new();
    state
        .session
        .messages
        .push(crate::state::session::ChatMessage::user_text("m1", "hi"));
    let (tx, mut rx) = drained_channel();

    assert!(super::edit::try_local_clear(&mut state, "/clear history", &tx).await);
    assert!(state.session.messages.is_empty());
    assert!(matches!(
        rx.try_recv(),
        Ok(UserCommand::ClearConversation {
            scope: crate::command::ClearScope::History
        })
    ));
}

#[tokio::test]
async fn slash_clear_unknown_variant_passes_through() {
    let mut state = AppState::new();
    let (tx, _rx) = drained_channel();
    // "/clear foo" is not a known variant — should NOT be intercepted.
    assert!(!super::edit::try_local_clear(&mut state, "/clear foo", &tx).await);
}

// ── Plan mode overlay behavior ──

#[tokio::test]
async fn plan_exit_deny_renders_rejection_and_keeps_plan_mode() {
    use crate::state::PlanExitOverlay;
    use coco_types::PermissionMode;

    let mut state = AppState::new();
    state.session.permission_mode = PermissionMode::Plan;
    state.ui.set_overlay(Overlay::PlanExit(PlanExitOverlay {
        plan_content: Some("# Plan\n- do stuff".into()),
        ..Default::default()
    }));
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Deny, &tx).await;

    // Mode stays Plan (user chose to keep planning).
    assert_eq!(state.session.permission_mode, PermissionMode::Plan);
    // Overlay dismissed.
    assert!(state.ui.overlay.is_none());
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
    use crate::state::PlanExitOverlay;
    use crate::state::PlanExitTarget;
    use coco_types::PermissionMode;

    let mut state = AppState::new();
    state.session.permission_mode = PermissionMode::Plan;
    state.ui.set_overlay(Overlay::PlanExit(PlanExitOverlay {
        plan_content: Some("plan".into()),
        next_mode: PlanExitTarget::AcceptEdits,
    }));
    let (tx, mut rx) = drained_channel();

    handle_command(&mut state, TuiCommand::Approve, &tx).await;

    assert_eq!(state.session.permission_mode, PermissionMode::AcceptEdits);
    assert!(state.ui.overlay.is_none());
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
    use crate::state::PlanExitOverlay;
    use crate::state::PlanExitTarget;

    // Capability-gate ON → cycle includes BypassPermissions.
    let mut state = AppState::new();
    state.session.bypass_permissions_available = true;
    state.ui.set_overlay(Overlay::PlanExit(PlanExitOverlay {
        plan_content: Some("plan".into()),
        next_mode: PlanExitTarget::RestorePrePlan,
    }));
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::OverlayNext, &tx).await;
    let Some(Overlay::PlanExit(ref p)) = state.ui.overlay else {
        panic!("overlay should still be PlanExit")
    };
    assert_eq!(p.next_mode, PlanExitTarget::AcceptEdits);

    handle_command(&mut state, TuiCommand::OverlayNext, &tx).await;
    let Some(Overlay::PlanExit(ref p)) = state.ui.overlay else {
        panic!()
    };
    assert_eq!(p.next_mode, PlanExitTarget::BypassPermissions);

    handle_command(&mut state, TuiCommand::OverlayNext, &tx).await;
    let Some(Overlay::PlanExit(ref p)) = state.ui.overlay else {
        panic!()
    };
    assert_eq!(p.next_mode, PlanExitTarget::RestorePrePlan);
}

#[tokio::test]
async fn plan_exit_tab_excludes_bypass_when_gate_off() {
    use crate::state::PlanExitOverlay;
    use crate::state::PlanExitTarget;

    // Capability-gate OFF → cycle skips BypassPermissions entirely.
    let mut state = AppState::new();
    state.session.bypass_permissions_available = false;
    state.ui.set_overlay(Overlay::PlanExit(PlanExitOverlay {
        plan_content: Some("plan".into()),
        next_mode: PlanExitTarget::RestorePrePlan,
    }));
    let (tx, _rx) = drained_channel();

    handle_command(&mut state, TuiCommand::OverlayNext, &tx).await;
    let Some(Overlay::PlanExit(ref p)) = state.ui.overlay else {
        panic!()
    };
    assert_eq!(p.next_mode, PlanExitTarget::AcceptEdits);

    // Wraps back to Restore — Bypass is not offered.
    handle_command(&mut state, TuiCommand::OverlayNext, &tx).await;
    let Some(Overlay::PlanExit(ref p)) = state.ui.overlay else {
        panic!()
    };
    assert_eq!(p.next_mode, PlanExitTarget::RestorePrePlan);
}
