//! Tests for TUI AppState.

use coco_types::PermissionMode;

use crate::state::AppState;
use crate::state::ModalState;
use crate::state::PanePromptState;
use crate::state::PermissionDetail;
use crate::state::PermissionPromptState;
use crate::state::derive::test_helpers;
use crate::state::session::TokenUsage;
use crate::state::transcript_view::CellKind;
use crate::state::ui::Toast;

// ── Permission-mode cycling ──

/// Shift+Tab cycle must match TS `getNextPermissionMode()`.
/// With neither bypass nor auto available, the cycle is
/// `Default → AcceptEdits → Plan → Default`.
#[test]
fn test_cycle_permission_mode_baseline() {
    let mut state = AppState::new();
    assert_eq!(state.session.permission_mode, PermissionMode::Default);

    state.cycle_permission_mode();
    assert_eq!(state.session.permission_mode, PermissionMode::AcceptEdits);

    state.cycle_permission_mode();
    assert_eq!(state.session.permission_mode, PermissionMode::Plan);

    state.cycle_permission_mode();
    assert_eq!(state.session.permission_mode, PermissionMode::Default);
}

#[test]
fn test_cycle_permission_mode_with_bypass() {
    let mut state = AppState::new();
    state.session.bypass_permissions_available = true;
    state.session.permission_mode = PermissionMode::Plan;

    state.cycle_permission_mode();
    assert_eq!(
        state.session.permission_mode,
        PermissionMode::BypassPermissions
    );

    state.cycle_permission_mode();
    assert_eq!(state.session.permission_mode, PermissionMode::Default);
}

#[test]
fn test_cycle_permission_mode_with_auto() {
    let mut state = AppState::new();
    state.session.auto_mode_available = true;
    state.session.permission_mode = PermissionMode::Plan;

    state.cycle_permission_mode();
    assert_eq!(state.session.permission_mode, PermissionMode::Auto);

    state.cycle_permission_mode();
    assert_eq!(state.session.permission_mode, PermissionMode::Default);
}

#[test]
fn test_toggle_plan_mode_flips_between_plan_and_default() {
    let mut state = AppState::new();
    assert_eq!(state.session.permission_mode, PermissionMode::Default);

    state.toggle_plan_mode();
    assert_eq!(state.session.permission_mode, PermissionMode::Plan);
    assert!(state.is_plan_mode());

    state.toggle_plan_mode();
    assert_eq!(state.session.permission_mode, PermissionMode::Default);
    assert!(!state.is_plan_mode());
}

#[test]
fn test_toggle_plan_mode_from_accept_edits_goes_to_plan() {
    // Quick toggle is "enable plan mode" regardless of source; it
    // doesn't try to preserve the prior mode.
    let mut state = AppState::new();
    state.session.permission_mode = PermissionMode::AcceptEdits;

    state.toggle_plan_mode();
    assert_eq!(state.session.permission_mode, PermissionMode::Plan);

    state.toggle_plan_mode();
    assert_eq!(state.session.permission_mode, PermissionMode::Default);
}

// ── PlanExit target resolution ──

#[test]
fn test_plan_exit_target_resolves_to_permission_mode() {
    use crate::state::PlanExitTarget;
    assert_eq!(PlanExitTarget::RestorePrePlan.resolve(), None);
    assert_eq!(
        PlanExitTarget::AcceptEdits.resolve(),
        Some(PermissionMode::AcceptEdits)
    );
    assert_eq!(
        PlanExitTarget::BypassPermissions.resolve(),
        Some(PermissionMode::BypassPermissions)
    );
}

#[test]
fn test_plan_exit_target_default_is_restore_pre_plan() {
    use crate::state::PlanExitTarget;
    assert_eq!(PlanExitTarget::default(), PlanExitTarget::RestorePrePlan);
}

#[test]
fn test_new_state_defaults() {
    let state = AppState::new();
    assert!(!state.should_exit());
    assert!(!state.has_active_surface());
    assert!(!state.is_streaming());
    assert!(!state.should_show_spinner());
    assert!(!state.is_plan_mode());
    assert!(!state.session.fast_mode);
    assert_eq!(state.session.turn_count, 0);
    assert!(!state.ui.ephemeral.turn_active());
    assert!(state.session.transcript.is_empty());
}

#[test]
fn test_apply_display_settings_updates_show_thinking_default() {
    let mut state = AppState::new();
    assert!(!state.ui.show_thinking);

    state.ui.apply_display_settings(crate::DisplaySettings {
        show_thinking: true,
        ..crate::DisplaySettings::default()
    });

    assert!(state.ui.show_thinking);
}

#[test]
fn test_apply_display_settings_keeps_runtime_show_thinking_toggle_when_default_unchanged() {
    let mut state = AppState::new();
    state.ui.apply_display_settings(crate::DisplaySettings {
        show_thinking: false,
        ..crate::DisplaySettings::default()
    });

    state.ui.show_thinking = true;
    state.ui.apply_display_settings(crate::DisplaySettings {
        show_thinking: false,
        ..crate::DisplaySettings::default()
    });

    assert!(state.ui.show_thinking);
}

#[test]
fn test_modal_queue_same_priority_fifo() {
    let mut state = AppState::new();

    // Help (priority 8) installed
    state.ui.show_modal(ModalState::Help);
    assert!(state.has_active_surface());

    // A second Help queues behind (same priority keeps insertion order)
    state.ui.show_modal(ModalState::Help);
    assert!(matches!(state.ui.modal, Some(ModalState::Help)));
    assert_eq!(state.ui.modal_queue.len(), 1);

    // Dismiss promotes the queued one
    state.ui.dismiss_modal();
    assert!(matches!(state.ui.modal, Some(ModalState::Help)));
    assert_eq!(state.ui.modal_queue.len(), 0);

    state.ui.dismiss_modal();
    assert!(!state.has_active_surface());
}

#[test]
fn test_modal_higher_priority_displaces_current() {
    let mut state = AppState::new();

    // Help is the weakest priority (8)
    state.ui.show_modal(ModalState::Help);
    // Error (priority 4) arriving should displace Help back into the queue
    state.ui.show_modal(ModalState::Error("boom".to_string()));
    assert!(matches!(state.ui.modal, Some(ModalState::Error(_))));
    assert_eq!(state.ui.modal_queue.len(), 1);

    // Dismissing Error restores Help
    state.ui.dismiss_modal();
    assert!(matches!(state.ui.modal, Some(ModalState::Help)));
    assert_eq!(state.ui.modal_queue.len(), 0);
}

#[test]
fn test_surface_lower_priority_queues_behind() {
    let mut state = AppState::new();

    // Error (priority 4) is current
    state.ui.show_modal(ModalState::Error("boom".to_string()));
    // Help (priority 8) queues behind without displacing
    state.ui.show_modal(ModalState::Help);
    assert!(matches!(state.ui.modal, Some(ModalState::Error(_))));
    assert_eq!(state.ui.modal_queue.len(), 1);

    state.ui.dismiss_modal();
    assert!(matches!(state.ui.modal, Some(ModalState::Help)));
}

#[test]
fn test_prompt_queue_priority_ordered() {
    let mut state = AppState::new();
    // Install highest-priority prompt so queue fills with strictly lower ones.
    state.ui.push_prompt(PanePromptState::SandboxPermission(
        crate::state::SandboxPermissionPromptState {
            request_id: "r0".into(),
            description: "sandbox".into(),
        },
    ));

    // Enqueue in reverse priority order; insertion should re-sort.
    state.ui.push_prompt(PanePromptState::Question(
        crate::state::QuestionPromptState {
            request_id: "question-1".into(),
            original_input: serde_json::json!({}),
            questions: vec![],
            current_question: crate::state::QuestionPage::Question(0),
            focus_target: crate::state::QuestionFocusTarget::QuestionOption(0),
            is_in_plan_mode: false,
        },
    )); // priority 2
    state
        .ui
        .push_prompt(PanePromptState::PlanExit(Default::default())); // priority 1
    state.ui.push_prompt(PanePromptState::PlanEntry(
        crate::state::PlanEntryPromptState {
            description: "plan".into(),
        },
    )); // priority 1

    // Queue promotes by priority asc after the active sandbox prompt is dismissed.
    let mut priorities = Vec::new();
    for _ in 0..3 {
        state.ui.dismiss_prompt();
        let prompt = state
            .ui
            .interaction
            .active_prompt
            .as_ref()
            .expect("queued prompt should be promoted");
        priorities.push(prompt.priority());
    }
    assert_eq!(priorities, vec![1, 1, 2]);
}

#[test]
fn test_toast_lifecycle() {
    let mut state = AppState::new();

    state.ui.add_toast(Toast::info("hello"));
    assert!(state.ui.has_toasts());
    assert_eq!(state.ui.toasts.len(), 1);

    // Toasts should not be expired immediately
    state.ui.expire_toasts();
    assert!(state.ui.has_toasts());
}

#[test]
fn test_session_messages() {
    let mut state = AppState::new();

    test_helpers::push_user_text(&mut state.session, "1", "hello");
    test_helpers::push_assistant_text(&mut state.session, "hi there");

    let cells = state.session.transcript.cells();
    assert_eq!(cells.len(), 2);
    assert!(matches!(
        cells.last().map(|c| &c.kind),
        Some(CellKind::AssistantText { .. })
    ));
}

#[test]
fn test_tool_execution_lifecycle() {
    let mut state = AppState::new();

    state
        .session
        .start_tool("call-1".to_string(), "Bash".to_string());
    assert_eq!(state.session.tool_executions.len(), 1);
    assert_eq!(
        state.session.tool_executions[0].status,
        crate::state::session::ToolStatus::Queued
    );

    state.session.run_tool("call-1");
    assert_eq!(
        state.session.tool_executions[0].status,
        crate::state::session::ToolStatus::Running
    );

    state.session.complete_tool("call-1", /*is_error*/ false);
    assert_eq!(
        state.session.tool_executions[0].status,
        crate::state::session::ToolStatus::Completed
    );

    state.session.complete_tool("call-2", /*is_error*/ true);
    // Non-existent tool: no panic
}

#[test]
fn test_input_editing() {
    let mut state = AppState::new();

    state.ui.input.textarea.insert_str("h");
    state.ui.input.textarea.insert_str("i");
    assert_eq!(state.ui.input.text(), "hi");
    assert_eq!(state.ui.input.textarea.cursor(), 2);

    state.ui.input.textarea.move_cursor_left();
    assert_eq!(state.ui.input.textarea.cursor(), 1);

    state.ui.input.textarea.insert_str("!");
    assert_eq!(state.ui.input.text(), "h!i");

    let taken = state.ui.input.take_input();
    assert_eq!(taken, "h!i");
    assert!(state.ui.input.is_empty());
    assert_eq!(state.ui.input.textarea.cursor(), 0);
}

#[test]
fn test_input_history_frecency() {
    let mut state = AppState::new();

    state.ui.input.add_to_history("first".to_string());
    state.ui.input.add_to_history("second".to_string());
    assert_eq!(state.ui.input.history.len(), 2);

    // Re-adding an entry bumps its frequency instead of creating a duplicate.
    state.ui.input.add_to_history("first".to_string());
    assert_eq!(state.ui.input.history.len(), 2);

    // "first" now has frequency 2; sort puts it at the top of the list.
    assert_eq!(state.ui.input.history[0].text, "first");
    assert_eq!(state.ui.input.history[0].frequency, 2);
    assert_eq!(state.ui.input.history[1].text, "second");
    assert_eq!(state.ui.input.history[1].frequency, 1);
}

#[test]
fn test_permission_prompt() {
    let mut state = AppState::new();

    state
        .ui
        .push_prompt(PanePromptState::Permission(PermissionPromptState {
            request_id: "req-1".to_string(),
            tool_name: "Bash".to_string(),
            description: "Run command".to_string(),
            detail: PermissionDetail::Bash {
                command: "ls -la".to_string(),
                risk_description: None,
                working_dir: None,
            },
            risk_level: None,
            show_always_allow: true,
            classifier_checking: false,
            classifier_auto_approved: None,
            choices: None,
            selected_choice: 0,
            display_input: coco_types::PermissionDisplayInput::Command("rm -rf /tmp/test".into()),
            original_input: None,
            permission_suggestions: vec![],
            worker_badge: None,
            explanation_visible: false,
            explanation: crate::state::ExplainerFetch::NotFetched,
        }));

    assert!(state.has_active_surface());
    assert!(matches!(
        state.ui.interaction.active_prompt,
        Some(PanePromptState::Permission(_))
    ));
}

#[test]
fn test_streaming_state() {
    let mut state = AppState::new();
    assert!(!state.is_streaming());

    state.ui.streaming = Some(crate::state::ui::StreamingState::new());
    assert!(state.is_streaming());
    assert!(state.should_show_spinner());

    if let Some(ref mut s) = state.ui.streaming {
        s.append_text("hello ");
        s.append_text("world\n");
        assert_eq!(s.content, "hello world\n");
        assert_eq!(s.visible_content(), ""); // cursor at 0

        s.advance_display();
        assert_eq!(s.visible_content(), "hello world\n");
    }
}

#[test]
fn test_token_usage_update() {
    let mut state = AppState::new();

    state.session.update_tokens(TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
        reasoning_tokens: 0,
        cache_read_tokens: 20,
        cache_creation_tokens: 10,
    });

    assert_eq!(state.session.token_usage.input_tokens, 100);
    assert_eq!(state.session.token_usage.output_tokens, 50);
}
