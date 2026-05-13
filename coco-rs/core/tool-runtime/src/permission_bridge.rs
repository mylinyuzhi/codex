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
    /// Optional multi-choice payload propagated from
    /// `PermissionDecision::Ask.choices`. When `Some`, the TUI / SDK
    /// client should render a choice list rather than yes/no; the picked
    /// `value` is echoed back via `ToolPermissionResolution.updated_input`
    /// as `{ ..originalInput, user_choice: "<value>" }` so the tool's
    /// `execute()` can branch on the selection.
    ///
    /// TS parity: `ExitPlanModePermissionRequest.tsx:691-704` option grid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<coco_types::PermissionAskChoice>>,
}

/// Leader's decision on a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolPermissionDecision {
    Approved,
    #[default]
    Rejected,
}

/// Resolution of a permission request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolPermissionResolution {
    pub decision: ToolPermissionDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    /// Permission updates the user authorized at decision time.
    /// The bridge consumer (TUI / SDK runner) is expected to have
    /// already applied these to the live engine config and persisted
    /// them to disk for User/Project/Local destinations. This field
    /// carries the *intent* through the resolution so audit/logging
    /// downstream of the bridge can see which rules a user agreed to.
    /// Empty when the user picked one-shot Approve / Reject without
    /// "Always Allow".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applied_updates: Vec<coco_types::PermissionUpdate>,
    /// Optional rewritten tool input the user supplied at approval
    /// time. When `Some`, downstream
    /// (`PermissionOutcome::Allow.updated_input` →
    /// `tool_call_preparer::resolve_effective_input_from_permission`)
    /// substitutes this for the original input before invoking the
    /// tool. Used by `AskUserQuestion` to splice user-selected
    /// `answers` (and optional `annotations`) into the tool's data
    /// envelope so `render_for_model` can produce the answered prose.
    /// TS parity: `permissionDecision.updatedInput` at
    /// `services/tools/toolExecution.ts:1130-1131`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
    /// Optional content blocks (image attachments etc.) the user
    /// supplied alongside the approval. Mirrors TS `PermissionAllowDecision.contentBlocks`
    /// (`types/permissions.ts:183`) — typically populated when the user
    /// pasted an image while answering `AskUserQuestion`. Consumers
    /// (e.g. the engine's tool-execution path) attach these to the
    /// next user message in the conversation. Carried as
    /// `Vec<serde_json::Value>` because the wire shape is Anthropic
    /// `ContentBlockParam`; the cross-provider translation happens at
    /// the consumer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_blocks: Option<Vec<serde_json::Value>>,
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
            applied_updates: Vec::new(),
            updated_input: None,
            content_blocks: None,
        })
    }
}
