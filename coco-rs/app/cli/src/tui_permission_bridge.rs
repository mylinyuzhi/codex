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
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use coco_query::CoreEvent;
use coco_tool_runtime::{
    ToolPermissionBridge, ToolPermissionDecision, ToolPermissionRequest, ToolPermissionResolution,
};
use coco_types::TuiOnlyEvent;
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::warn;

use crate::session_runtime::SessionRuntime;

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
    /// Late-bound `Weak<SessionRuntime>` used to fire the
    /// `Notification` hook (TS `executeNotificationHooks`) when an
    /// `Ask` permission lands in front of the user. Set by
    /// [`Self::set_notification_runtime`] from `tui_runner` after
    /// `SessionRuntime::build` returns. Weak avoids extending the
    /// runtime's lifetime through the bridge.
    notification_runtime: RwLock<Option<Weak<SessionRuntime>>>,
}

impl TuiPermissionBridge {
    pub fn new(notification_tx: mpsc::Sender<CoreEvent>, pending: PendingApprovals) -> Self {
        Self {
            notification_tx,
            pending,
            notification_runtime: RwLock::new(None),
        }
    }

    /// Install the runtime weak-ref used to fire `Notification` hooks
    /// when prompting the user. Call once after `SessionRuntime::build`
    /// returns. Safe to skip — bridge degrades to no hook fire.
    pub async fn set_notification_runtime(&self, weak: Weak<SessionRuntime>) {
        *self.notification_runtime.write().await = Some(weak);
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

        // TS `useNotifyAfterTimeout('Claude Code is waiting for your input',
        // 'permission_prompt')` (`PermissionRequest.tsx:190`): fire the
        // Notification hook before the overlay is shown so user-defined
        // notifiers run in lockstep with TS. Best-effort — no runtime
        // installed (e.g. tests) leaves the hook unfired.
        if let Some(runtime) = self
            .notification_runtime
            .read()
            .await
            .as_ref()
            .and_then(Weak::upgrade)
        {
            let title = format!("Permission request: {}", request.tool_name);
            runtime
                .fire_notification_hooks(
                    "permission_prompt",
                    "Claude Code needs your permission to use a tool",
                    Some(&title),
                )
                .await;
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
///
/// `permission_updates` are forwarded into
/// [`ToolPermissionResolution::applied_updates`] so audit/logging
/// downstream of the bridge sees what the user authorized. Persistence
/// (settings.json writes) and live engine_config mutation are
/// performed by the consumer (`tui_runner::ApprovalResponse` arm)
/// before this fn is called — by the time the resolution lands on
/// the bridge the rules are already effective.
pub async fn resolve_pending(
    pending: &PendingApprovals,
    request_id: &str,
    approved: bool,
    feedback: Option<String>,
    permission_updates: Vec<coco_types::PermissionUpdate>,
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
        applied_updates: permission_updates,
    };
    tx.send(resolution).is_ok()
}

#[cfg(test)]
#[path = "tui_permission_bridge.test.rs"]
mod tests;
