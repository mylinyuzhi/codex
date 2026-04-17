//! `ServerRequest` — agent-to-SDK protocol requests requiring responses.
//!
//! TS source: `src/entrypoints/sdk/controlSchemas.ts` — these match the
//! TS `SDKControl*Request` types that flow agent→SDK (the reverse direction
//! of `ClientRequest`).
//!
//! See `event-system-design.md` §5.2.

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

/// Bidirectional control protocol — server-initiated requests.
///
/// The agent sends these to SDK clients when it needs a decision or input
/// (permission approval, user question, hook callback, MCP routing). The
/// SDK client must reply via the corresponding `ClientRequest` variant.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum ServerRequest {
    /// Ask the SDK client to approve or deny a tool use.
    /// Matches TS `SDKControlPermissionRequestSchema` (controlSchemas.ts:108-121).
    /// Expected response: `ClientRequest::ApprovalResolve`.
    #[serde(rename = "approval/askForApproval")]
    AskForApproval(AskForApprovalParams),

    /// Ask the user a question via the SDK client (e.g. multiple choice).
    /// Expected response: `ClientRequest::UserInputResolve`.
    #[serde(rename = "input/requestUserInput")]
    RequestUserInput(RequestUserInputParams),

    /// Route an MCP JSON-RPC message to the SDK-hosted MCP server.
    /// Matches TS `SDKControlMcpMessageRequestSchema` (controlSchemas.ts:377-381).
    /// Expected response: `ClientRequest::McpRouteMessageResponse`.
    #[serde(rename = "mcp/routeMessage")]
    McpRouteMessage(McpRouteMessageParams),

    /// Invoke an SDK-registered hook callback.
    /// Matches TS `SDKHookCallbackRequestSchema` (controlSchemas.ts:366-371).
    /// Expected response: `ClientRequest::HookCallbackResponse`.
    #[serde(rename = "hook/callback")]
    HookCallback(HookCallbackParams),

    /// Notify the SDK that a previously-sent ServerRequest should be cancelled.
    #[serde(rename = "control/cancelRequest")]
    CancelRequest(ServerCancelRequestParams),
}

// ---------------------------------------------------------------------------
// Param structs
// ---------------------------------------------------------------------------

/// Matches TS `SDKControlPermissionRequestSchema` (controlSchemas.ts:108-121).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskForApprovalParams {
    /// Unique correlation id (SDK must echo in `ApprovalResolve`).
    pub request_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub tool_use_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Suggested permission updates the SDK can present to the user.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_suggestions: Vec<serde_json::Value>,
}

/// Ask the SDK to request user input (free-form or choice-list).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestUserInputParams {
    pub request_id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional choice list; if present, the SDK should render a picker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Route an MCP JSON-RPC message to an SDK-hosted server.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRouteMessageParams {
    pub request_id: String,
    pub server_name: String,
    /// The raw JSON-RPC message to forward.
    pub message: serde_json::Value,
}

/// Invoke an SDK-registered hook callback.
/// Matches TS `SDKHookCallbackRequestSchema` (controlSchemas.ts:366-371).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookCallbackParams {
    pub request_id: String,
    pub callback_id: String,
    /// Hook input payload (event-specific shape).
    pub input: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
}

/// Cancel a previously-sent ServerRequest.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCancelRequestParams {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Response types (for the success path of each request)
// ---------------------------------------------------------------------------

/// Aggregate response to `ClientRequest::ConfigRead`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigReadResult {
    /// Merged effective config as a JSON object.
    pub config: serde_json::Value,
    /// Per-source settings keyed by source name ("user", "project", "local",
    /// "flags", "policy").
    #[serde(default)]
    pub sources: HashMap<String, serde_json::Value>,
}

/// Response to `ClientRequest::McpStatus`.
/// Matches TS `SDKControlMcpStatusResponseSchema` (controlSchemas.ts:165-173).
///
/// The `mcpServers` field is camelCase on the wire to match the TS
/// zod schema. Internal Rust uses snake_case for the field name.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStatusResult {
    #[serde(rename = "mcpServers")]
    pub mcp_servers: Vec<McpServerStatus>,
}

/// MCP server connection state on the wire.
///
/// Matches the TS `McpServerStatusSchema` enum at `coreSchemas.ts:167-173`:
/// `'connected' | 'failed' | 'needs-auth' | 'pending' | 'disabled'`.
/// `Disconnected` is a coco-rs extension used when the connection manager
/// has no record of a named server.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpConnectionStatus {
    Connected,
    Pending,
    Failed,
    NeedsAuth,
    Disabled,
    /// coco-rs extension: server name unknown to the connection manager.
    Disconnected,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub status: McpConnectionStatus,
    #[serde(default)]
    pub tool_count: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response to `ClientRequest::ContextUsage`.
/// Matches TS `SDKControlGetContextUsageResponseSchema` (controlSchemas.ts:205-306).
/// Simplified subset — TS includes a rich breakdown grid that's UI-specific.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUsageResult {
    pub total_tokens: i64,
    pub max_tokens: i64,
    pub raw_max_tokens: i64,
    pub percentage: f64,
    pub model: String,
    /// Categorized breakdown (system prompt, tools, history, etc.).
    pub categories: Vec<ContextUsageCategory>,
    pub is_auto_compact_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_threshold: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_breakdown: Option<MessageBreakdown>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUsageCategory {
    pub name: String,
    pub tokens: i64,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBreakdown {
    pub tool_call_tokens: i64,
    pub tool_result_tokens: i64,
    pub attachment_tokens: i64,
    pub assistant_message_tokens: i64,
    pub user_message_tokens: i64,
}

/// Response to `ClientRequest::McpSetServers`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpSetServersResult {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub errors: HashMap<String, String>,
}

/// Response to `ClientRequest::RewindFiles`.
///
/// Reports which files would be (or were) restored to the snapshot
/// keyed by `user_message_id`, plus a diff summary.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RewindFilesResult {
    /// Paths that were (or would be) restored. PathBuf serialized as
    /// strings for wire portability.
    #[serde(default)]
    pub files_changed: Vec<String>,
    /// Total lines that would be added by the rewind.
    #[serde(default)]
    pub insertions: i64,
    /// Total lines that would be removed by the rewind.
    #[serde(default)]
    pub deletions: i64,
    /// True if this was a dry-run preview (files were not actually
    /// modified). Echoed from the request.
    #[serde(default)]
    pub dry_run: bool,
}

/// Response to `ClientRequest::PluginReload`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginReloadResult {
    pub plugins: Vec<String>,
    pub commands: Vec<String>,
    pub agents: Vec<String>,
    pub error_count: i32,
}

/// Response to `ClientRequest::Initialize`.
///
/// Matches TS `SDKControlInitializeResponseSchema` (controlSchemas.ts:77-95).
/// Returned synchronously after the client sends `initialize`; gives the
/// client the full bootstrap context it needs before calling `session/start`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Slash commands the client can invoke.
    #[serde(default)]
    pub commands: Vec<SdkSlashCommand>,
    /// Subagents available for the `Agent` tool.
    #[serde(default)]
    pub agents: Vec<SdkAgentInfo>,
    /// Currently-selected output style (e.g. `"default"`, `"explanatory"`).
    pub output_style: String,
    /// All output styles the server knows about.
    #[serde(default)]
    pub available_output_styles: Vec<String>,
    /// Available models.
    #[serde(default)]
    pub models: Vec<SdkModelInfo>,
    /// Account / auth info for the logged-in user.
    #[serde(default)]
    pub account: SdkAccountInfo,
    /// Process PID — used by SDK clients for tmux socket isolation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Fast-mode feature state if enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_mode_state: Option<crate::event::FastModeState>,
    // coco-rs extensions (not in TS). TS parsers accept unknown fields by
    // default, so these pass through transparently. Prefixed with
    // `_cocoRs` so they're visually distinct from protocol fields.
    /// Protocol version the coco-rs server speaks.
    #[serde(default, rename = "_cocoRsProtocolVersion")]
    pub protocol_version: String,
    /// coco-rs binary version.
    #[serde(default, rename = "_cocoRsVersion")]
    pub version: String,
}

/// Slash command descriptor for `InitializeResult.commands`. Matches TS
/// `SlashCommandSchema` at `coreSchemas.ts:1016-1028`.
///
/// Named `SdkSlashCommand` to avoid colliding with the existing coco-rs
/// `commands` crate notion of a slash command (which has richer
/// internal fields not on the SDK wire).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkSlashCommand {
    /// Command name without the leading `/`.
    pub name: String,
    /// Description shown in help / completion UI.
    pub description: String,
    /// Argument hint rendered next to the command (e.g. `"<file>"`).
    #[serde(rename = "argumentHint")]
    pub argument_hint: String,
}

/// Available subagent descriptor for `InitializeResult.agents`. Matches
/// TS `AgentInfoSchema` at `coreSchemas.ts:1030-1045`.
///
/// Named `SdkAgentInfo` to avoid colliding with `event::AgentInfo`
/// (the payload for the `agents/registered` notification, which has a
/// different schema — `description: Option<String>` without `model`).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkAgentInfo {
    /// Agent type identifier (e.g. `"Explore"`).
    pub name: String,
    /// Description of when to use this agent.
    pub description: String,
    /// Model alias this agent uses; `None` means inherit parent model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Model capability descriptor for `InitializeResult.models`. Matches
/// TS `ModelInfoSchema` at `coreSchemas.ts:1047-1079`. The wire uses
/// `value` + camelCase capability keys.
///
/// Named `SdkModelInfo` to match the existing re-export name at the
/// crate root and to leave breathing room for other model-info shapes
/// (e.g. per-provider config models) elsewhere in the codebase.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkModelInfo {
    /// Model identifier used in API calls (e.g. `"claude-opus-4-6"`).
    pub value: String,
    /// Human-readable display name.
    #[serde(rename = "displayName")]
    pub display_name: String,
    /// Short description of the model's capabilities.
    pub description: String,
    #[serde(
        rename = "supportsEffort",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub supports_effort: Option<bool>,
    #[serde(
        rename = "supportedEffortLevels",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub supported_effort_levels: Vec<EffortLevel>,
    #[serde(
        rename = "supportsAdaptiveThinking",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub supports_adaptive_thinking: Option<bool>,
    #[serde(
        rename = "supportsFastMode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub supports_fast_mode: Option<bool>,
    #[serde(
        rename = "supportsAutoMode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub supports_auto_mode: Option<bool>,
}

/// Model effort tier. Matches TS enum `z.enum(['low','medium','high','max'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffortLevel {
    Low,
    Medium,
    High,
    Max,
}

/// Account + auth info for the logged-in user. Matches TS
/// `AccountInfoSchema` at `coreSchemas.ts:1081-1097`. All fields optional
/// — clients that don't sign in get an empty struct.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SdkAccountInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(
        rename = "subscriptionType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub subscription_type: Option<String>,
    #[serde(
        rename = "tokenSource",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub token_source: Option<String>,
    #[serde(
        rename = "apiKeySource",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub api_key_source: Option<String>,
    /// Active API backend. Anthropic OAuth login only applies when
    /// `FirstParty`; for third-party providers the other fields are
    /// absent and auth is external (AWS creds, gcloud ADC, etc.).
    #[serde(
        rename = "apiProvider",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub api_provider: Option<ApiProvider>,
}

/// Active API backend. Matches TS
/// `z.enum(['firstParty', 'bedrock', 'vertex', 'foundry'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApiProvider {
    FirstParty,
    Bedrock,
    Vertex,
    Foundry,
}

/// Minimal session metadata returned by `session/list` and `session/read`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SdkSessionSummary {
    pub session_id: String,
    pub model: String,
    pub cwd: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub message_count: i32,
    #[serde(default)]
    pub total_tokens: i64,
}

/// Response to `ClientRequest::SessionList`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionListResult {
    pub sessions: Vec<SdkSessionSummary>,
}

/// Response to `ClientRequest::SessionRead`.
///
/// Phase 2.C.11 returns session metadata only. Message-history
/// retrieval (via the JSONL transcript) is a future enhancement — the
/// `messages` / `next_cursor` / `has_more` fields are reserved for
/// when the transcript reader is wired.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionReadResult {
    pub session: SdkSessionSummary,
    /// Messages paginated by `cursor`/`limit` from the original
    /// request. Empty until the transcript reader lands.
    #[serde(default)]
    pub messages: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub has_more: bool,
}

/// Response to `ClientRequest::SessionResume`.
///
/// Returned after the server loads a previously-persisted session
/// from disk and installs it as the active session. The SDK client
/// can then issue `turn/start` to continue the conversation.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionResumeResult {
    pub session: SdkSessionSummary,
}

/// Response to `ClientRequest::SessionStart`.
///
/// Returned after the server creates an agent session and emits the
/// `session/started` notification. Subsequent ClientRequests
/// (turn/start, approval/resolve, etc.) operate on this session.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStartResult {
    pub session_id: String,
}

/// Response to `ClientRequest::TurnStart`.
///
/// `turn/start` is a fire-and-forget trigger — the server accepts the
/// request, spawns the turn as a detached task, and replies immediately
/// with a handle. Progress is delivered via `turn/started`, streaming
/// deltas, `turn/completed` / `turn/failed` notifications.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TurnStartResult {
    /// Opaque turn identifier the client can use to correlate notifications
    /// and issue `turn/interrupt` for cancellation.
    pub turn_id: String,
}

#[cfg(test)]
#[path = "server_request.test.rs"]
mod tests;
