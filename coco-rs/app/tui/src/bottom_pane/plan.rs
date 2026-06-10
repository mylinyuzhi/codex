//! Plan-mode prompt behavior: PlanEntry (enter plan mode), PlanExit (leave
//! with a target permission mode), and PlanApproval (teammate plan review).

use coco_types::PermissionMode;
use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::state::AppState;

/// Approve a PlanEntry prompt: flip into Plan mode.
pub(crate) async fn approve_plan_entry(
    state: &mut AppState,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    state.toggle_plan_mode();
    let _ = command_tx
        .send(UserCommand::SetPermissionMode {
            mode: state.session.permission_mode,
        })
        .await;
}

/// Approve a PlanExit prompt: the target mode depends on which approval
/// option the user picked. `RestorePrePlan` defers the mode switch to
/// `ExitPlanModeTool::execute`, which writes the restored mode onto
/// `app_state.permission_mode` (source of truth); the other variants
/// explicitly set the target mode via `SetPermissionMode` because the user's
/// pick overrides the stashed `pre_plan_mode`.
///
/// Defense in depth: if the state somehow holds `BypassPermissions` but the
/// capability gate is off, down-shift to `AcceptEdits` rather than silently
/// escalating. Normal paths can't reach this (the renderer and cycle honor
/// the gate) but a stale state is cheap to defend against.
pub(crate) async fn approve_plan_exit(
    state: &mut AppState,
    next_mode: crate::state::PlanExitTarget,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    let mut next = next_mode;
    if next == crate::state::PlanExitTarget::BypassPermissions
        && !state.session.bypass_permissions_available
    {
        next = crate::state::PlanExitTarget::AcceptEdits;
    }
    let target = next.resolve().unwrap_or(PermissionMode::Default);
    state.session.permission_mode = target;
    let _ = command_tx
        .send(UserCommand::SetPermissionMode { mode: target })
        .await;
}

/// Deny a PlanExit prompt: the user rejected the plan. Surface a visible
/// record in the chat transcript — TS parity: `RejectedPlanMessage`
/// component renders the plan in a bordered block. Mode stays in `Plan` (no
/// mutation); the user can keep refining or exit via the normal toggle.
/// Routed through the engine round-trip so the entry surfaces via
/// `MessageAppended` like every other system row.
pub(crate) async fn deny_plan_exit(
    plan_content: Option<String>,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    let plan = plan_content.unwrap_or_default();
    let body = if plan.trim().is_empty() {
        crate::i18n::t!("plan.rejected_empty").to_string()
    } else {
        format!("{}\n\n{plan}", crate::i18n::t!("plan.rejected_header"),)
    };
    let _ = command_tx
        .send(UserCommand::PushSystemMessage {
            kind: crate::command::SystemPushKind::Informational {
                level: coco_messages::SystemMessageLevel::Info,
                title: String::new(),
                message: body,
            },
        })
        .await;
}

/// Confirm (Enter) a PlanExit prompt: commit the focused target mode.
pub(crate) async fn confirm_plan_exit(
    state: &mut AppState,
    next_mode: crate::state::PlanExitTarget,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    let target = next_mode.resolve().unwrap_or(PermissionMode::Default);
    state.session.permission_mode = target;
    let _ = command_tx
        .send(UserCommand::SetPermissionMode { mode: target })
        .await;
}

/// Confirm (Enter) a PlanApproval prompt: ship the focused approve/reject
/// decision for the teammate's plan.
pub(crate) async fn confirm_plan_approval(
    p: &crate::state::PlanApprovalPromptState,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    let _ = command_tx
        .send(UserCommand::PlanApprovalResponse {
            request_id: p.request_id.clone(),
            teammate_agent: p.from.clone(),
            approved: p.is_approve_focused(),
            feedback: None,
        })
        .await;
}

/// Move the PlanExit target-mode cursor by `delta` (wrapping over the
/// gate-filtered option order).
pub(crate) fn nav_plan_exit(
    p: &mut crate::state::PlanExitPromptState,
    bypass_permissions_available: bool,
    delta: i32,
) {
    let order = crate::state::PlanExitTarget::available(bypass_permissions_available);
    let current_idx = order.iter().position(|t| *t == p.next_mode).unwrap_or(0) as i32;
    let len = order.len() as i32;
    let new_idx = ((current_idx + delta).rem_euclid(len)) as usize;
    p.next_mode = order[new_idx];
}
