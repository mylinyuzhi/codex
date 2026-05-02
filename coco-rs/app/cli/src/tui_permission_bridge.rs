//! TUI permission bridge — drives the permission overlay from a
//! `ToolPermissionBridge::request_permission` call.
//!
//! ## Why
//!
//! Without an installed bridge, the engine's `permission_controller`
//! treats `PermissionDecision::Ask` as "auto-allow" (legacy headless
//! fallback at `permission_controller.rs:100-107`). For interactive
//! TUI users that's the wrong default — Ask should prompt, not pass.
//!
//! This module wires the loop:
//!
//! ```text
//!  engine                       TUI state                user
//!    │                              │                      │
//!    │ request_permission()         │                      │
//!    │ ─ insert oneshot in pending  │                      │
//!    │ ─ emit ApprovalRequired ────>│ Overlay::Permission  │
//!    │   await oneshot              │ ─────────────────────│
//!    │                              │ <── Approve / Deny ──│
//!    │                              │ UserCommand::Approval
//!    │                              │      Response ──┐    │
//!    │                              │                 ▼    │
//!    │             tui_runner: pop pending oneshot, send   │
//!    │ <─ Approved / Rejected ──────┘                      │
//! ```
//!
//! ## Pieces
//!
//! - [`PendingApprovals`]: shared `Arc<RwLock<HashMap<request_id,
//!   oneshot::Sender>>>` between the bridge (writer) and tui_runner
//!   (reader). Constructed once at TUI startup.
//! - [`TuiPermissionBridge`]: implements `ToolPermissionBridge`. Each
//!   `request_permission` allocates a oneshot, stores the sender in
//!   the pending map, emits `ApprovalRequired` onto the TUI event
//!   channel, and awaits the receiver.
//! - [`resolve_pending`]: tui_runner calls this when
//!   `UserCommand::ApprovalResponse` arrives.
//!
//! ## Cross-mode contract
//!
//! Worker subagents (AgentTool spawns) inherit the leader's bridge
//! via `wire_engine`. So a worker's tool deny in TUI mode prompts the
//! leader's overlay automatically — no per-spawn install needed.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use coco_query::CoreEvent;
use coco_tool_runtime::{
    ToolPermissionBridge, ToolPermissionDecision, ToolPermissionRequest, ToolPermissionResolution,
};
use coco_types::TuiOnlyEvent;
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::warn;

/// Shared sender side of pending approvals — keyed by `request_id` so
/// `resolve_pending` can route the matching response back.
pub type PendingApprovals = Arc<RwLock<HashMap<String, oneshot::Sender<ToolPermissionResolution>>>>;

/// Build a fresh empty pending map. Hand the same `Arc` to
/// [`TuiPermissionBridge::new`] AND the tui_runner's
/// `UserCommand::ApprovalResponse` arm.
pub fn new_pending_map() -> PendingApprovals {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Bridge implementation for the interactive TUI.
///
/// Holds a clone of the TUI's notification channel (so it can emit
/// `TuiOnlyEvent::ApprovalRequired`) and a clone of the pending
/// oneshot map (so `resolve_pending` can complete the await).
pub struct TuiPermissionBridge {
    notification_tx: mpsc::Sender<CoreEvent>,
    pending: PendingApprovals,
}

impl TuiPermissionBridge {
    pub fn new(notification_tx: mpsc::Sender<CoreEvent>, pending: PendingApprovals) -> Self {
        Self {
            notification_tx,
            pending,
        }
    }
}

#[async_trait]
impl ToolPermissionBridge for TuiPermissionBridge {
    async fn request_permission(
        &self,
        request: ToolPermissionRequest,
    ) -> Result<ToolPermissionResolution, String> {
        // Step 1: register the oneshot in the pending map BEFORE
        // emitting the event. Reverse order risks a fast-path race
        // where the user clicks Approve before the entry exists and
        // the resolver finds nothing to send to.
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.write().await;
            pending.insert(request.id.clone(), tx);
        }

        // Step 2: emit ApprovalRequired onto the TUI event channel.
        // The TUI handler at `tui_only.rs:20` consumes it and sets
        // `Overlay::Permission` with the request fields.
        let event = CoreEvent::Tui(TuiOnlyEvent::ApprovalRequired {
            request_id: request.id.clone(),
            tool_name: request.tool_name.clone(),
            description: request.description.clone(),
            input_preview: serde_json::to_string(&request.input)
                .unwrap_or_else(|_| "<unrenderable input>".to_string()),
        });
        if let Err(e) = self.notification_tx.send(event).await {
            // Channel closed → the TUI is shutting down. Pull the
            // pending entry back so we don't leak the oneshot, and
            // bail out closed.
            self.pending.write().await.remove(&request.id);
            return Err(format!("TUI notification channel closed: {e}"));
        }

        // Step 3: await the user's decision (or cancellation).
        match rx.await {
            Ok(resolution) => Ok(resolution),
            Err(_) => {
                // Sender dropped without sending — the pending entry
                // may still be there if the TUI exited without
                // resolving. Best-effort cleanup.
                self.pending.write().await.remove(&request.id);
                Err("Permission response channel closed".into())
            }
        }
    }
}

/// Called by tui_runner when `UserCommand::ApprovalResponse` arrives.
/// Pops the matching oneshot and sends the resolution. Returns `true`
/// when the request_id matched a pending entry, `false` otherwise
/// (stale response after the bridge dropped the sender).
pub async fn resolve_pending(
    pending: &PendingApprovals,
    request_id: &str,
    approved: bool,
    feedback: Option<String>,
) -> bool {
    let sender = {
        let mut map = pending.write().await;
        map.remove(request_id)
    };
    let Some(tx) = sender else {
        warn!(%request_id, "ApprovalResponse for unknown request_id (stale or already resolved)");
        return false;
    };
    let resolution = ToolPermissionResolution {
        decision: if approved {
            ToolPermissionDecision::Approved
        } else {
            ToolPermissionDecision::Rejected
        },
        feedback,
    };
    tx.send(resolution).is_ok()
}

#[cfg(test)]
#[path = "tui_permission_bridge.test.rs"]
mod tests;
