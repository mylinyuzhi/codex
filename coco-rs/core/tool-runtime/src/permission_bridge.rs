//! Permission bridge trait — async permission forwarding for teammate agents.
//!
//! TS: utils/swarm/inProcessRunner.ts createInProcessCanUseTool()
//!
//! When a teammate agent's tool needs approval, the request is forwarded
//! to the team leader via this bridge. The leader responds through the
//! mailbox, and the bridge completes the pending request.
//!
//! **Split design** (same pattern as SideQuery):
//! - Trait definition → here in `coco-tool`
//! - Implementation → `coco-state` (PermissionBridge struct)
//! - Consumer → tool execution layer (checks before running unsafe tools)

use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

/// A permission request from a teammate to the leader.
///
/// TS: SwarmPermissionRequest in utils/swarm/permissionSync.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissionRequest {
    /// Server-assigned correlation id for this approval request. Used by
    /// the approval bridge to match `approval/resolve` replies and by
    /// `control/cancelRequest` to cancel pending approvals. Fresh per
    /// request, decoupled from any tool invocation id.
    pub id: String,
    /// The model-assigned tool-invocation id (e.g. `toolu_01ABC...`) that
    /// this approval corresponds to. Matches TS
    /// `SDKControlPermissionRequestSchema.tool_use_id` — SDK clients use
    /// it to group the approval UI with the tool-call rendering.
    pub tool_use_id: String,
    /// Agent that needs permission.
    pub agent_id: String,
    /// Tool that needs approval.
    pub tool_name: String,
    /// Human-readable description of the action.
    pub description: String,
    /// Tool input as JSON.
    pub input: serde_json::Value,
}

/// Leader's decision on a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPermissionDecision {
    Approved,
    Rejected,
}

/// Resolution of a permission request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissionResolution {
    pub decision: ToolPermissionDecision,
    pub feedback: Option<String>,
}

/// Trait for forwarding permission requests from agents to the leader.
///
/// Implementations handle the async request/response lifecycle:
/// 1. Agent calls `request_permission()` — blocks until resolved
/// 2. Leader resolves via the mailbox or UI
/// 3. Implementation completes the pending request
///
/// TS: createInProcessCanUseTool() + PermissionBridge in inProcessRunner.ts
#[async_trait::async_trait]
pub trait ToolPermissionBridge: Send + Sync {
    /// Send a permission request and wait for the leader's response.
    async fn request_permission(
        &self,
        request: ToolPermissionRequest,
    ) -> Result<ToolPermissionResolution, String>;
}

/// Shared handle type.
pub type ToolPermissionBridgeRef = Arc<dyn ToolPermissionBridge>;

/// No-op implementation — rejects all requests. Used for main agent (no forwarding).
#[derive(Debug, Clone)]
pub struct NoOpPermissionBridge;

#[async_trait::async_trait]
impl ToolPermissionBridge for NoOpPermissionBridge {
    async fn request_permission(
        &self,
        _request: ToolPermissionRequest,
    ) -> Result<ToolPermissionResolution, String> {
        Ok(ToolPermissionResolution {
            decision: ToolPermissionDecision::Rejected,
            feedback: Some("Permission forwarding not available".into()),
        })
    }
}
