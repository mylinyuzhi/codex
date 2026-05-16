//! Confirmation dialog renderers.

use ratatui::prelude::Color;

use crate::presentation::confirm;
use crate::presentation::styles::UiStyles;
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

pub(super) fn cost_warning_content(
    c: &CostWarningOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::cost_warning_content(c, styles)
}

pub(super) fn plan_exit_content(
    p: &PlanExitOverlay,
    bypass_permissions_available: bool,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::plan_exit_content(p, bypass_permissions_available, styles)
}

pub(super) fn plan_entry_content(
    p: &PlanEntryOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::plan_entry_content(p, styles)
}

pub(super) fn sandbox_content(
    s: &SandboxPermissionOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::sandbox_content(s, styles)
}

pub(super) fn elicitation_content(
    e: &ElicitationOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::elicitation_content(e, styles)
}

pub(super) fn mcp_server_approval_content(
    m: &McpServerApprovalOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::mcp_server_approval_content(m, styles)
}

pub(super) fn worktree_exit_content(
    w: &WorktreeExitOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::worktree_exit_content(w, styles)
}

pub(super) fn doctor_content(d: &DoctorOverlay, styles: UiStyles<'_>) -> (String, String, Color) {
    confirm::doctor_content(d, styles)
}

pub(super) fn bridge_content(b: &BridgeOverlay, styles: UiStyles<'_>) -> (String, String, Color) {
    confirm::bridge_content(b, styles)
}

pub(super) fn invalid_config_content(
    ic: &InvalidConfigOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::invalid_config_content(ic, styles)
}

pub(super) fn idle_return_content(
    ir: &IdleReturnOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::idle_return_content(ir, styles)
}

pub(super) fn trust_content(tr: &TrustOverlay, styles: UiStyles<'_>) -> (String, String, Color) {
    confirm::trust_content(tr, styles)
}

pub(super) fn auto_mode_opt_in_content(
    a: &AutoModeOptInOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::auto_mode_opt_in_content(a, styles)
}

pub(super) fn bypass_permissions_content(
    bp: &BypassPermissionsOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::bypass_permissions_content(bp, styles)
}

pub(super) fn task_detail_content(
    td: &TaskDetailOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::task_detail_content(td, styles)
}

pub(super) fn plan_approval_content(
    p: &PlanApprovalOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::plan_approval_content(p, styles)
}

pub(super) fn feedback_content(
    f: &FeedbackOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    confirm::feedback_content(f, styles)
}
