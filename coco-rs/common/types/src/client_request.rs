//! `ClientRequest` — SDK-to-agent protocol requests.
//!
//! TS source: `src/entrypoints/sdk/controlSchemas.ts` (21 control request
//! subtypes). coco-rs extends this to 29 variants: 22 from the cocode-rs
//! base + 7 P1 gap additions from the TS control protocol.
//!
//! See `event-system-design.md` §5.

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

use crate::HookEventType;
use crate::PermissionMode;
use crate::PermissionUpdate;
use crate::ThinkingLevel;

/// Bidirectional control protocol — client-initiated requests.
///
/// Each variant carries a unique `method` string used on the wire.
/// The method is the discriminator; params are the variant-specific payload.
///
/// See `event-system-design.md` §5.1 for the 22 base variants and §5.4 for
/// the 7 gap additions. 29 total.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum ClientRequest {
    // === Session lifecycle (6) ===
    #[serde(rename = "initialize")]
    Initialize(InitializeParams),
    #[serde(rename = "session/start")]
    SessionStart(Box<SessionStartParams>),
    #[serde(rename = "session/resume")]
    SessionResume(SessionResumeParams),
    #[serde(rename = "session/list")]
    SessionList,
    #[serde(rename = "session/read")]
    SessionRead(SessionReadParams),
    #[serde(rename = "session/archive")]
    SessionArchive(SessionArchiveParams),

    // === Turn control (2) ===
    #[serde(rename = "turn/start")]
    TurnStart(TurnStartParams),
    #[serde(rename = "turn/interrupt")]
    TurnInterrupt,

    // === Approval + user input resolution (3) ===
    #[serde(rename = "approval/resolve")]
    ApprovalResolve(ApprovalResolveParams),
    #[serde(rename = "input/resolveUserInput")]
    UserInputResolve(UserInputResolveParams),
    /// Resolve a pending MCP elicitation request. Counterpart to the
    /// `ServerRequest` the agent sends when an MCP server needs
    /// structured user input (form values, OAuth tokens, etc.).
    ///
    /// TS: `SDKControlElicitationRequestSchema` — documented as a
    /// planned addition in `event-system-design.md` §5.4.
    #[serde(rename = "elicitation/resolve")]
    ElicitationResolve(ElicitationResolveParams),

    // === Runtime control (8) ===
    #[serde(rename = "control/setModel")]
    SetModel(SetModelParams),
    #[serde(rename = "control/setPermissionMode")]
    SetPermissionMode(SetPermissionModeParams),
    #[serde(rename = "control/setThinking")]
    SetThinking(SetThinkingParams),
    #[serde(rename = "control/stopTask")]
    StopTask(StopTaskParams),
    #[serde(rename = "control/rewindFiles")]
    RewindFiles(RewindFilesParams),
    #[serde(rename = "control/updateEnv")]
    UpdateEnv(UpdateEnvParams),
    #[serde(rename = "control/keepAlive")]
    KeepAlive,
    #[serde(rename = "control/cancelRequest")]
    CancelRequest(CancelRequestParams),

    // === Config (2) ===
    #[serde(rename = "config/read")]
    ConfigRead,
    #[serde(rename = "config/value/write")]
    ConfigWrite(ConfigWriteParams),

    // === Hook + MCP message routing responses (2) ===
    #[serde(rename = "hook/callbackResponse")]
    HookCallbackResponse(HookCallbackResponseParams),
    #[serde(rename = "mcp/routeMessageResponse")]
    McpRouteMessageResponse(McpRouteMessageResponseParams),

    // === TS P1 gap additions (7) — event-system-design §5.4 ===
    /// Query MCP server connection status.
    /// TS: `SDKControlMcpStatusRequestSchema`
    #[serde(rename = "mcp/status")]
    McpStatus,

    /// Get context window usage breakdown.
    /// TS: `SDKControlGetContextUsageRequestSchema`
    #[serde(rename = "context/usage")]
    ContextUsage,

    /// Hot-reload MCP server configurations.
    /// TS: `SDKControlMcpSetServersRequestSchema`
    #[serde(rename = "mcp/setServers")]
    McpSetServers(McpSetServersParams),

    /// Reconnect a specific MCP server.
    /// TS: `SDKControlMcpReconnectRequestSchema`
    #[serde(rename = "mcp/reconnect")]
    McpReconnect(McpReconnectParams),

    /// Enable/disable a specific MCP server.
    /// TS: `SDKControlMcpToggleRequestSchema`
    #[serde(rename = "mcp/toggle")]
    McpToggle(McpToggleParams),

    /// Reload all plugins from disk.
    /// TS: `SDKControlReloadPluginsRequestSchema`
    #[serde(rename = "plugin/reload")]
    PluginReload,

    /// Apply feature flag settings at runtime.
    /// TS: `SDKControlApplyFlagSettingsRequestSchema`
    #[serde(rename = "config/applyFlags")]
    ConfigApplyFlags(ConfigApplyFlagsParams),
}

// ---------------------------------------------------------------------------
// Param structs (alphabetized by variant)
// ---------------------------------------------------------------------------

/// Matches TS `SDKControlInitializeRequestSchema` (controlSchemas.ts:57-71).
///
/// Sent once at session start for capability negotiation. Carries hooks,
/// SDK MCP servers, output format, system prompt, and agent definitions
/// so the agent can construct its registries before the first turn.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InitializeParams {
    /// Hook callbacks keyed by event type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HashMap<HookEventType, Vec<HookCallbackMatcher>>>,
    /// SDK-provided MCP server names (to skip env-configured ones).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_mcp_servers: Option<Vec<String>>,
    /// JSON schema for structured output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<serde_json::Value>,
    /// Full system prompt override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Text appended to the default system prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_system_prompt: Option<String>,
    /// Custom agent definitions keyed by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<HashMap<String, serde_json::Value>>,
    /// Enable prompt suggestions in the output stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_suggestions: Option<bool>,
    /// Enable agent progress summaries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_progress_summaries: Option<bool>,
}

/// Matches TS `SDKHookCallbackMatcherSchema` (controlSchemas.ts:43-51).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookCallbackMatcher {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    pub hook_callback_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i64>,
}

/// Params for `session/start`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStartParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_system_prompt: Option<String>,
    /// Optional initial user prompt to run immediately after start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_prompt: Option<String>,
}

/// Params for `session/resume`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResumeParams {
    pub session_id: String,
}

/// Params for `session/read`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReadParams {
    pub session_id: String,
    /// Optional pagination cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
}

/// Params for `session/archive`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionArchiveParams {
    pub session_id: String,
}

/// Params for `turn/start`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnStartParams {
    pub prompt: String,
    /// Optional turn-scoped permission mode override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    /// Optional turn-scoped thinking override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

/// Matches TS `SDKControlPermissionRequestSchema` response shape flipped —
/// here the SDK is *resolving* an approval request, so it's sent
/// client→server.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResolveParams {
    pub request_id: String,
    pub decision: ApprovalDecision,
    /// Optional permission update to persist to rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_update: Option<PermissionUpdate>,
    /// Optional feedback to inject back to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    /// Optional modified tool input (for pre-approval mutation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
}

/// TS uses `allow` / `deny` / `ask` for the canUseTool response flow.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Allow,
    Deny,
}

/// Params for `input/resolveUserInput`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputResolveParams {
    pub request_id: String,
    /// The user's answer to the `AskUserQuestion` prompt.
    pub answer: String,
}

/// Params for `elicitation/resolve`.
///
/// Sent client→server in response to a prior `ServerRequest` that
/// asked the client to collect structured input on behalf of an MCP
/// server (form values, OAuth tokens, etc.). The client populates
/// `values` with the user's input and sets `approved=true`, or sets
/// `approved=false` to reject the elicitation.
///
/// TS reference: `SDKControlElicitationRequestSchema` (controlSchemas.ts)
/// — TS uses a single bidirectional message that carries both request
/// and response shapes; coco-rs splits them into a `ServerRequest` for
/// the ask and this params struct for the reply.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationResolveParams {
    /// Correlation id matching the `ServerRequest` the agent sent.
    pub request_id: String,
    /// Which MCP server the elicitation originated from. Echoed back
    /// so the agent can route the resolution to the right connection.
    pub mcp_server_name: String,
    /// Whether the user approved the elicitation. If `false`, `values`
    /// is ignored and the MCP server sees a rejection.
    pub approved: bool,
    /// The collected field values keyed by field name. Each value is
    /// an opaque JSON payload so typed/untyped fields share the wire
    /// format. Empty when `approved=false`.
    #[serde(default)]
    pub values: std::collections::HashMap<String, serde_json::Value>,
}

/// Matches TS `SDKControlSetModelRequestSchema` (controlSchemas.ts:140-143).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SetModelParams {
    /// None means revert to the default model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Matches TS `SDKControlSetPermissionModeRequestSchema` (controlSchemas.ts:127-134).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetPermissionModeParams {
    pub mode: PermissionMode,
    /// TS `ultraplan` — enables ultraplan mode within Plan (feature flag).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ultraplan: Option<bool>,
}

/// Matches TS `SDKControlSetMaxThinkingTokensRequestSchema` + ThinkingConfig.
/// TS only carries `max_thinking_tokens: number | null`; coco-rs uses the
/// richer `ThinkingLevel` from coco-types which includes effort level and
/// per-provider options.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetThinkingParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

/// Matches TS `SDKControlStopTaskRequestSchema` (controlSchemas.ts:458-461).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopTaskParams {
    pub task_id: String,
}

/// Matches TS `SDKControlRewindFilesRequestSchema` (controlSchemas.ts:311-315).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindFilesParams {
    pub user_message_id: String,
    #[serde(default)]
    pub dry_run: bool,
}

/// Params for `control/updateEnv`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEnvParams {
    pub env: HashMap<String, String>,
}

/// Params for `control/cancelRequest`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelRequestParams {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Params for `config/value/write`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigWriteParams {
    pub key: String,
    pub value: serde_json::Value,
    /// Optional scope: "user" | "project" | "local".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Matches TS `SDKHookCallbackRequestSchema` response direction flipped —
/// client→server reply to a prior `hook/callback` ServerRequest.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookCallbackResponseParams {
    pub callback_id: String,
    /// Hook output (stdout/stderr + optional behavior field).
    pub output: serde_json::Value,
}

/// Matches TS `SDKControlMcpMessageRequestSchema` response direction —
/// client→server reply to a prior `mcp/routeMessage` ServerRequest.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRouteMessageResponseParams {
    pub request_id: String,
    /// JSON-RPC message response from the SDK-hosted MCP server.
    pub message: serde_json::Value,
}

// --- TS gap additions (7) ---

/// Matches TS `SDKControlMcpSetServersRequestSchema` (controlSchemas.ts:387-390).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSetServersParams {
    /// Server configs keyed by name.
    pub servers: HashMap<String, serde_json::Value>,
}

/// Matches TS `SDKControlMcpReconnectRequestSchema` (controlSchemas.ts:438-441).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpReconnectParams {
    pub server_name: String,
}

/// Matches TS `SDKControlMcpToggleRequestSchema` (controlSchemas.ts:447-451).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToggleParams {
    pub server_name: String,
    pub enabled: bool,
}

/// Matches TS `SDKControlApplyFlagSettingsRequestSchema` (controlSchemas.ts:467-472).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigApplyFlagsParams {
    pub settings: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
#[path = "client_request.test.rs"]
mod tests;
