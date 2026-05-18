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
use crate::state::MemoryDialogOverlay;
use crate::state::MemoryDialogRowKind;
use crate::state::MemoryDialogScope;
use crate::state::ModelEntry;
use crate::state::ModelPickerOverlay;
use crate::state::Overlay;
use crate::state::ProviderUnavailableReason;
use crate::state::ui::ToastSeverity;
use coco_types::ModelRole;
use coco_types::ReasoningEffort;
use tokio::sync::mpsc;

fn picker(entries: Vec<ModelEntry>, selected: i32, effort: Option<ReasoningEffort>) -> AppState {
    let mut s = AppState::new();
    s.ui.set_overlay(Overlay::ModelPicker(ModelPickerOverlay {
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
    let Some(Overlay::ModelPicker(m)) = s.ui.active_overlay() else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::Medium));
    cycle_model_effort(&mut s, 1);
    let Some(Overlay::ModelPicker(m)) = s.ui.active_overlay() else {
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
    let Some(Overlay::ModelPicker(m)) = s.ui.active_overlay() else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::Low));
    // Wrap back from Low → High.
    cycle_model_effort(&mut s, -1);
    let Some(Overlay::ModelPicker(m)) = s.ui.active_overlay() else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::High));
}

#[test]
fn cycle_effort_noops_when_no_supported_levels() {
    let mut s = picker(vec![entry("m", &[], None)], 0, None);
    cycle_model_effort(&mut s, 1);
    let Some(Overlay::ModelPicker(m)) = s.ui.active_overlay() else {
        panic!()
    };
    assert!(m.effort.is_none());
}

#[test]
fn cycle_effort_noops_outside_picker() {
    let mut s = AppState::new();
    // No overlay → cycle_effort should silently no-op (no panic).
    cycle_model_effort(&mut s, 1);
    assert!(!s.ui.has_overlay());
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
        s.ui.active_overlay(),
        Some(Overlay::ModelPicker(_))
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
        .set_overlay(Overlay::MemoryDialog(MemoryDialogOverlay {
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
    assert!(!state.ui.has_overlay(), "overlay dismissed after select");
}

#[tokio::test]
async fn confirm_memory_dialog_keeps_non_file_rows_open() {
    let mut state = AppState::new();
    state
        .ui
        .set_overlay(Overlay::MemoryDialog(MemoryDialogOverlay {
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
        matches!(state.ui.active_overlay(), Some(Overlay::MemoryDialog(_))),
        "overlay stays open"
    );
    assert_eq!(state.ui.toasts.len(), 1);
    assert_eq!(state.ui.toasts[0].severity, ToastSeverity::Warning);
}

// ── Permission overlay: multi-choice commit path ──
//
// TS parity: `ExitPlanModePermissionRequest.tsx:691-704` — the
// user picks via arrows and Enter; the chosen `value` is spliced
// into `updated_input` and sent back as `ApprovalResponse`.

fn permission_with_choices(values: &[&str], selected: usize) -> AppState {
    use crate::state::PermissionDetail;
    use crate::state::PermissionOverlay;
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
    s.ui.set_overlay(Overlay::Permission(PermissionOverlay {
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
    assert!(!s.ui.has_overlay(), "overlay dismissed after commit");
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
    use crate::state::PermissionOverlay;
    let mut s = AppState::new();
    s.ui.set_overlay(Overlay::Permission(PermissionOverlay {
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
    assert!(!s.ui.has_overlay(), "overlay dismissed");
}

#[tokio::test]
async fn confirm_classic_always_allow_sends_session_update() {
    use crate::state::PermissionDetail;
    use crate::state::PermissionOverlay;
    let mut s = AppState::new();
    s.ui.set_overlay(Overlay::Permission(PermissionOverlay {
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
    assert!(!s.ui.has_overlay(), "overlay dismissed");
}

#[tokio::test]
async fn confirm_classic_read_always_allow_sends_path_scoped_session_update() {
    use crate::state::PermissionDetail;
    use crate::state::PermissionOverlay;
    let dir = std::env::temp_dir().join("coco-tui-read-permission-test");
    let file = dir.join("notes.txt");
    let mut s = AppState::new();
    s.ui.set_overlay(Overlay::Permission(PermissionOverlay {
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
    let Some(Overlay::Permission(p)) = s.ui.active_overlay() else {
        panic!()
    };
    assert_eq!(p.selected_choice, 1);
    nav(&mut s, 5);
    let Some(Overlay::Permission(p)) = s.ui.active_overlay() else {
        panic!()
    };
    assert_eq!(p.selected_choice, 0);
    nav(&mut s, -1);
    let Some(Overlay::Permission(p)) = s.ui.active_overlay() else {
        panic!()
    };
    assert_eq!(p.selected_choice, 2);
}

#[test]
fn build_choice_payload_merges_with_original_input() {
    use crate::state::PermissionDetail;
    use crate::state::PermissionOverlay;
    use coco_types::PermissionAskChoice;

    let p = PermissionOverlay {
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
    };
    let out = build_choice_payload(&p).expect("payload built");
    assert_eq!(out["existing"], 42);
    assert_eq!(out["other"], "v");
    assert_eq!(out["user_choice"], "pick-1");
}

#[test]
fn build_choice_payload_none_when_cursor_out_of_range() {
    use crate::state::PermissionDetail;
    use crate::state::PermissionOverlay;

    let p = PermissionOverlay {
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
    let m = ModelPickerOverlay {
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
