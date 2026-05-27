//! Surface content builders — one file per category to stay under the 800-LoC
//! module-size guideline.
//!
//! Each submodule produces `(title, body, border_color)` for the caller to
//! wrap in a `Paragraph` with a `Block` border.

mod confirm;
mod context_viz;
mod diff;
mod help;
mod permission;
mod pickers;
mod question;
mod rewind;
mod search;
mod settings;

use ratatui::prelude::*;

use crate::i18n::t;
use crate::presentation::styles::UiStyles;
use crate::state::AppState;
use crate::state::ModalState;
use crate::state::PanePromptState;

pub(crate) enum TextSurfaceContent<'a> {
    Permission(&'a crate::state::PermissionPromptState),
    Help,
    Error(&'a str),
    PlanExit(&'a crate::state::PlanExitPromptState),
    PlanEntry(&'a crate::state::PlanEntryPromptState),
    CostWarning(&'a crate::state::CostWarningPromptState),
    ModelPicker(&'a crate::state::ModelPickerState),
    SessionBrowser(&'a crate::state::SessionBrowserState),
    Question(&'a crate::state::QuestionPromptState),
    SandboxPermission(&'a crate::state::SandboxPermissionPromptState),
    GlobalSearch(&'a crate::state::GlobalSearchState),
    QuickOpen(&'a crate::state::QuickOpenState),
    Export(&'a crate::state::ExportState),
    DiffView(&'a crate::state::DiffViewState),
    McpServerApproval(&'a crate::state::McpServerApprovalPromptState),
    WorktreeExit(&'a crate::state::WorktreeExitState),
    Doctor(&'a crate::state::DoctorState),
    Bridge(&'a crate::state::BridgeState),
    InvalidConfig(&'a crate::state::InvalidConfigState),
    IdleReturn(&'a crate::state::IdleReturnState),
    Trust(&'a crate::state::TrustState),
    AutoModeOptIn(&'a crate::state::AutoModeOptInState),
    BypassPermissions(&'a crate::state::BypassPermissionsState),
    TaskDetail(&'a crate::state::TaskDetailState),
    Feedback(&'a crate::state::FeedbackState),
    McpServerSelect(&'a crate::state::McpServerSelectState),
    ContextVisualization,
    Rewind(&'a crate::state::RewindState),
    Settings(&'a crate::widgets::settings_panel::SettingsPanelState),
    PlanApproval(&'a crate::state::PlanApprovalPromptState),
    MemoryDialog(&'a crate::state::MemoryDialogState),
    SkillsDialog(&'a crate::state::SkillsDialogState),
    AgentsDialog(&'a crate::state::AgentsDialogState),
    CopyPicker(&'a crate::state::CopyPickerState),
}

pub(crate) fn prompt_text_surface(prompt: &PanePromptState) -> TextSurfaceContent<'_> {
    match prompt {
        PanePromptState::Permission(p) => TextSurfaceContent::Permission(p),
        PanePromptState::Question(q) => TextSurfaceContent::Question(q),
        PanePromptState::SandboxPermission(s) => TextSurfaceContent::SandboxPermission(s),
        PanePromptState::CostWarning(c) => TextSurfaceContent::CostWarning(c),
        PanePromptState::PlanEntry(p) => TextSurfaceContent::PlanEntry(p),
        PanePromptState::PlanExit(p) => TextSurfaceContent::PlanExit(p),
        PanePromptState::PlanApproval(p) => TextSurfaceContent::PlanApproval(p),
        PanePromptState::McpServerApproval(m) => TextSurfaceContent::McpServerApproval(m),
    }
}

pub(crate) fn modal_text_surface(modal: &ModalState) -> Option<TextSurfaceContent<'_>> {
    Some(match modal {
        ModalState::Help => TextSurfaceContent::Help,
        ModalState::Error(msg) => TextSurfaceContent::Error(msg),
        ModalState::ModelPicker(m) => TextSurfaceContent::ModelPicker(m),
        ModalState::SessionBrowser(s) => TextSurfaceContent::SessionBrowser(s),
        ModalState::GlobalSearch(g) => TextSurfaceContent::GlobalSearch(g),
        ModalState::QuickOpen(q) => TextSurfaceContent::QuickOpen(q),
        ModalState::Export(e) => TextSurfaceContent::Export(e),
        ModalState::DiffView(d) => TextSurfaceContent::DiffView(d),
        ModalState::Rewind(r) => TextSurfaceContent::Rewind(r),
        ModalState::Settings(s) => TextSurfaceContent::Settings(s),
        ModalState::MemoryDialog(m) => TextSurfaceContent::MemoryDialog(m),
        ModalState::SkillsDialog(s) => TextSurfaceContent::SkillsDialog(s),
        ModalState::AgentsDialog(a) => TextSurfaceContent::AgentsDialog(a),
        ModalState::Doctor(d) => TextSurfaceContent::Doctor(d),
        ModalState::ContextVisualization => TextSurfaceContent::ContextVisualization,
        ModalState::WorktreeExit(w) => TextSurfaceContent::WorktreeExit(w),
        ModalState::Bridge(b) => TextSurfaceContent::Bridge(b),
        ModalState::InvalidConfig(ic) => TextSurfaceContent::InvalidConfig(ic),
        ModalState::IdleReturn(ir) => TextSurfaceContent::IdleReturn(ir),
        ModalState::Trust(tr) => TextSurfaceContent::Trust(tr),
        ModalState::AutoModeOptIn(a) => TextSurfaceContent::AutoModeOptIn(a),
        ModalState::BypassPermissions(bp) => TextSurfaceContent::BypassPermissions(bp),
        ModalState::TaskDetail(td) => TextSurfaceContent::TaskDetail(td),
        ModalState::Feedback(f) => TextSurfaceContent::Feedback(f),
        ModalState::McpServerSelect(ms) => TextSurfaceContent::McpServerSelect(ms),
        ModalState::CopyPicker(cp) => TextSurfaceContent::CopyPicker(cp),
        ModalState::Transcript(_) => return None,
    })
}

/// Produce (title, body, border_color) for text surfaces.
pub(crate) fn surface_content(
    content: TextSurfaceContent<'_>,
    state: &AppState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    match content {
        TextSurfaceContent::Permission(p) => permission::permission_content(p, styles),
        TextSurfaceContent::Help => help::help_content(state, styles),
        TextSurfaceContent::Error(msg) => (
            t!("dialog.title_error").to_string(),
            msg.to_string(),
            styles.error(),
        ),
        TextSurfaceContent::PlanExit(p) => {
            confirm::plan_exit_content(p, state.session.bypass_permissions_available, styles)
        }
        TextSurfaceContent::PlanEntry(p) => confirm::plan_entry_content(p, styles),
        TextSurfaceContent::CostWarning(c) => confirm::cost_warning_content(c, styles),
        TextSurfaceContent::ModelPicker(m) => pickers::model_picker_content(m, styles),
        TextSurfaceContent::SessionBrowser(s) => pickers::session_browser_content(s, styles),
        TextSurfaceContent::Question(q) => question::question_content(q, styles),
        TextSurfaceContent::SandboxPermission(s) => confirm::sandbox_content(s, styles),
        TextSurfaceContent::GlobalSearch(g) => search::global_search_content(g, styles),
        TextSurfaceContent::QuickOpen(q) => pickers::quick_open_content(q, styles),
        TextSurfaceContent::Export(e) => pickers::export_content(e, styles),
        TextSurfaceContent::DiffView(d) => diff::diff_view_content(d, styles),
        TextSurfaceContent::McpServerApproval(m) => confirm::mcp_server_approval_content(m, styles),
        TextSurfaceContent::WorktreeExit(w) => confirm::worktree_exit_content(w, styles),
        TextSurfaceContent::Doctor(d) => confirm::doctor_content(d, styles),
        TextSurfaceContent::Bridge(b) => confirm::bridge_content(b, styles),
        TextSurfaceContent::InvalidConfig(ic) => confirm::invalid_config_content(ic, styles),
        TextSurfaceContent::IdleReturn(ir) => confirm::idle_return_content(ir, styles),
        TextSurfaceContent::Trust(tr) => confirm::trust_content(tr, styles),
        TextSurfaceContent::AutoModeOptIn(a) => confirm::auto_mode_opt_in_content(a, styles),
        TextSurfaceContent::BypassPermissions(bp) => {
            confirm::bypass_permissions_content(bp, styles)
        }
        TextSurfaceContent::TaskDetail(td) => confirm::task_detail_content(td, styles),
        TextSurfaceContent::Feedback(f) => confirm::feedback_content(f, styles),
        TextSurfaceContent::McpServerSelect(ms) => pickers::mcp_server_select_content(ms, styles),
        TextSurfaceContent::ContextVisualization => context_viz::context_viz_content(state, styles),
        TextSurfaceContent::Rewind(r) => rewind::rewind_surface_content(r, styles),
        TextSurfaceContent::Settings(s) => settings::settings_surface_content(s, styles),
        TextSurfaceContent::PlanApproval(p) => confirm::plan_approval_content(p, styles),
        TextSurfaceContent::MemoryDialog(m) => pickers::memory_dialog_content(m, styles),
        TextSurfaceContent::SkillsDialog(s) => pickers::skills_dialog_content(s, styles),
        TextSurfaceContent::AgentsDialog(a) => {
            pickers::agents_dialog_content(a, &state.session.subagents, styles)
        }
        TextSurfaceContent::CopyPicker(cp) => pickers::copy_picker_content(cp, styles),
    }
}
