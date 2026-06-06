//! Picker-mechanics tests focused on the parts that aren't covered
//! by the broader `update` integration tests.
//!
//! Effort cycling is the most fragile bit: the focused entry's
//! `supported_efforts` list drives the wraparound, and missing
//! entries (e.g. a model without thinking capability) must no-op
//! rather than panic.

use super::*;
use crate::command::UserCommand;
use crate::state::AppState;
use crate::state::MemoryDialogEntry;
use crate::state::MemoryDialogRowKind;
use crate::state::MemoryDialogScope;
use crate::state::MemoryDialogState;
use crate::state::ModalState;
use crate::state::ModelEntry;
use crate::state::ModelPickerState;
use crate::state::PanePromptState;
use crate::state::ProviderUnavailableReason;
use crate::state::ui::ToastSeverity;
use coco_types::ModelRole;
use coco_types::ReasoningEffort;
use tokio::sync::mpsc;

fn picker(entries: Vec<ModelEntry>, selected: i32, effort: Option<ReasoningEffort>) -> AppState {
    let mut s = AppState::new();
    s.ui.show_modal(ModalState::ModelPicker(ModelPickerState {
        role: ModelRole::Main,
        entries,
        filter: String::new(),
        selected,
        effort,
    }));
    s
}

fn entry(
    model_id: &str,
    efforts: &[ReasoningEffort],
    default: Option<ReasoningEffort>,
) -> ModelEntry {
    ModelEntry {
        provider: "test".into(),
        provider_display: "Test".into(),
        model_id: model_id.into(),
        display_name: model_id.into(),
        context_window: Some(200_000),
        supported_efforts: efforts.to_vec(),
        default_effort: default,
        is_current_for_role: false,
        unavailable_reasons: Vec::new(),
    }
}

#[test]
fn cycle_effort_advances_through_supported_levels() {
    let levels = [
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
    ];
    let mut s = picker(
        vec![entry("m", &levels, Some(ReasoningEffort::Low))],
        0,
        Some(ReasoningEffort::Low),
    );
    cycle_model_effort(&mut s, 1);
    let Some(ModalState::ModelPicker(m)) = s.ui.modal.as_ref() else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::Medium));
    cycle_model_effort(&mut s, 1);
    let Some(ModalState::ModelPicker(m)) = s.ui.modal.as_ref() else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::High));
}

#[test]
fn cycle_effort_wraps_around_at_endpoints() {
    let levels = [
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
    ];
    let mut s = picker(
        vec![entry("m", &levels, Some(ReasoningEffort::High))],
        0,
        Some(ReasoningEffort::High),
    );
    // Wrap forward from High → Low.
    cycle_model_effort(&mut s, 1);
    let Some(ModalState::ModelPicker(m)) = s.ui.modal.as_ref() else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::Low));
    // Wrap back from Low → High.
    cycle_model_effort(&mut s, -1);
    let Some(ModalState::ModelPicker(m)) = s.ui.modal.as_ref() else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::High));
}

#[test]
fn cycle_effort_noops_when_no_supported_levels() {
    let mut s = picker(vec![entry("m", &[], None)], 0, None);
    cycle_model_effort(&mut s, 1);
    let Some(ModalState::ModelPicker(m)) = s.ui.modal.as_ref() else {
        panic!()
    };
    assert!(m.effort.is_none());
}

#[test]
fn cycle_effort_noops_outside_picker() {
    let mut s = AppState::new();
    // No state → cycle_effort should silently no-op (no panic).
    cycle_model_effort(&mut s, 1);
    assert!(!s.ui.has_active_surface());
}

#[tokio::test]
async fn confirm_model_picker_blocks_unavailable_provider() {
    let mut unavailable = entry("m", &[], None);
    unavailable
        .unavailable_reasons
        .push(ProviderUnavailableReason::MissingApiKey {
            env_key: "TEST_API_KEY".to_string(),
        });
    let mut s = picker(vec![unavailable], 0, None);
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);

    confirm(&mut s, &tx).await;

    assert!(rx.try_recv().is_err(), "no model-change command sent");
    assert!(matches!(
        s.ui.modal.as_ref(),
        Some(ModalState::ModelPicker(_))
    ));
    assert_eq!(s.ui.toasts.len(), 1);
    assert_eq!(s.ui.toasts[0].severity, ToastSeverity::Warning);
    assert!(s.ui.toasts[0].message.contains("TEST_API_KEY"));
}

#[tokio::test]
async fn confirm_memory_dialog_sends_open_memory_file_command() {
    let path = std::path::PathBuf::from("/tmp/coco-memory-test/CLAUDE.md");
    let mut state = AppState::new();
    state
        .ui
        .show_modal(ModalState::MemoryDialog(MemoryDialogState {
            entries: vec![MemoryDialogEntry {
                path: path.clone(),
                label: "Project memory".to_string(),
                scope: MemoryDialogScope::Project,
                row_kind: MemoryDialogRowKind::File {
                    exists: false,
                    read_only: false,
                },
            }],
            selected: 0,
        }));
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);

    confirm(&mut state, &tx).await;

    let UserCommand::OpenMemoryFile { path: sent_path } =
        rx.try_recv().expect("memory open command sent")
    else {
        panic!("expected OpenMemoryFile")
    };
    assert_eq!(sent_path, path);
    assert!(
        !state.ui.has_active_surface(),
        "state dismissed after select"
    );
}

#[tokio::test]
async fn confirm_memory_dialog_keeps_non_file_rows_open() {
    let mut state = AppState::new();
    state
        .ui
        .show_modal(ModalState::MemoryDialog(MemoryDialogState {
            entries: vec![MemoryDialogEntry {
                path: std::path::PathBuf::from("/tmp/coco-memory-test"),
                label: "Auto-memory folder".to_string(),
                scope: MemoryDialogScope::User,
                row_kind: MemoryDialogRowKind::Folder { enabled: true },
            }],
            selected: 0,
        }));
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);

    confirm(&mut state, &tx).await;

    assert!(rx.try_recv().is_err(), "no editor command for non-file row");
    assert!(
        matches!(state.ui.modal.as_ref(), Some(ModalState::MemoryDialog(_))),
        "state stays open"
    );
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Warning);
}

// ── Permission state: multi-choice commit path ──
//
// TS parity: `ExitPlanModePermissionRequest.tsx:691-704` — the
// user picks via arrows and Enter; the chosen `value` is spliced
// into `updated_input` and sent back as `ApprovalResponse`.

fn permission_with_choices(values: &[&str], selected: usize) -> AppState {
    use crate::state::PermissionDetail;
    use crate::state::PermissionPromptState;
    use coco_types::PermissionAskChoice;

    let mut s = AppState::new();
    let choices: Vec<PermissionAskChoice> = values
        .iter()
        .map(|v| PermissionAskChoice {
            value: (*v).to_string(),
            label: (*v).to_string(),
            description: None,
        })
        .collect();
    s.ui.push_prompt(PanePromptState::Permission(PermissionPromptState {
        request_id: "req-1".into(),
        tool_name: "ExitPlanMode".into(),
        description: "Exit plan mode?".into(),
        detail: PermissionDetail::Generic {
            input_preview: String::new(),
        },
        risk_level: None,
        show_always_allow: false,
        classifier_checking: false,
        classifier_auto_approved: None,
        choices: Some(choices),
        selected_choice: selected,
        display_input: coco_types::PermissionDisplayInput::Empty,
        original_input: Some(serde_json::json!({"plan": "do the thing"})),
        permission_suggestions: vec![],
        worker_badge: None,
    }));
    s
}

#[tokio::test]
async fn confirm_with_choice_splices_user_choice_into_updated_input() {
    // Selecting "yes-clear-context" should send approved=true with
    // user_choice spliced into the original input — the engine reads
    // this off ExitPlanModeTool's input to flag history clear.
    let mut s = permission_with_choices(
        &["yes-keep-context", "yes-clear-context", "no"],
        1, // "yes-clear-context"
    );
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);
    confirm(&mut s, &tx).await;

    let cmd = rx.try_recv().expect("approval sent");
    let UserCommand::ApprovalResponse {
        approved,
        updated_input,
        ..
    } = cmd
    else {
        panic!("expected ApprovalResponse")
    };
    assert!(approved, "non-'no' choice should approve");
    let payload = updated_input.expect("updated_input populated");
    assert_eq!(payload["plan"], "do the thing");
    assert_eq!(payload["user_choice"], "yes-clear-context");
    assert!(!s.ui.has_active_surface(), "state dismissed after commit");
}

#[tokio::test]
async fn confirm_with_no_choice_sends_approved_false() {
    // "no" is the sentinel for deny; engine treats it as a regular
    // denial (tool doesn't execute). updated_input still carries the
    // value so logs/audits see what the user picked.
    let mut s = permission_with_choices(
        &["yes-keep-context", "yes-clear-context", "no"],
        2, // "no"
    );
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);
    confirm(&mut s, &tx).await;

    let cmd = rx.try_recv().expect("approval sent");
    let UserCommand::ApprovalResponse {
        approved,
        updated_input,
        ..
    } = cmd
    else {
        panic!("expected ApprovalResponse")
    };
    assert!(!approved, "'no' choice should deny");
    let payload = updated_input.expect("updated_input populated");
    assert_eq!(payload["user_choice"], "no");
}

#[tokio::test]
async fn approve_with_choice_takes_same_path_as_confirm() {
    // Pressing 'y' (Approve) when choices are present must commit the
    // currently-focused choice, not the implicit yes — otherwise the
    // tool would see updated_input=None and lose the user's pick.
    let mut s = permission_with_choices(&["yes-keep-context", "yes-clear-context"], 1);
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);
    approve(&mut s, &tx).await;

    let UserCommand::ApprovalResponse {
        approved,
        updated_input,
        ..
    } = rx.try_recv().expect("approval sent")
    else {
        panic!()
    };
    assert!(approved);
    let payload = updated_input.expect("updated_input populated");
    assert_eq!(payload["user_choice"], "yes-clear-context");
}

#[tokio::test]
async fn confirm_classic_yes_no_approves_selected_action() {
    // No choices → Enter commits the focused classic action, matching
    // TS PermissionPrompt / codex-rs list-selection behavior.
    use crate::state::PermissionDetail;
    use crate::state::PermissionPromptState;
    let mut s = AppState::new();
    s.ui.push_prompt(PanePromptState::Permission(PermissionPromptState {
        request_id: "req-1".into(),
        tool_name: "Bash".into(),
        description: "Run".into(),
        detail: PermissionDetail::Generic {
            input_preview: "ls".into(),
        },
        risk_level: None,
        show_always_allow: true,
        classifier_checking: false,
        classifier_auto_approved: None,
        choices: None,
        selected_choice: 0,
        display_input: coco_types::PermissionDisplayInput::Command("ls".into()),
        original_input: None,
        permission_suggestions: vec![],
        worker_badge: None,
    }));
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);
    confirm(&mut s, &tx).await;

    let UserCommand::ApprovalResponse {
        approved,
        always_allow,
        permission_updates,
        ..
    } = rx.try_recv().expect("approval sent")
    else {
        panic!("expected ApprovalResponse")
    };
    assert!(approved);
    assert!(!always_allow);
    assert!(permission_updates.is_empty());
    assert!(!s.ui.has_active_surface(), "state dismissed");
}

#[tokio::test]
async fn confirm_classic_always_allow_sends_session_update() {
    use crate::state::PermissionDetail;
    use crate::state::PermissionPromptState;
    let mut s = AppState::new();
    s.ui.push_prompt(PanePromptState::Permission(PermissionPromptState {
        request_id: "req-1".into(),
        tool_name: "Bash".into(),
        description: "Run".into(),
        detail: PermissionDetail::Generic {
            input_preview: "ls".into(),
        },
        risk_level: None,
        show_always_allow: true,
        classifier_checking: false,
        classifier_auto_approved: None,
        choices: None,
        selected_choice: 1,
        display_input: coco_types::PermissionDisplayInput::Command("ls".into()),
        original_input: None,
        permission_suggestions: vec![],
        worker_badge: None,
    }));
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);
    confirm(&mut s, &tx).await;

    let UserCommand::ApprovalResponse {
        approved,
        always_allow,
        permission_updates,
        ..
    } = rx.try_recv().expect("approval sent")
    else {
        panic!("expected ApprovalResponse")
    };
    assert!(approved);
    assert!(always_allow);
    assert_eq!(permission_updates.len(), 1);
    assert!(!s.ui.has_active_surface(), "state dismissed");
}

#[tokio::test]
async fn confirm_classic_read_always_allow_sends_path_scoped_session_update() {
    use crate::state::PermissionDetail;
    use crate::state::PermissionPromptState;
    let dir = std::env::temp_dir().join("coco-tui-read-permission-test");
    let file = dir.join("notes.txt");
    let mut s = AppState::new();
    s.ui.push_prompt(PanePromptState::Permission(PermissionPromptState {
        request_id: "req-1".into(),
        tool_name: "Read".into(),
        description: "Read outside cwd".into(),
        detail: PermissionDetail::Generic {
            input_preview: file.display().to_string(),
        },
        risk_level: None,
        show_always_allow: true,
        classifier_checking: false,
        classifier_auto_approved: None,
        choices: None,
        selected_choice: 1,
        display_input: coco_types::PermissionDisplayInput::Text(file.display().to_string()),
        original_input: Some(serde_json::json!({"file_path": file.to_string_lossy()})),
        permission_suggestions: vec![],
        worker_badge: None,
    }));
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);
    confirm(&mut s, &tx).await;

    let UserCommand::ApprovalResponse {
        permission_updates, ..
    } = rx.try_recv().expect("approval sent")
    else {
        panic!("expected ApprovalResponse")
    };
    let [coco_types::PermissionUpdate::AddRules { rules, destination }] =
        permission_updates.as_slice()
    else {
        panic!("expected AddRules update")
    };
    assert_eq!(
        *destination,
        coco_types::PermissionUpdateDestination::Session
    );
    assert_eq!(rules[0].value.tool_pattern, "Read");
    let expected = format!("/{}/**", dir.to_string_lossy());
    assert_eq!(
        rules[0].value.rule_content.as_deref(),
        Some(expected.as_str())
    );
}

#[test]
fn nav_advances_selected_choice_with_wraparound() {
    let mut s = permission_with_choices(&["a", "b", "c"], 0);
    nav(&mut s, 1);
    let Some(PanePromptState::Permission(p)) = s.ui.interaction.active_prompt.as_ref() else {
        panic!()
    };
    assert_eq!(p.selected_choice, 1);
    nav(&mut s, 5);
    let Some(PanePromptState::Permission(p)) = s.ui.interaction.active_prompt.as_ref() else {
        panic!()
    };
    assert_eq!(p.selected_choice, 0);
    nav(&mut s, -1);
    let Some(PanePromptState::Permission(p)) = s.ui.interaction.active_prompt.as_ref() else {
        panic!()
    };
    assert_eq!(p.selected_choice, 2);
}

#[test]
fn build_choice_payload_merges_with_original_input() {
    use crate::state::PermissionDetail;
    use crate::state::PermissionPromptState;
    use coco_types::PermissionAskChoice;

    let p = PermissionPromptState {
        request_id: "req-1".into(),
        tool_name: "Foo".into(),
        description: String::new(),
        detail: PermissionDetail::Generic {
            input_preview: String::new(),
        },
        risk_level: None,
        show_always_allow: false,
        classifier_checking: false,
        classifier_auto_approved: None,
        choices: Some(vec![PermissionAskChoice {
            value: "pick-1".into(),
            label: "Pick 1".into(),
            description: None,
        }]),
        selected_choice: 0,
        display_input: coco_types::PermissionDisplayInput::Empty,
        original_input: Some(serde_json::json!({"existing": 42, "other": "v"})),
        permission_suggestions: vec![],
        worker_badge: None,
    };
    let out = build_choice_payload(&p).expect("payload built");
    assert_eq!(out["existing"], 42);
    assert_eq!(out["other"], "v");
    assert_eq!(out["user_choice"], "pick-1");
}

#[test]
fn build_choice_payload_none_when_cursor_out_of_range() {
    use crate::state::PermissionDetail;
    use crate::state::PermissionPromptState;

    let p = PermissionPromptState {
        request_id: "req-1".into(),
        tool_name: "Foo".into(),
        description: String::new(),
        detail: PermissionDetail::Generic {
            input_preview: String::new(),
        },
        risk_level: None,
        show_always_allow: false,
        classifier_checking: false,
        classifier_auto_approved: None,
        choices: Some(vec![]),
        selected_choice: 5,
        display_input: coco_types::PermissionDisplayInput::Empty,
        original_input: None,
        permission_suggestions: vec![],
        worker_badge: None,
    };
    assert!(build_choice_payload(&p).is_none());
}

#[test]
fn filtered_models_matches_provider_display() {
    let entries = vec![
        ModelEntry {
            provider: "anthropic".into(),
            provider_display: "Anthropic".into(),
            model_id: "claude-haiku-4-5".into(),
            display_name: "Claude Haiku".into(),
            context_window: Some(200_000),
            supported_efforts: vec![],
            default_effort: None,
            is_current_for_role: false,
            unavailable_reasons: Vec::new(),
        },
        ModelEntry {
            provider: "openai".into(),
            provider_display: "OpenAI".into(),
            model_id: "gpt-5-4".into(),
            display_name: "GPT-5.4".into(),
            context_window: Some(272_000),
            supported_efforts: vec![],
            default_effort: None,
            is_current_for_role: false,
            unavailable_reasons: Vec::new(),
        },
    ];
    let m = ModelPickerState {
        role: ModelRole::Main,
        entries,
        filter: "open".to_string(),
        selected: 0,
        effort: None,
    };
    let filtered = filtered_models(&m);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].provider, "openai");
}

/// `team_roster_cycle_mode` (gap 8 / R8) cycles the FOCUSED member's own mode
/// through the four interactive modes with wraparound, seeding from that
/// member's current mode (not a hardcoded `Default`) and leaving OTHER members
/// untouched — the per-member independence a single shared mode field could
/// not represent.
#[test]
fn team_roster_cycle_mode_wraps_interactive_modes() {
    use coco_types::PermissionMode as PM;
    let member = |name: &str, mode: PM| crate::state::TeamRosterMember {
        name: name.into(),
        agent_type: "explore".into(),
        color: None,
        mode,
    };
    let mut s = AppState::new();
    s.ui.show_modal(ModalState::TeamRoster(crate::state::TeamRosterState {
        team_name: "t".into(),
        // Two members with DIFFERENT live modes; focus is on member 0.
        members: vec![
            member("researcher", PM::Plan),
            member("builder", PM::AcceptEdits),
        ],
        selected: 0,
    }));

    let focused = |s: &AppState| match s.ui.modal.as_ref() {
        Some(ModalState::TeamRoster(r)) => r.members[r.selected].mode,
        _ => panic!("expected TeamRoster"),
    };
    let other = |s: &AppState| match s.ui.modal.as_ref() {
        Some(ModalState::TeamRoster(r)) => r.members[1].mode,
        _ => panic!("expected TeamRoster"),
    };

    // Seeds from the focused member's CURRENT mode (Plan), not Default. The
    // returned (name, mode) is what gets persisted immediately (TS-faithful).
    assert_eq!(
        team_roster_cycle_mode(&mut s, 1),
        Some(("researcher".to_string(), PM::BypassPermissions))
    );
    assert_eq!(focused(&s), PM::BypassPermissions);
    team_roster_cycle_mode(&mut s, 1); // wrap → Default
    assert_eq!(focused(&s), PM::Default);
    team_roster_cycle_mode(&mut s, -1); // wrap backward → BypassPermissions
    assert_eq!(focused(&s), PM::BypassPermissions);

    // The unfocused member's mode was never touched.
    assert_eq!(
        other(&s),
        PM::AcceptEdits,
        "cycling member 0 must not affect member 1"
    );
}

fn roster_state(modes: &[(&str, coco_types::PermissionMode)]) -> AppState {
    let mut s = AppState::new();
    s.ui.show_modal(ModalState::TeamRoster(crate::state::TeamRosterState {
        team_name: "t".into(),
        members: modes
            .iter()
            .map(|(name, mode)| crate::state::TeamRosterMember {
                name: (*name).into(),
                agent_type: "explore".into(),
                color: None,
                mode: *mode,
            })
            .collect(),
        selected: 0,
    }));
    s
}

fn roster_modes(s: &AppState) -> Vec<coco_types::PermissionMode> {
    match s.ui.modal.as_ref() {
        Some(ModalState::TeamRoster(r)) => r.members.iter().map(|m| m.mode).collect(),
        _ => panic!("expected TeamRoster"),
    }
}

/// `team_roster_cycle_all_modes` (R8 cycle-all, TS `cycleAllTeammateModes`):
/// when every teammate already shares the same mode, advance ALL by `delta` in
/// tandem and return the full batch of updates.
#[test]
fn team_roster_cycle_all_modes_all_same_advances_in_tandem() {
    use coco_types::PermissionMode as PM;
    let mut s = roster_state(&[("a", PM::Default), ("b", PM::Default), ("c", PM::Default)]);

    let updates = team_roster_cycle_all_modes(&mut s, 1);
    assert_eq!(
        updates,
        vec![
            ("a".to_string(), PM::AcceptEdits),
            ("b".to_string(), PM::AcceptEdits),
            ("c".to_string(), PM::AcceptEdits),
        ]
    );
    assert_eq!(roster_modes(&s), vec![PM::AcceptEdits; 3]);
}

/// When modes DIVERGE, TS normalises every teammate to `Default` first
/// (regardless of `delta` direction).
#[test]
fn team_roster_cycle_all_modes_divergent_resets_to_default() {
    use coco_types::PermissionMode as PM;
    let mut s = roster_state(&[("a", PM::Plan), ("b", PM::AcceptEdits), ("c", PM::Plan)]);

    let updates = team_roster_cycle_all_modes(&mut s, 1);
    assert!(
        updates.iter().all(|(_, m)| *m == PM::Default),
        "got {updates:?}"
    );
    assert_eq!(roster_modes(&s), vec![PM::Default; 3]);

    // Now that they're all equal, a second cycle advances them together.
    let updates2 = team_roster_cycle_all_modes(&mut s, 1);
    assert!(updates2.iter().all(|(_, m)| *m == PM::AcceptEdits));
}

/// Empty roster ⇒ no-op (no updates, no panic).
#[test]
fn team_roster_cycle_all_modes_empty_is_noop() {
    let mut s = roster_state(&[]);
    assert!(team_roster_cycle_all_modes(&mut s, 1).is_empty());
}

// === AskUserQuestion key + answer mechanics ===

fn question_option(label: &str) -> crate::state::QuestionOption {
    crate::state::QuestionOption {
        label: label.into(),
        description: String::new(),
        preview: None,
        kind: crate::state::OptionKind::Pick,
    }
}

fn question_item(
    options: Vec<crate::state::QuestionOption>,
    multi_select: bool,
) -> crate::state::QuestionItem {
    crate::state::QuestionItem {
        header: "h".into(),
        question: "Q?".into(),
        options,
        multi_select,
        selected: 0,
        checked: Vec::new(),
        notes: String::new(),
    }
}

fn question_state(item: crate::state::QuestionItem) -> AppState {
    let mut s = AppState::new();
    s.ui.push_prompt(PanePromptState::Question(
        crate::state::QuestionPromptState {
            request_id: "q1".into(),
            original_input: serde_json::json!({}),
            questions: vec![item],
            focus: crate::state::QuestionFocus::Question(0),
            is_in_plan_mode: false,
            submit_selected: 0,
        },
    ));
    s
}

fn focused_selected(s: &AppState) -> i32 {
    let Some(PanePromptState::Question(q)) = s.ui.interaction.active_prompt.as_ref() else {
        panic!("expected a question prompt");
    };
    q.questions[0].selected
}

fn question_state_multi(items: Vec<crate::state::QuestionItem>) -> AppState {
    let mut s = AppState::new();
    s.ui.push_prompt(PanePromptState::Question(
        crate::state::QuestionPromptState {
            request_id: "q1".into(),
            original_input: serde_json::json!({}),
            questions: items,
            focus: crate::state::QuestionFocus::Question(0),
            is_in_plan_mode: false,
            submit_selected: 0,
        },
    ));
    s
}

fn focused_question(s: &AppState) -> crate::state::QuestionFocus {
    let Some(PanePromptState::Question(q)) = s.ui.interaction.active_prompt.as_ref() else {
        panic!("expected a question prompt");
    };
    q.focus
}

fn set_focus(s: &mut AppState, focus: crate::state::QuestionFocus) {
    if let Some(PanePromptState::Question(q)) = s.ui.interaction.active_prompt.as_mut() {
        q.focus = focus;
    }
}

#[test]
fn question_switch_question_walks_questions_then_submit_with_wrap() {
    use crate::state::QuestionFocus;
    // 3 questions → nav ring [Q0, Q1, Q2, Submit].
    let mut s = question_state_multi(vec![
        question_item(vec![question_option("A"), question_option("B")], false),
        question_item(vec![question_option("C"), question_option("D")], false),
        question_item(vec![question_option("E"), question_option("F")], false),
    ]);
    super::question_switch_question(&mut s, 1);
    assert_eq!(focused_question(&s), QuestionFocus::Question(1));
    super::question_switch_question(&mut s, 1);
    assert_eq!(focused_question(&s), QuestionFocus::Question(2));
    super::question_switch_question(&mut s, 1);
    assert_eq!(
        focused_question(&s),
        QuestionFocus::Submit,
        "after the last question → Submit tab"
    );
    super::question_switch_question(&mut s, 1);
    assert_eq!(
        focused_question(&s),
        QuestionFocus::Question(0),
        "Submit wraps to the first question"
    );
    super::question_switch_question(&mut s, -1);
    assert_eq!(
        focused_question(&s),
        QuestionFocus::Submit,
        "Left from the first question wraps to Submit"
    );
}

#[test]
fn question_switch_question_from_footer_reenters_ring() {
    use crate::state::QuestionFocus;
    // 2 questions → ring [Q0, Q1, Submit].
    let mut s = question_state_multi(vec![
        question_item(vec![question_option("A"), question_option("B")], false),
        question_item(vec![question_option("C"), question_option("D")], false),
    ]);
    set_focus(&mut s, QuestionFocus::ChatAboutThis);
    super::question_switch_question(&mut s, 1);
    assert_eq!(
        focused_question(&s),
        QuestionFocus::Question(0),
        "Right from footer → first tab"
    );
    set_focus(&mut s, QuestionFocus::ChatAboutThis);
    super::question_switch_question(&mut s, -1);
    assert_eq!(
        focused_question(&s),
        QuestionFocus::Submit,
        "Left from footer → last tab (Submit)"
    );
}

#[test]
fn nav_on_submit_tab_toggles_submit_and_cancel() {
    use crate::state::QuestionFocus;
    let mut s = question_state_multi(vec![
        question_item(vec![question_option("A"), question_option("B")], false),
        question_item(vec![question_option("C"), question_option("D")], false),
    ]);
    set_focus(&mut s, QuestionFocus::Submit);
    let submit_sel = |s: &AppState| -> i32 {
        let Some(PanePromptState::Question(q)) = s.ui.interaction.active_prompt.as_ref() else {
            panic!("expected a question prompt");
        };
        q.submit_selected
    };
    assert_eq!(submit_sel(&s), 0, "starts on Submit");
    super::nav(&mut s, 1);
    assert_eq!(submit_sel(&s), 1, "Down → Cancel");
    super::nav(&mut s, 1);
    assert_eq!(submit_sel(&s), 1, "clamps at Cancel");
    super::nav(&mut s, -1);
    assert_eq!(submit_sel(&s), 0, "Up → Submit");
}

#[test]
fn question_switch_question_single_question_is_noop() {
    use crate::state::QuestionFocus;
    let mut s = question_state(question_item(
        vec![question_option("A"), question_option("B")],
        false,
    ));
    super::question_switch_question(&mut s, 1);
    assert_eq!(focused_question(&s), QuestionFocus::Question(0));
}

#[test]
fn question_digit_shortcut_moves_cursor_to_that_option() {
    let mut s = question_state(question_item(
        vec![
            question_option("A"),
            question_option("B"),
            question_option("C"),
        ],
        false,
    ));
    super::question_select_digit(&mut s, 3);
    assert_eq!(focused_selected(&s), 2);
    // Out-of-range digit is a no-op.
    super::question_select_digit(&mut s, 9);
    assert_eq!(focused_selected(&s), 2);
}

#[test]
fn question_digit_routes_through_filter_when_not_editing_other() {
    let mut s = question_state(question_item(
        vec![question_option("A"), question_option("B")],
        false,
    ));
    super::filter(&mut s, '2');
    assert_eq!(focused_selected(&s), 1);
}

#[test]
fn multi_select_empty_submits_empty_answer_like_ts() {
    // TS `SelectMulti` ships the selected array verbatim — an untouched
    // multi-select question submits an empty answer, NOT the cursor option.
    let q = crate::state::QuestionPromptState {
        request_id: "q1".into(),
        original_input: serde_json::json!({}),
        questions: vec![question_item(
            vec![question_option("A"), question_option("B")],
            true,
        )],
        focus: crate::state::QuestionFocus::Question(0),
        is_in_plan_mode: false,
        submit_selected: 0,
    };
    let payload = super::build_answer_payload(&q);
    let answer = payload["answers"]["Q?"].as_str().unwrap();
    assert_eq!(answer, "", "untouched multi-select must submit empty");
}

fn question_other_option() -> crate::state::QuestionOption {
    crate::state::QuestionOption {
        label: crate::state::OTHER_OPTION_DISPLAY.into(),
        description: String::new(),
        preview: None,
        kind: crate::state::OptionKind::Other,
    }
}

#[test]
fn question_notes_paste_appends_into_focused_other_composer() {
    let mut item = question_item(vec![question_option("A"), question_other_option()], false);
    item.selected = 1; // focus the injected Other composer
    let mut s = question_state(item);
    assert!(super::question_notes_paste(&mut s, "够清楚"));
    let Some(PanePromptState::Question(q)) = s.ui.interaction.active_prompt.as_ref() else {
        panic!("expected a question prompt");
    };
    assert_eq!(q.questions[0].notes, "够清楚", "paste lands in Other notes");
}

#[test]
fn question_notes_paste_ignored_when_other_not_focused() {
    let item = question_item(vec![question_option("A"), question_other_option()], false);
    // selected stays 0 (a normal Pick), not the Other composer.
    let mut s = question_state(item);
    assert!(!super::question_notes_paste(&mut s, "x"));
}

#[tokio::test]
async fn confirm_on_empty_other_keeps_prompt_open_instead_of_submitting() {
    let mut item = question_item(vec![question_option("A"), question_other_option()], false);
    item.selected = 1; // focus Other, notes empty
    let mut s = question_state(item);
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);
    super::confirm(&mut s, &tx).await;
    assert!(rx.try_recv().is_err(), "empty Other must not submit");
    assert!(
        s.ui.interaction.active_prompt.is_some(),
        "prompt stays open so the user can type directly"
    );
}

#[tokio::test]
async fn confirm_on_other_with_typed_notes_submits() {
    let mut item = question_item(vec![question_option("A"), question_other_option()], false);
    item.selected = 1;
    item.notes = "my answer".into();
    let mut s = question_state(item);
    let (tx, mut rx) = mpsc::channel::<UserCommand>(8);
    super::confirm(&mut s, &tx).await;
    match rx.try_recv() {
        Ok(UserCommand::ApprovalResponse { approved, .. }) => {
            assert!(approved, "non-empty Other on the last question submits")
        }
        other => panic!("expected ApprovalResponse, got {other:?}"),
    }
}
