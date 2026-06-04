//! Leader-side resolution of cross-process teammate permission requests.
//!
//! A pane teammate (separate process) that hits a deny-path tool forwards
//! the approval prompt to the leader's inbox via mailbox IPC (see
//! [`coco_coordinator::MailboxPermissionBridge`]). The leader's per-turn
//! inbox poll ([`coco_query`]'s plan-mode reminder) hands each such request
//! to the setter registered here; we route it to the leader's human approval
//! UI (the session's `TuiPermissionBridge`) and write the decision back to
//! the worker's inbox.
//!
//! TS: `useInboxPoller.ts` routes a `PermissionRequest` into the leader's
//! `ToolUseConfirm` queue, then replies via `sendPermissionResponseViaMailbox`.

use std::sync::Arc;

use coco_coordinator::mailbox;
use coco_tool_runtime::ToolPermissionBridgeRef;
use coco_tool_runtime::ToolPermissionDecision;
use coco_tool_runtime::ToolPermissionRequest;

/// Register the leader's permission-queue setter, backed by `bridge` (the
/// session's `TuiPermissionBridge`). The coordinator holds one global setter;
/// call once for a leader session when agent-teams is enabled.
pub async fn register(bridge: ToolPermissionBridgeRef) {
    let setter: coco_coordinator::teammate::PermissionQueueSetter = Arc::new(move |value| {
        let bridge = bridge.clone();
        tokio::spawn(async move { handle_request(bridge, value).await });
    });
    coco_coordinator::teammate::register_leader_permission_queue(setter).await;
}

/// Prompt the leader (human) for a worker's permission request, then write
/// the decision back to the worker's inbox. On any failure we write nothing,
/// leaving the worker's bounded wait to fail closed.
async fn handle_request(bridge: ToolPermissionBridgeRef, value: serde_json::Value) {
    let Ok(mailbox::ProtocolMessage::PermissionRequest {
        request_id,
        agent_id,
        tool_name,
        tool_use_id,
        description,
        input,
        ..
    }) = serde_json::from_value::<mailbox::ProtocolMessage>(value)
    else {
        return;
    };
    // The worker's agent_id is `worker_name@team_name`.
    let Some((worker_name, team_name)) = agent_id.rsplit_once('@') else {
        tracing::warn!(%agent_id, "leader permission: malformed worker agent id");
        return;
    };

    let req = ToolPermissionRequest {
        id: request_id.clone(),
        tool_use_id,
        agent_id: agent_id.clone(),
        tool_name,
        description,
        input,
        suggestions: Vec::new(),
        choices: None,
    };
    let resolution = match bridge.request_permission(req).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, %request_id, "leader permission: bridge declined to resolve");
            return;
        }
    };
    let approved = matches!(resolution.decision, ToolPermissionDecision::Approved);
    if let Err(e) = mailbox::send_permission_response_via_mailbox(
        worker_name,
        &request_id,
        approved,
        resolution.feedback.as_deref(),
        resolution.updated_input,
        resolution.applied_updates,
        team_name,
    ) {
        tracing::warn!(error = %e, %request_id, "leader permission: failed to reply to worker");
    }
}
