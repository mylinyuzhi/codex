//! Confirmation dialog renderers.

use ratatui::prelude::Color;

use crate::presentation::confirm;
use crate::state::AutoModeOptInOverlay;
use crate::state::BridgeOverlay;
use crate::state::BypassPermissionsOverlay;
use crate::state::CostWarningOverlay;
use crate::state::DoctorOverlay;
use crate::state::ElicitationOverlay;
use crate::state::FeedbackOverlay;
use crate::state::IdleReturnOverlay;
use crate::state::InvalidConfigOverlay;
use crate::state::McpServerApprovalOverlay;
use crate::state::PlanApprovalOverlay;
use crate::state::PlanEntryOverlay;
use crate::state::PlanExitOverlay;
use crate::state::SandboxPermissionOverlay;
use crate::state::TaskDetailOverlay;
use crate::state::TrustOverlay;
use crate::state::WorktreeExitOverlay;
use crate::theme::Theme;

pub(super) fn cost_warning_content(
    c: &CostWarningOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::cost_warning_content(c, theme)
}

pub(super) fn plan_exit_content(
    p: &PlanExitOverlay,
    bypass_permissions_available: bool,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::plan_exit_content(p, bypass_permissions_available, theme)
}

pub(super) fn plan_entry_content(p: &PlanEntryOverlay, theme: &Theme) -> (String, String, Color) {
    confirm::plan_entry_content(p, theme)
}

pub(super) fn sandbox_content(
    s: &SandboxPermissionOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::sandbox_content(s, theme)
}

pub(super) fn elicitation_content(
    e: &ElicitationOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::elicitation_content(e, theme)
}

pub(super) fn mcp_server_approval_content(
    m: &McpServerApprovalOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::mcp_server_approval_content(m, theme)
}

pub(super) fn worktree_exit_content(
    w: &WorktreeExitOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::worktree_exit_content(w, theme)
}

pub(super) fn doctor_content(d: &DoctorOverlay, theme: &Theme) -> (String, String, Color) {
    confirm::doctor_content(d, theme)
}

pub(super) fn bridge_content(b: &BridgeOverlay, theme: &Theme) -> (String, String, Color) {
    confirm::bridge_content(b, theme)
}

pub(super) fn invalid_config_content(
    ic: &InvalidConfigOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::invalid_config_content(ic, theme)
}

pub(super) fn idle_return_content(
    ir: &IdleReturnOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::idle_return_content(ir, theme)
}

pub(super) fn trust_content(tr: &TrustOverlay, theme: &Theme) -> (String, String, Color) {
    confirm::trust_content(tr, theme)
}

pub(super) fn auto_mode_opt_in_content(
    a: &AutoModeOptInOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::auto_mode_opt_in_content(a, theme)
}

pub(super) fn bypass_permissions_content(
    bp: &BypassPermissionsOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::bypass_permissions_content(bp, theme)
}

pub(super) fn task_detail_content(
    td: &TaskDetailOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::task_detail_content(td, theme)
}

pub(super) fn plan_approval_content(
    p: &PlanApprovalOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    confirm::plan_approval_content(p, theme)
}

pub(super) fn feedback_content(f: &FeedbackOverlay, theme: &Theme) -> (String, String, Color) {
    confirm::feedback_content(f, theme)
}
