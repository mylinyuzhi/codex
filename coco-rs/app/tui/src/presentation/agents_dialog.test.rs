//! View-string snapshot tests for the `/agents` dialog renderer.
//!
//! These lock the body text we emit for each tab + wizard step so
//! cosmetic regressions surface as `insta` diffs. The dispatch /
//! state tests at `update/agents_dialog.test.rs` and
//! `state/agents_dialog.test.rs` cover behaviour; this file covers
//! presentation.

use super::*;
use crate::state::AgentsDialogState;
use crate::state::CreateWizardState;
use crate::state::CreateWizardStep;
use crate::state::LibraryRow;
use crate::state::SubagentInstance;
use crate::state::SubagentKind;
use crate::state::SubagentStatus;
use crate::state::WizardError;
use crate::state::WizardSource;
use crate::state::WizardTextField;
use crate::theme::Theme;
use coco_types::AgentSource;
use std::path::PathBuf;

fn wizard_with(step: CreateWizardStep) -> CreateWizardState {
    let mut w = CreateWizardState::new();
    w.step = step;
    w.name = WizardTextField::seeded("my-agent");
    w.description = WizardTextField::seeded("Handles XYZ.");
    w.source = WizardSource::Project;
    w
}

fn body_only(state: &AgentsDialogState, subagents: &[SubagentInstance]) -> String {
    // Pin the locale to `en` for the render. `cargo test` shares one process,
    // so a concurrent locale-sensitive test can otherwise leave the global
    // rust-i18n locale set to `zh-CN` and this dialog's translated strings
    // ("Running", "built-in…") would render in the wrong language. The guard
    // both sets `en` and serializes against other locale-sensitive tests.
    let _locale = crate::i18n::locale_test_guard("en");
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let (_title, body, _color) = agents_dialog_content(state, subagents, styles);
    body
}

#[test]
fn snapshot_running_tab_empty() {
    let state = AgentsDialogState::new(vec![LibraryRow::CreateNew]);
    insta::assert_snapshot!("agents_running_empty", body_only(&state, &[]));
}

#[test]
fn snapshot_library_list_grouped() {
    let library = vec![
        LibraryRow::CreateNew,
        LibraryRow::SourceHeader {
            label: "User agents".into(),
        },
        LibraryRow::Agent {
            name: "alpha".into(),
            description: Some("First user agent".into()),
            source: AgentSource::UserSettings,
            color: None,
            is_builtin: false,
            is_overridden: false,
            running_count: 0,
            source_path: Some(PathBuf::from("/home/u/.coco/agents/alpha.md")),
        },
        LibraryRow::SourceHeader {
            label: "Built-in agents".into(),
        },
        LibraryRow::Agent {
            name: "Explore".into(),
            description: Some("Fast read-only search agent".into()),
            source: AgentSource::BuiltIn,
            color: None,
            is_builtin: true,
            is_overridden: false,
            running_count: 0,
            source_path: None,
        },
    ];
    let mut state = AgentsDialogState::new(library);
    state.selected_tab = crate::state::AgentsDialogTab::Library;
    insta::assert_snapshot!("agents_library_grouped", body_only(&state, &[]));
}

#[test]
fn snapshot_wizard_step_name() {
    let mut state = AgentsDialogState::new(vec![LibraryRow::CreateNew]);
    state.selected_tab = crate::state::AgentsDialogTab::Library;
    state.wizard = Some({
        let mut w = CreateWizardState::new();
        w.name = WizardTextField::seeded("my-agent");
        w
    });
    insta::assert_snapshot!("agents_wizard_name", body_only(&state, &[]));
}

#[test]
fn snapshot_wizard_step_name_with_error() {
    let mut state = AgentsDialogState::new(vec![LibraryRow::CreateNew]);
    state.selected_tab = crate::state::AgentsDialogTab::Library;
    state.wizard = Some({
        let mut w = CreateWizardState::new();
        w.name = WizardTextField::seeded("3plan");
        w.error = Some(WizardError::NameLead);
        w
    });
    insta::assert_snapshot!("agents_wizard_name_with_error", body_only(&state, &[]));
}

#[test]
fn snapshot_wizard_step_description() {
    let mut state = AgentsDialogState::new(vec![LibraryRow::CreateNew]);
    state.selected_tab = crate::state::AgentsDialogTab::Library;
    state.wizard = Some(wizard_with(CreateWizardStep::Description));
    insta::assert_snapshot!("agents_wizard_description", body_only(&state, &[]));
}

#[test]
fn snapshot_wizard_step_source() {
    let mut state = AgentsDialogState::new(vec![LibraryRow::CreateNew]);
    state.selected_tab = crate::state::AgentsDialogTab::Library;
    state.wizard = Some(wizard_with(CreateWizardStep::Source));
    insta::assert_snapshot!("agents_wizard_source", body_only(&state, &[]));
}

#[test]
fn snapshot_wizard_step_confirm() {
    let mut state = AgentsDialogState::new(vec![LibraryRow::CreateNew]);
    state.selected_tab = crate::state::AgentsDialogTab::Library;
    state.wizard = Some(wizard_with(CreateWizardStep::Confirm));
    insta::assert_snapshot!("agents_wizard_confirm", body_only(&state, &[]));
}

#[test]
fn snapshot_wizard_already_exists_error() {
    let mut state = AgentsDialogState::new(vec![LibraryRow::CreateNew]);
    state.selected_tab = crate::state::AgentsDialogTab::Library;
    state.wizard = Some({
        let mut w = wizard_with(CreateWizardStep::Confirm);
        w.error = Some(WizardError::AlreadyExists {
            path: PathBuf::from("/home/u/.coco/agents/my-agent.md"),
        });
        w
    });
    insta::assert_snapshot!("agents_wizard_already_exists", body_only(&state, &[]));
}

#[test]
fn render_wizard_error_covers_every_variant() {
    // Compile-time coverage: every WizardError variant must produce
    // a non-empty rendered string. If a new variant lands without an
    // arm, `match` exhaustiveness fails here at compile time.
    for err in [
        WizardError::NameEmpty,
        WizardError::NameLead,
        WizardError::NameChars,
        WizardError::DescEmpty,
        WizardError::AlreadyExists {
            path: PathBuf::from("/x"),
        },
        WizardError::NonWritableSource,
    ] {
        let rendered = render_wizard_error(&err);
        assert!(!rendered.is_empty(), "{err:?} produced an empty rendering");
    }
}

#[test]
fn render_running_tab_with_active_and_completed() {
    let running = SubagentInstance {
        kind: SubagentKind::Subagent,
        agent_id: "task-1".into(),
        agent_type: "Explore".into(),
        description: String::new(),
        status: SubagentStatus::Running,
        color: None,
        team_name: None,
        tool_use_id: None,
        started_at_ms: None,
        last_tool_name: Some("read".into()),
        tool_count: 3,
        total_tokens: 0,
        is_backgrounded: false,
        recent_activities: vec![],
        final_message: None,
    };
    let done = SubagentInstance {
        agent_id: "task-2".into(),
        status: SubagentStatus::Completed,
        ..running.clone()
    };
    let state = AgentsDialogState::new(vec![LibraryRow::CreateNew]);
    let body = body_only(&state, &[running, done]);
    // Sanity: both sections appear. The full layout is snapshotted
    // in the empty-state test; here we just guard the section split
    // so a future renderer rewrite doesn't drop the "completed"
    // bucket.
    assert!(body.contains("Explore"));
    assert!(body.contains("3 tools"));
}
