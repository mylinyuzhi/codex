//! Overlay content builders — one file per category to stay under the 800-LoC
//! module-size guideline.
//!
//! Each submodule produces `(title, body, border_color)` for the caller to
//! wrap in a centered `Paragraph` with a `Block` border.

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
use crate::state::Overlay;

pub(crate) enum TextOverlay<'a> {
    Permission(&'a crate::state::PermissionOverlay),
    Help,
    Error(&'a str),
    PlanExit(&'a crate::state::PlanExitOverlay),
    PlanEntry(&'a crate::state::PlanEntryOverlay),
    CostWarning(&'a crate::state::CostWarningOverlay),
    ModelPicker(&'a crate::state::ModelPickerOverlay),
    SessionBrowser(&'a crate::state::SessionBrowserOverlay),
    Question(&'a crate::state::QuestionOverlay),
    Elicitation(&'a crate::state::ElicitationOverlay),
    SandboxPermission(&'a crate::state::SandboxPermissionOverlay),
    GlobalSearch(&'a crate::state::GlobalSearchOverlay),
    QuickOpen(&'a crate::state::QuickOpenOverlay),
    Export(&'a crate::state::ExportOverlay),
    DiffView(&'a crate::state::DiffViewOverlay),
    McpServerApproval(&'a crate::state::McpServerApprovalOverlay),
    WorktreeExit(&'a crate::state::WorktreeExitOverlay),
    Doctor(&'a crate::state::DoctorOverlay),
    Bridge(&'a crate::state::BridgeOverlay),
    InvalidConfig(&'a crate::state::InvalidConfigOverlay),
    IdleReturn(&'a crate::state::IdleReturnOverlay),
    Trust(&'a crate::state::TrustOverlay),
    AutoModeOptIn(&'a crate::state::AutoModeOptInOverlay),
    BypassPermissions(&'a crate::state::BypassPermissionsOverlay),
    TaskDetail(&'a crate::state::TaskDetailOverlay),
    Feedback(&'a crate::state::FeedbackOverlay),
    McpServerSelect(&'a crate::state::McpServerSelectOverlay),
    ContextVisualization,
    Rewind(&'a crate::state::RewindOverlay),
    Settings(&'a crate::widgets::settings_panel::SettingsPanelState),
    PlanApproval(&'a crate::state::PlanApprovalOverlay),
    MemoryDialog(&'a crate::state::MemoryDialogOverlay),
}

pub(crate) fn text_overlay(overlay: &Overlay) -> Option<TextOverlay<'_>> {
    Some(match overlay {
        Overlay::Permission(p) => TextOverlay::Permission(p),
        Overlay::Help => TextOverlay::Help,
        Overlay::Error(msg) => TextOverlay::Error(msg),
        Overlay::PlanExit(p) => TextOverlay::PlanExit(p),
        Overlay::PlanEntry(p) => TextOverlay::PlanEntry(p),
        Overlay::CostWarning(c) => TextOverlay::CostWarning(c),
        Overlay::ModelPicker(m) => TextOverlay::ModelPicker(m),
        Overlay::SessionBrowser(s) => TextOverlay::SessionBrowser(s),
        Overlay::Question(q) => TextOverlay::Question(q),
        Overlay::Elicitation(e) => TextOverlay::Elicitation(e),
        Overlay::SandboxPermission(s) => TextOverlay::SandboxPermission(s),
        Overlay::GlobalSearch(g) => TextOverlay::GlobalSearch(g),
        Overlay::QuickOpen(q) => TextOverlay::QuickOpen(q),
        Overlay::Export(e) => TextOverlay::Export(e),
        Overlay::DiffView(d) => TextOverlay::DiffView(d),
        Overlay::McpServerApproval(m) => TextOverlay::McpServerApproval(m),
        Overlay::WorktreeExit(w) => TextOverlay::WorktreeExit(w),
        Overlay::Doctor(d) => TextOverlay::Doctor(d),
        Overlay::Bridge(b) => TextOverlay::Bridge(b),
        Overlay::InvalidConfig(ic) => TextOverlay::InvalidConfig(ic),
        Overlay::IdleReturn(ir) => TextOverlay::IdleReturn(ir),
        Overlay::Trust(tr) => TextOverlay::Trust(tr),
        Overlay::AutoModeOptIn(a) => TextOverlay::AutoModeOptIn(a),
        Overlay::BypassPermissions(bp) => TextOverlay::BypassPermissions(bp),
        Overlay::TaskDetail(td) => TextOverlay::TaskDetail(td),
        Overlay::Feedback(f) => TextOverlay::Feedback(f),
        Overlay::McpServerSelect(ms) => TextOverlay::McpServerSelect(ms),
        Overlay::ContextVisualization => TextOverlay::ContextVisualization,
        Overlay::Rewind(r) => TextOverlay::Rewind(r),
        Overlay::Settings(s) => TextOverlay::Settings(s),
        Overlay::PlanApproval(p) => TextOverlay::PlanApproval(p),
        Overlay::MemoryDialog(m) => TextOverlay::MemoryDialog(m),
        Overlay::Transcript(_) | Overlay::CommandPalette(_) => return None,
    })
}

/// Produce (title, body, border_color) for text overlays.
pub(crate) fn overlay_content(
    overlay: TextOverlay<'_>,
    state: &AppState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    match overlay {
        TextOverlay::Permission(p) => permission::permission_content(p, styles),
        TextOverlay::Help => help::help_content(state, styles),
        TextOverlay::Error(msg) => (
            t!("dialog.title_error").to_string(),
            msg.to_string(),
            styles.error(),
        ),
        TextOverlay::PlanExit(p) => {
            confirm::plan_exit_content(p, state.session.bypass_permissions_available, styles)
        }
        TextOverlay::PlanEntry(p) => confirm::plan_entry_content(p, styles),
        TextOverlay::CostWarning(c) => confirm::cost_warning_content(c, styles),
        TextOverlay::ModelPicker(m) => pickers::model_picker_content(m, styles),
        TextOverlay::SessionBrowser(s) => pickers::session_browser_content(s, styles),
        TextOverlay::Question(q) => question::question_content(q, styles),
        TextOverlay::Elicitation(e) => confirm::elicitation_content(e, styles),
        TextOverlay::SandboxPermission(s) => confirm::sandbox_content(s, styles),
        TextOverlay::GlobalSearch(g) => search::global_search_content(g, styles),
        TextOverlay::QuickOpen(q) => pickers::quick_open_content(q, styles),
        TextOverlay::Export(e) => pickers::export_content(e, styles),
        TextOverlay::DiffView(d) => diff::diff_view_content(d, styles),
        TextOverlay::McpServerApproval(m) => confirm::mcp_server_approval_content(m, styles),
        TextOverlay::WorktreeExit(w) => confirm::worktree_exit_content(w, styles),
        TextOverlay::Doctor(d) => confirm::doctor_content(d, styles),
        TextOverlay::Bridge(b) => confirm::bridge_content(b, styles),
        TextOverlay::InvalidConfig(ic) => confirm::invalid_config_content(ic, styles),
        TextOverlay::IdleReturn(ir) => confirm::idle_return_content(ir, styles),
        TextOverlay::Trust(tr) => confirm::trust_content(tr, styles),
        TextOverlay::AutoModeOptIn(a) => confirm::auto_mode_opt_in_content(a, styles),
        TextOverlay::BypassPermissions(bp) => confirm::bypass_permissions_content(bp, styles),
        TextOverlay::TaskDetail(td) => confirm::task_detail_content(td, styles),
        TextOverlay::Feedback(f) => confirm::feedback_content(f, styles),
        TextOverlay::McpServerSelect(ms) => pickers::mcp_server_select_content(ms, styles),
        TextOverlay::ContextVisualization => context_viz::context_viz_content(state, styles),
        TextOverlay::Rewind(r) => rewind::rewind_overlay_content(r, styles),
        TextOverlay::Settings(s) => settings::settings_overlay_content(s, styles),
        TextOverlay::PlanApproval(p) => confirm::plan_approval_content(p, styles),
        TextOverlay::MemoryDialog(m) => pickers::memory_dialog_content(m, styles),
    }
}
