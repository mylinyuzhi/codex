//! MCP-based permission prompt tool routing.
//!
//! When `permission_prompt_tool` is configured, permission decisions are
//! first routed to the named MCP tool via the SDK MCP bridge. If the tool
//! is unavailable or returns an error, the request falls back to the
//! default `SdkPermissionBridge` (interactive approval via the SDK client).

use std::sync::Arc;

use async_trait::async_trait;
use cocode_protocol::ApprovalDecision;
use cocode_protocol::ApprovalRequest;
use cocode_tools_api::PermissionRequester;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::debug;
use tracing::warn;

use super::control::SdkPermissionBridge;

/// Info about a pending MCP permission request (consumed by the turn loop).
pub struct McpPermissionRequestInfo {
    pub request_id: String,
    pub server_name: String,
    pub tool_name: String,
    pub arguments: Value,
}

/// Routes permission decisions through an MCP tool with SDK bridge fallback.
pub struct McpPermissionRequester {
    server_name: String,
    tool_name: String,
    pending: Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<Value>>>>,
    request_tx: mpsc::Sender<McpPermissionRequestInfo>,
    request_rx: tokio::sync::Mutex<mpsc::Receiver<McpPermissionRequestInfo>>,
    fallback: Arc<SdkPermissionBridge>,
}

impl McpPermissionRequester {
    /// Create a new MCP permission requester.
    ///
    /// `tool_spec` is the MCP tool name, optionally prefixed with server name
    /// as `"server/tool"`. If no `/` separator is found, the tool name is used
    /// as both server and tool name.
    pub fn new(tool_spec: &str, fallback: Arc<SdkPermissionBridge>) -> Self {
        let (server_name, tool_name) = if let Some((server, tool)) = tool_spec.split_once('/') {
            (server.to_string(), tool.to_string())
        } else {
            (tool_spec.to_string(), tool_spec.to_string())
        };

        let (tx, rx) = mpsc::channel(16);
        Self {
            server_name,
            tool_name,
            pending: Arc::new(Mutex::new(std::collections::HashMap::new())),
            request_tx: tx,
            request_rx: tokio::sync::Mutex::new(rx),
            fallback,
        }
    }

    /// Receive the next pending MCP permission request (for the turn loop).
    pub async fn recv_request(&self) -> Option<McpPermissionRequestInfo> {
        self.request_rx.lock().await.recv().await
    }

    /// Try to resolve a pending MCP permission request.
    ///
    /// Returns `Ok(())` if the request was found and resolved, or
    /// `Err(response)` with the original value if the request_id was unknown.
    pub async fn try_resolve(&self, request_id: &str, response: Value) -> Result<(), Value> {
        let tx = {
            let mut pending = self.pending.lock().await;
            pending.remove(request_id)
        };
        if let Some(tx) = tx {
            let _ = tx.send(response);
            Ok(())
        } else {
            Err(response)
        }
    }

    /// Drain all pending MCP permission requests on turn end.
    pub async fn drain_pending(&self) {
        let mut pending = self.pending.lock().await;
        for (id, tx) in pending.drain() {
            debug!(
                request_id = id,
                "Draining pending MCP permission on turn end"
            );
            let _ = tx.send(serde_json::json!({"error": "turn ended"}));
        }
    }

    /// Parse the MCP tool response into an approval decision.
    ///
    /// Matches Claude Code's permission prompt tool response format:
    /// `{ "behavior": "allow"|"deny"|"ask", "message": "...", "interrupt": bool }`
    ///
    /// The response may be wrapped in MCP tool result format with a `content`
    /// array containing text blocks, or may be a direct JSON object.
    fn parse_response(response: &Value) -> Option<ApprovalDecision> {
        // Check for error response
        if response.get("error").is_some() {
            return None;
        }

        // Unwrap MCP result wrapper if present
        let result = response.get("result").unwrap_or(response);

        // Direct "behavior" field (primary format)
        if let Some(decision) = Self::extract_behavior(result) {
            return Some(decision);
        }

        // MCP tool result with content array containing text blocks
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            for item in content {
                if let Some(text) = item.get("text").and_then(|t| t.as_str())
                    && let Ok(parsed) = serde_json::from_str::<Value>(text)
                    && let Some(decision) = Self::extract_behavior(&parsed)
                {
                    return Some(decision);
                }
            }
        }

        None
    }

    /// Extract an approval decision from a JSON object's "behavior" field.
    fn extract_behavior(obj: &Value) -> Option<ApprovalDecision> {
        let behavior = obj.get("behavior").and_then(|b| b.as_str())?;
        match behavior {
            "allow" => Some(ApprovalDecision::Approved),
            "deny" => Some(ApprovalDecision::Denied),
            // "ask" means the tool cannot decide — fall back to SDK bridge
            "ask" => None,
            _ => None,
        }
    }
}

#[async_trait]
impl PermissionRequester for McpPermissionRequester {
    async fn request_permission(
        &self,
        request: ApprovalRequest,
        worker_id: &str,
    ) -> ApprovalDecision {
        let request_id = uuid::Uuid::new_v4().to_string();
        let arguments = serde_json::json!({
            "tool_name": request.tool_name,
            "input": request.input,
            "description": request.description,
        });

        let (resp_tx, resp_rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id.clone(), resp_tx);
        }

        let info = McpPermissionRequestInfo {
            request_id: request_id.clone(),
            server_name: self.server_name.clone(),
            tool_name: self.tool_name.clone(),
            arguments,
        };

        if let Err(e) = self.request_tx.send(info).await {
            warn!("Failed to send MCP permission request: {e}");
            let mut pending = self.pending.lock().await;
            pending.remove(&request_id);
            return self.fallback.request_permission(request, worker_id).await;
        }

        // Wait for MCP tool response with timeout
        let response = match tokio::time::timeout(std::time::Duration::from_secs(30), resp_rx).await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => {
                warn!("MCP permission channel closed, falling back to SDK bridge");
                return self.fallback.request_permission(request, worker_id).await;
            }
            Err(_) => {
                warn!("MCP permission tool timed out, falling back to SDK bridge");
                let mut pending = self.pending.lock().await;
                pending.remove(&request_id);
                return self.fallback.request_permission(request, worker_id).await;
            }
        };

        // Parse response, fallback on failure
        match Self::parse_response(&response) {
            Some(decision) => {
                debug!(
                    tool = %self.tool_name,
                    ?decision,
                    "MCP permission tool decided"
                );
                decision
            }
            None => {
                debug!("MCP permission tool returned unparseable response, falling back");
                self.fallback.request_permission(request, worker_id).await
            }
        }
    }
}

#[cfg(test)]
#[path = "permission_prompt.test.rs"]
mod tests;
