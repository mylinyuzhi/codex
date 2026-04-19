//! Confirmation dialog renderers — small, mostly one-screen text overlays.
//!
//! Covers: cost warning, plan exit/entry, sandbox permission, MCP server
//! approval, worktree exit, doctor, bridge, invalid config, idle return,
//! trust, auto-mode opt-in, bypass permissions, task detail, feedback,
//! elicitation. Kept together because each is short.

use ratatui::prelude::Color;

use crate::i18n::t;
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
    (
        t!("dialog.title_cost").to_string(),
        format!(
            "{}\n{}\n\n{}",
            t!(
                "dialog.cost_current",
                amount = format!("{:.2}", c.current_cost_cents as f64 / 100.0)
            ),
            t!(
                "dialog.cost_threshold",
                amount = format!("{:.2}", c.threshold_cents as f64 / 100.0)
            ),
            t!("dialog.cost_continue"),
        ),
        theme.warning,
    )
}

pub(super) fn plan_exit_content(
    p: &PlanExitOverlay,
    bypass_permissions_available: bool,
    theme: &Theme,
) -> (String, String, Color) {
    use crate::state::PlanExitTarget;

    // TS parity: `buildPlanApprovalOptions()` offers "Yes, keep
    // default" / "Yes, auto-accept edits" / (conditionally) "Yes, and
    // bypass permissions" plus a "No" path. The bypass entry is only
    // rendered when the session was authorized to reach
    // `BypassPermissions` at startup — matching TS's
    // `isBypassPermissionsModeAvailable` gate.
    let plan_body = p
        .plan_content
        .clone()
        .unwrap_or_else(|| t!("dialog.exit_plan_prompt").to_string());
    let rendered: Vec<String> = PlanExitTarget::available(bypass_permissions_available)
        .into_iter()
        .map(|target| {
            let label = match target {
                PlanExitTarget::RestorePrePlan => t!("dialog.exit_plan_opt_restore"),
                PlanExitTarget::AcceptEdits => t!("dialog.exit_plan_opt_accept_edits"),
                PlanExitTarget::BypassPermissions => t!("dialog.exit_plan_opt_bypass"),
            };
            let marker = if target == p.next_mode { "▸ " } else { "  " };
            format!("{marker}{label}")
        })
        .collect();
    (
        t!("dialog.title_exit_plan").to_string(),
        format!(
            "{plan_body}\n\n{}\n\n{}",
            rendered.join("\n"),
            t!("dialog.exit_plan_hint"),
        ),
        theme.plan_mode,
    )
}

pub(super) fn plan_entry_content(p: &PlanEntryOverlay, theme: &Theme) -> (String, String, Color) {
    (
        t!("dialog.title_enter_plan").to_string(),
        format!("{}\n\n{}", p.description, t!("dialog.confirm_yn")),
        theme.plan_mode,
    )
}

pub(super) fn sandbox_content(
    s: &SandboxPermissionOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    (
        t!("dialog.title_sandbox").to_string(),
        format!("{}\n\n{}", s.description, t!("dialog.allow_yn")),
        theme.error,
    )
}

pub(super) fn elicitation_content(
    e: &ElicitationOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    (
        format!(" {} ", e.server_name),
        format!("{}\n\n{}", e.message, t!("dialog.fill_fields_hint")),
        theme.accent,
    )
}

pub(super) fn mcp_server_approval_content(
    m: &McpServerApprovalOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    (
        t!("dialog.title_mcp_server").to_string(),
        format!(
            "{}\n{}\n{}\n\n{}",
            t!("dialog.server_prefix", name = m.server_name.as_str()),
            m.server_url.as_deref().unwrap_or(""),
            t!("dialog.tools_prefix", list = m.tools.join(", ")),
            t!("dialog.actions_approve_deny"),
        ),
        theme.accent,
    )
}

pub(super) fn worktree_exit_content(
    w: &WorktreeExitOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let files = if w.changed_files.is_empty() {
        t!("dialog.no_uncommitted_changes").to_string()
    } else {
        w.changed_files
            .iter()
            .map(|f| format!("  {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    (
        t!("dialog.title_exit_worktree").to_string(),
        format!(
            "{}\n\n{files}\n\n{}",
            t!("dialog.branch_prefix", name = w.branch.as_str()),
            t!("dialog.yn_exit_stay"),
        ),
        theme.warning,
    )
}

pub(super) fn doctor_content(d: &DoctorOverlay, theme: &Theme) -> (String, String, Color) {
    let checks: Vec<String> = d
        .checks
        .iter()
        .map(|c| {
            let icon = if c.passed { "✓" } else { "✗" };
            format!("  {icon} {}: {}", c.name, c.message)
        })
        .collect();
    let body = if checks.is_empty() {
        format!(
            "{}\n\n{}",
            t!("dialog.running_diagnostics"),
            t!("dialog.esc_close")
        )
    } else {
        format!("{}\n\n{}", checks.join("\n"), t!("dialog.esc_close"))
    };
    (t!("dialog.title_doctor").to_string(), body, theme.primary)
}

pub(super) fn bridge_content(b: &BridgeOverlay, theme: &Theme) -> (String, String, Color) {
    (
        t!("dialog.title_bridge", bridge_type = b.bridge_type.as_str()).to_string(),
        format!(
            "{}\n\n{}\n\n{}",
            t!("dialog.status_prefix", status = b.status.as_str()),
            b.details,
            t!("dialog.esc_close")
        ),
        theme.accent,
    )
}

pub(super) fn invalid_config_content(
    ic: &InvalidConfigOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let errors = ic
        .errors
        .iter()
        .map(|e| format!("  • {e}"))
        .collect::<Vec<_>>()
        .join("\n");
    (
        t!("dialog.title_invalid_config").to_string(),
        format!(
            "{}\n\n{errors}\n\n{}",
            t!("dialog.config_errors"),
            t!("dialog.hints_invalid_config"),
        ),
        theme.error,
    )
}

pub(super) fn idle_return_content(
    ir: &IdleReturnOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let mins = ir.idle_duration_secs / 60;
    (
        t!("dialog.title_welcome_back").to_string(),
        format!(
            "{}\n\n{}",
            t!("dialog.welcome_back_body", mins = mins),
            t!("dialog.enter_continue")
        ),
        theme.primary,
    )
}

pub(super) fn trust_content(tr: &TrustOverlay, theme: &Theme) -> (String, String, Color) {
    (
        t!("dialog.title_trust").to_string(),
        format!(
            "{}\n\n  {}\n\n{}\n\n{}",
            t!("dialog.trust_prompt"),
            tr.path,
            tr.description,
            t!("dialog.yn_trust_deny"),
        ),
        theme.warning,
    )
}

pub(super) fn auto_mode_opt_in_content(
    a: &AutoModeOptInOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    (
        t!("dialog.title_auto_mode").to_string(),
        format!("{}\n\n{}", a.description, t!("dialog.enable_auto_approve")),
        theme.primary,
    )
}

pub(super) fn bypass_permissions_content(
    bp: &BypassPermissionsOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    (
        t!("dialog.title_bypass_permissions").to_string(),
        format!(
            "{}\n\n{}",
            t!("dialog.bypass_body", mode = bp.current_mode.as_str()),
            t!("dialog.yn_enable_cancel"),
        ),
        theme.error,
    )
}

pub(super) fn task_detail_content(
    td: &TaskDetailOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let output_lines: Vec<&str> = td.output.lines().collect();
    let visible: String = output_lines
        .iter()
        .skip(td.scroll as usize)
        .take(20)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    (
        t!("dialog.title_task", name = td.task_type.as_str()).to_string(),
        format!(
            "{}\n{}\n\n{visible}\n\n{}",
            td.description,
            t!("dialog.status_prefix", status = td.status.as_str()),
            t!("dialog.scroll_close_hints"),
        ),
        theme.primary,
    )
}

pub(super) fn feedback_content(f: &FeedbackOverlay, theme: &Theme) -> (String, String, Color) {
    let items: Vec<String> = f
        .options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let marker = if i as i32 == f.selected { "▸ " } else { "  " };
            format!("{marker}{opt}")
        })
        .collect();
    (
        t!("dialog.title_feedback").to_string(),
        format!("{}\n\n{}", f.prompt, items.join("\n")),
        theme.primary,
    )
}
