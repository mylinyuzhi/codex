//! Approval flow handler for SDK mode.
//!
//! Routes approval requests to the client (via stdout) and collects
//! responses from stdin.

use cocode_app_server_protocol::ApprovalResolveRequestParams;
use cocode_app_server_protocol::AskForApprovalParams;
use cocode_app_server_protocol::ServerRequest;

/// Manages pending approval requests in SDK mode.
pub struct ApprovalHandler {
    next_request_id: i32,
}

impl ApprovalHandler {
    pub fn new() -> Self {
        Self { next_request_id: 0 }
    }

    /// Create a `ServerRequest::AskForApproval` from an internal approval request.
    pub fn create_approval_request(
        &mut self,
        request: &cocode_protocol::ApprovalRequest,
    ) -> ServerRequest {
        self.next_request_id += 1;
        let request_id = format!("approval_{}", self.next_request_id);

        ServerRequest::AskForApproval(AskForApprovalParams {
            request_id,
            tool_name: request.tool_name.clone(),
            input: serde_json::Value::Null,
            description: Some(request.description.clone()),
        })
    }

    /// Process an approval resolution from the client.
    ///
    /// In a full implementation, this would route the decision back to the
    /// agent loop via a oneshot channel. For now, it logs the resolution.
    pub fn resolve(&mut self, _resolve: &ApprovalResolveRequestParams) {
        // TODO: Route decision back to the agent loop's approval store.
        // This requires wiring the ApprovalStore from SessionState into
        // the SDK mode, which will be done when we extract the full
        // app-server crate.
        tracing::info!("approval resolved (routing not yet wired)");
    }
}
