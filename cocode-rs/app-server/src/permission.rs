//! SDK permission bridge for interactive approval flow.
//!
//! Implements [`PermissionRequester`] so the tool executor can block on
//! approval decisions from the client. Each approval request emits a
//! `CoreEvent::Tui(TuiEvent::ApprovalRequired)` (picked up by the event
//! loop to emit `ServerRequest::AskForApproval`), then blocks on a oneshot
//! channel until `resolve()` is called with the client's decision.
//!
//! Shared by both the CLI SDK mode and the app-server WebSocket mode.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use cocode_app_server_protocol::AskForApprovalParams;
use cocode_app_server_protocol::ServerRequest;
use cocode_protocol::ApprovalDecision;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::CoreEvent;
use cocode_protocol::TuiEvent;
use cocode_tools_api::PermissionRequester;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::debug;
use tracing::warn;

/// Bridge between the tool executor's approval requests and client responses.
pub struct SdkPermissionBridge {
    /// Pending requests keyed by request_id → oneshot sender.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
    /// Event channel for emitting `CoreEvent::Tui(TuiEvent::ApprovalRequired)`.
    event_tx: mpsc::Sender<CoreEvent>,
}

impl SdkPermissionBridge {
    pub fn new(event_tx: mpsc::Sender<CoreEvent>) -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
        }
    }

    /// Convert an internal `ApprovalRequest` to a `ServerRequest`.
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
            debug!(request_id, "Approval resolved");
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

        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        if let Err(e) = self
            .event_tx
            .send(CoreEvent::Tui(TuiEvent::ApprovalRequired {
                request: request.clone(),
            }))
            .await
        {
            warn!("Failed to emit ApprovalRequired event: {e}");
            let mut pending = self.pending.lock().await;
            pending.remove(&request_id);
            return ApprovalDecision::Denied;
        }

        match rx.await {
            Ok(decision) => decision,
            Err(_) => {
                warn!(request_id, "Approval channel closed, denying");
                ApprovalDecision::Denied
            }
        }
    }
}
