//! `ClientRequest` — SDK-to-agent protocol requests.
//!
//! TS source: `src/entrypoints/sdk/controlSchemas.ts` (21 control request
//! subtypes). coco-rs extends this to 31 variants: 22 cocode-rs base +
//! `elicitation/resolve` (TS-aligned) + 8 P1 gap additions.
//!
//! See `event-system-design.md` §5.

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

use crate::HookEventType;
use crate::PermissionMode;
use crate::PermissionUpdate;
use crate::ThinkingLevel;
use crate::wire_tagged::wire_tagged_enum;

wire_tagged_enum! {
    method_enum = ClientRequestMethod,
    tagged_enum = ClientRequest,
    method_doc = "\
Wire-method identifier for every `ClientRequest` variant.\n\n\
Cross-language protocol constant exported to the JSON schema bundle so \
Python / other SDK codegens obtain the same vocabulary. Consumers should \
reference `ClientRequestMethod::SessionStart` rather than compare against \
raw wire strings.",
    tagged_doc = "\
Bidirectional control protocol — client-initiated requests.\n\n\
Each variant carries a unique `method` string used on the wire. \
The method is the discriminator; params are the variant-specific payload.\n\n\
See `event-system-design.md` §5.1 for the 22 base variants and §5.4 for \
the 8 gap additions (`elicitation/resolve` is TS-aligned). 31 total.",
    variants = {
        // === Session lifecycle (6) ===
        "initialize" => Initialize(InitializeParams),
        "session/start" => SessionStart(Box<SessionStartParams>),
        "session/resume" => SessionResume(SessionResumeParams),
        "session/list" => SessionList,
        "session/read" => SessionRead(SessionReadParams),
        "session/archive" => SessionArchive(SessionArchiveParams),

        // === Turn control (2) ===
        "turn/start" => TurnStart(TurnStartParams),
        "turn/interrupt" => TurnInterrupt,

        // === Approval + user input resolution (3) ===
        "approval/resolve" => ApprovalResolve(ApprovalResolveParams),
        "input/resolveUserInput" => UserInputResolve(UserInputResolveParams),
        /// Resolve a pending MCP elicitation request. Counterpart to the
        /// `ServerRequest` the agent sends when an MCP server needs
        /// structured user input (form values, OAuth tokens, etc.).
        ///
        /// TS: `SDKControlElicitationRequestSchema` — documented as a
        /// planned addition in `event-system-design.md` §5.4.
        "elicitation/resolve" => ElicitationResolve(ElicitationResolveParams),

        // === Runtime control (9) ===
        "control/setModel" => SetModel(SetModelParams),
        "control/setPermissionMode" => SetPermissionMode(SetPermissionModeParams),
        "control/setThinking" => SetThinking(SetThinkingParams),
        "control/stopTask" => StopTask(StopTaskParams),
        "control/rewindFiles" => RewindFiles(RewindFilesParams),
        "control/updateEnv" => UpdateEnv(UpdateEnvParams),
        "control/keepAlive" => KeepAlive,
        "control/cancelRequest" => CancelRequest(CancelRequestParams),
        /// Interrupt one in-process teammate's active turn without
        /// stopping the teammate lifecycle.
        "agent/interruptCurrentWork" => AgentInterruptCurrentWork(AgentInterruptCurrentWorkParams),

        // === Config (2) ===
        "config/read" => ConfigRead,
        "config/value/write" => ConfigWrite(ConfigWriteParams),

        // === Hook + MCP message routing responses (2) ===
        "hook/callbackResponse" => HookCallbackResponse(HookCallbackResponseParams),
        "mcp/routeMessageResponse" => McpRouteMessageResponse(McpRouteMessageResponseParams),

        // === TS P1 gap additions (7) — event-system-design §5.4 ===
        /// Query MCP server connection status.
        /// TS: `SDKControlMcpStatusRequestSchema`
        "mcp/status" => McpStatus,
        /// Get context window usage breakdown.
        /// TS: `SDKControlGetContextUsageRequestSchema`
        "context/usage" => ContextUsage,
        /// Hot-reload MCP server configurations.
        /// TS: `SDKControlMcpSetServersRequestSchema`
        "mcp/setServers" => McpSetServers(McpSetServersParams),
        /// Reconnect a specific MCP server.
        /// TS: `SDKControlMcpReconnectRequestSchema`
        "mcp/reconnect" => McpReconnect(McpReconnectParams),
        /// Enable/disable a specific MCP server.
        /// TS: `SDKControlMcpToggleRequestSchema`
        "mcp/toggle" => McpToggle(McpToggleParams),
        /// Reload all plugins from disk.
        /// TS: `SDKControlReloadPluginsRequestSchema`
        "plugin/reload" => PluginReload,
        /// Apply feature flag settings at runtime.
        /// TS: `SDKControlApplyFlagSettingsRequestSchema`
        "config/applyFlags" => ConfigApplyFlags(ConfigApplyFlagsParams),
    }
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
    /// Optional rewritten tool input the SDK client supplies at
    /// approval time. When `Some`, the engine substitutes this for the
    /// model-emitted input before invoking the tool. Used by
    /// `AskUserQuestion` to ship user-selected `answers` (and optional
    /// `annotations`) back into the tool's data envelope.
    ///
    /// Protocol mirror of `coco_tool_runtime::ToolPermissionResolution.updated_input`
    /// (the in-process equivalent for TUI mode). TS parity:
    /// `permissionDecision.updatedInput` at `services/tools/toolExecution.ts:1130-1131`.
    /// Consumed by `app/cli/src/sdk_server/approval_bridge.rs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
    /// Optional content blocks (typically image attachments) the SDK
    /// client wants attached to the next user message. Mirrors TS
    /// `contentBlocks?: ContentBlockParam[]` on `PermissionAllowDecision`
    /// (`types/permissions.ts:183`) — paste-image-during-AskUserQuestion
    /// or attachments alongside `MCPTool` answers ride this slot.
    /// Carried verbatim as `serde_json::Value` because the underlying
    /// `ContentBlockParam` is Anthropic-shaped; consumers translate
    /// per provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_blocks: Option<Vec<serde_json::Value>>,
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
///
/// TS carries an additional `ultraplan: boolean` field for the CCR web-UI
/// refinement flow. coco-rs intentionally skips Ultraplan (see CLAUDE.md
/// "Plan Mode — Skip Ultraplan (CCR Web UI) Only"), so that field is
/// omitted here — SDK clients targeting coco-rs should not send it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetPermissionModeParams {
    pub mode: PermissionMode,
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

/// Params for `agent/interruptCurrentWork`.
///
/// Mirrors TS's teammate Escape path: abort the target teammate's
/// current model/tool turn while keeping the teammate process alive for
/// later messages.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInterruptCurrentWorkParams {
    pub agent_id: String,
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
