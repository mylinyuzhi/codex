//! `ToolPermissionBridge` implementation backed by the SDK server.
//!
//! When the agent hits a `PermissionDecision::Ask` during tool
//! execution, the engine calls `ctx.permission_bridge.request_permission`.
//! This module provides the bridge impl used in SDK mode: it issues a
//! `ServerRequest::AskForApproval` on the transport via
//! [`SdkServerState::send_server_request`] and translates the client's
//! `ApprovalResolveParams` reply back into [`ToolPermissionResolution`].
//!
//! TS reference: `canUseTool()` installed by `createStructuredIOQueryConfig`
//! in `src/cli/structuredIO.ts` — the SDK client is the ultimate authority
//! for any tool gated on `ask` semantics.
//!
//! See `event-system-design.md` §6.

use std::sync::Arc;

use async_trait::async_trait;
use coco_tool::ToolPermissionBridge;
use coco_tool::ToolPermissionDecision;
use coco_tool::ToolPermissionRequest;
use coco_tool::ToolPermissionResolution;
use coco_types::ApprovalDecision;
use coco_types::ApprovalResolveParams;
use coco_types::JsonRpcMessage;
use tracing::debug;
use tracing::warn;

use crate::sdk_server::handlers::SdkServerState;

/// Bridge the engine's permission gate to the SDK control protocol.
///
/// Holds a reference to the shared `SdkServerState` so it can:
/// - Read the cached transport handle
/// - Issue a `ServerRequest::AskForApproval` via `send_server_request`
/// - Translate the response into a `ToolPermissionResolution`
///
/// Construction is cheap — just an `Arc` clone. Install one per turn
/// on `QueryEngineConfig` / `ToolUseContext`.
pub struct SdkPermissionBridge {
    state: Arc<SdkServerState>,
}

impl SdkPermissionBridge {
    pub fn new(state: Arc<SdkServerState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl ToolPermissionBridge for SdkPermissionBridge {
    async fn request_permission(
        &self,
        request: ToolPermissionRequest,
    ) -> Result<ToolPermissionResolution, String> {
        // Read the transport handle the dispatcher published at startup.
        let transport = {
            let guard = self.state.transport.read().await;
            match guard.as_ref() {
                Some(t) => t.clone(),
                None => {
                    return Err(
                        "SdkPermissionBridge: transport not initialized; SdkServer::run() \
                         must be running before the bridge is consulted"
                            .into(),
                    );
                }
            }
        };

        // Build the `approval/askForApproval` params. Matches TS
        // `SDKControlPermissionRequestSchema` (controlSchemas.ts:108-121).
        let params = serde_json::json!({
            "request_id": request.id,
            "tool_name": request.tool_name,
            "input": request.input,
            "tool_use_id": request.id,
            "description": request.description,
            "agent_id": request.agent_id,
        });

        debug!(
            request_id = %request.id,
            tool = %request.tool_name,
            "SdkPermissionBridge: asking client for approval"
        );

        // Issue the outbound ServerRequest and await the reply.
        let reply = self
            .state
            .send_server_request(&transport, "approval/askForApproval", params)
            .await
            .map_err(|e| format!("send_server_request failed: {e}"))?;

        // Interpret the reply. TS clients reply with
        // `ApprovalResolveParams`-shaped payloads; we parse that.
        match reply {
            JsonRpcMessage::Response(r) => {
                let parsed: ApprovalResolveParams = serde_json::from_value(r.result.clone())
                    .map_err(|e| format!("invalid approval response: {e}"))?;
                let decision = match parsed.decision {
                    ApprovalDecision::Allow => ToolPermissionDecision::Approved,
                    ApprovalDecision::Deny => ToolPermissionDecision::Rejected,
                };
                Ok(ToolPermissionResolution {
                    decision,
                    feedback: parsed.feedback,
                })
            }
            JsonRpcMessage::Error(e) => {
                warn!(
                    request_id = %request.id,
                    code = e.code,
                    message = %e.message,
                    "SdkPermissionBridge: client returned error for approval ask"
                );
                // Client-side error on the approval path is treated as a
                // rejection — safer default than blocking execution.
                Ok(ToolPermissionResolution {
                    decision: ToolPermissionDecision::Rejected,
                    feedback: Some(format!("approval error: {}", e.message)),
                })
            }
            other => Err(format!(
                "unexpected reply variant for approval ask: {other:?}"
            )),
        }
    }
}

#[cfg(test)]
#[path = "approval_bridge.test.rs"]
mod tests;
