//! SDK permission bridge for interactive approval flow.
//!
//! Implements [`PermissionRequester`] so the tool executor can block on
//! approval decisions from the SDK client. Each approval request emits a
//! `LoopEvent::ApprovalRequired` (picked up by the `select!` loop in mod.rs
//! to emit `ServerRequest::AskForApproval` on stdout), then blocks on a
//! oneshot channel until `resolve()` is called with the client's decision.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use cocode_app_server_protocol::AskForApprovalParams;
use cocode_app_server_protocol::ServerRequest;
use cocode_protocol::ApprovalDecision;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::LoopEvent;
use cocode_tools::PermissionRequester;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::debug;
use tracing::warn;

/// Bridge between the tool executor's approval requests and the SDK
/// client's NDJSON responses.
///
/// The tool executor calls `request_permission()` which:
/// 1. Emits `LoopEvent::ApprovalRequired` via `event_tx`
/// 2. Blocks on a oneshot channel
///
/// The SDK turn loop reads `LoopEvent::ApprovalRequired` from the event
/// channel, emits `ServerRequest::AskForApproval` to stdout, reads the
/// client's `ApprovalResolve` from stdin, and calls `resolve()` to
/// unblock the tool executor.
pub struct SdkPermissionBridge {
    /// Pending requests keyed by request_id → oneshot sender.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
    /// Event channel for emitting `LoopEvent::ApprovalRequired`.
    event_tx: mpsc::Sender<LoopEvent>,
}

impl SdkPermissionBridge {
    pub fn new(event_tx: mpsc::Sender<LoopEvent>) -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
        }
    }

    /// Convert an internal `ApprovalRequest` to a `ServerRequest` for
    /// emission on stdout.
    pub fn create_server_request(request: &ApprovalRequest) -> ServerRequest {
        ServerRequest::AskForApproval(AskForApprovalParams {
            request_id: request.request_id.clone(),
            tool_name: request.tool_name.clone(),
            input: request.input.clone().unwrap_or(serde_json::Value::Null),
            description: Some(request.description.clone()),
            permission_suggestions: None,
            blocked_path: None,
            decision_reason: None,
        })
    }

    /// Resolve a pending approval with the client's decision.
    ///
    /// Maps the protocol-level `ApprovalDecision` to the core-level enum
    /// and unblocks the waiting tool executor.
    pub async fn resolve(
        &self,
        request_id: &str,
        decision: &cocode_app_server_protocol::ApprovalDecision,
    ) {
        let core_decision = match decision {
            cocode_app_server_protocol::ApprovalDecision::Approve => ApprovalDecision::Approved,
            cocode_app_server_protocol::ApprovalDecision::ApproveSession => {
                ApprovalDecision::ApprovedWithPrefix {
                    prefix_pattern: "*".to_string(),
                }
            }
            cocode_app_server_protocol::ApprovalDecision::Deny => ApprovalDecision::Denied,
        };

        let tx = {
            let mut pending = self.pending.lock().await;
            pending.remove(request_id)
        };

        if let Some(tx) = tx {
            let _ = tx.send(core_decision);
            debug!(request_id, "Approval resolved via SDK bridge");
        } else {
            warn!(request_id, "Approval response for unknown request_id");
        }
    }
}

#[async_trait]
impl PermissionRequester for SdkPermissionBridge {
    async fn request_permission(
        &self,
        request: ApprovalRequest,
        _worker_id: &str,
    ) -> ApprovalDecision {
        let request_id = request.request_id.clone();
        let (tx, rx) = oneshot::channel();

        // Store the sender so resolve() can unblock us later
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        // Emit the event so the select! loop picks it up and writes the
        // ServerRequest to stdout
        if let Err(e) = self
            .event_tx
            .send(LoopEvent::ApprovalRequired {
                request: request.clone(),
            })
            .await
        {
            warn!("Failed to emit ApprovalRequired event: {e}");
            let mut pending = self.pending.lock().await;
            pending.remove(&request_id);
            return ApprovalDecision::Denied;
        }

        // Block until the SDK client responds (or channel closes)
        match rx.await {
            Ok(decision) => decision,
            Err(_) => {
                warn!(request_id, "Approval channel closed, denying");
                ApprovalDecision::Denied
            }
        }
    }
}
