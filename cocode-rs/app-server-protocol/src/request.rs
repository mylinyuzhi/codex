//! Request/response types for bidirectional communication.
//!
//! - `ClientRequest`: client → server (session lifecycle, turn control, approvals).
//! - `ServerRequest`: server → client (permission prompts, user input, MCP routing).

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

// ===========================================================================
// Client → Server
// ===========================================================================

/// Requests from a client to the server.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "camelCase")]
pub enum ClientRequest {
    /// Start a new session.
    #[serde(rename = "session/start")]
    SessionStart(Box<SessionStartRequestParams>),
    /// Resume an existing session.
    #[serde(rename = "session/resume")]
    SessionResume(SessionResumeRequestParams),
    /// Start a new turn with user input.
    #[serde(rename = "turn/start")]
    TurnStart(TurnStartRequestParams),
    /// Interrupt the current turn.
    #[serde(rename = "turn/interrupt")]
    TurnInterrupt(TurnInterruptRequestParams),
    /// Resolve a pending approval request.
    #[serde(rename = "approval/resolve")]
    ApprovalResolve(ApprovalResolveRequestParams),
    /// Respond to a user input request (e.g., AskUserQuestion tool).
    #[serde(rename = "input/resolveUserInput")]
    UserInputResolve(UserInputResolveRequestParams),

    // ── Runtime control ─────────────────────────────────────────────
    /// Change the model during a session.
    #[serde(rename = "control/setModel")]
    SetModel(SetModelRequestParams),
    /// Change the permission mode during a session.
    #[serde(rename = "control/setPermissionMode")]
    SetPermissionMode(SetPermissionModeRequestParams),
    /// Stop a running background task.
    #[serde(rename = "control/stopTask")]
    StopTask(StopTaskRequestParams),
    /// Respond to an SDK hook callback.
    #[serde(rename = "hook/callbackResponse")]
    HookCallbackResponse(HookCallbackResponseParams),

    // ── Thinking and rewind ──────────────────────────────────────
    /// Change thinking configuration during a session.
    #[serde(rename = "control/setThinking")]
    SetThinking(SetThinkingRequestParams),
    /// Rewind files to a previous turn's state.
    #[serde(rename = "control/rewindFiles")]
    RewindFiles(RewindFilesRequestParams),

    // ── Environment and keepalive ───────────────────────────────
    /// Update environment variables during a session.
    #[serde(rename = "control/updateEnv")]
    UpdateEnv(UpdateEnvRequestParams),
    /// Keepalive signal (prevents idle timeouts).
    #[serde(rename = "control/keepAlive")]
    KeepAlive(KeepAliveRequestParams),
}

/// Parameters for `session/start`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionStartRequestParams {
    /// Initial prompt to send.
    pub prompt: String,
    /// Model override (e.g., "sonnet", "opus").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Maximum turns before stopping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,
    /// Working directory override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// System prompt suffix (appended to built-in system prompt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_suffix: Option<String>,
    /// Full system prompt override or structured config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<crate::SystemPromptConfig>,
    /// Permission mode override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// Environment variables to set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::HashMap<String, String>>,

    // ── Initialize fields (matching Claude Agent SDK pattern) ────────
    /// Custom sub-agent definitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<std::collections::HashMap<String, crate::AgentDefinitionConfig>>,
    /// MCP server configurations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<std::collections::HashMap<String, crate::McpServerConfig>>,
    /// Structured output JSON schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_format: Option<crate::OutputFormatConfig>,
    /// Sandbox configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<crate::SandboxConfig>,
    /// Thinking/reasoning configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<crate::ThinkingConfig>,
    /// Tool whitelist or preset configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<crate::ToolsConfig>,
    /// Permission rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_rules: Option<Vec<Value>>,
    /// Maximum budget in cents before stopping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_budget_cents: Option<i32>,
    /// SDK hook callbacks (pre-registered at session start).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Vec<crate::HookCallbackConfig>>,
    /// Disable all built-in agent definitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_builtin_agents: Option<bool>,
}

/// Parameters for `session/resume`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionResumeRequestParams {
    /// Session ID to resume.
    pub session_id: String,
    /// New prompt to send in the resumed session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// Parameters for `turn/start`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TurnStartRequestParams {
    /// User input text.
    pub text: String,
}

/// Parameters for `turn/interrupt`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TurnInterruptRequestParams {
    /// Turn ID to interrupt (if not specified, interrupts current turn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

/// Parameters for `approval/resolve`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalResolveRequestParams {
    /// The request ID of the approval being resolved.
    pub request_id: String,
    /// The decision.
    pub decision: ApprovalDecision,
}

/// Parameters for `input/resolveUserInput`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UserInputResolveRequestParams {
    /// The request ID of the input request being resolved.
    pub request_id: String,
    /// Response payload (structure depends on the original question).
    pub response: Value,
}

/// Parameters for `control/setModel`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetModelRequestParams {
    /// Model identifier (e.g., "sonnet", "opus", "haiku").
    pub model: String,
}

/// Parameters for `control/setPermissionMode`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetPermissionModeRequestParams {
    /// Permission mode (e.g., "default", "acceptEdits", "bypassPermissions").
    pub mode: String,
}

/// Parameters for `control/stopTask`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StopTaskRequestParams {
    /// Task ID to stop.
    pub task_id: String,
}

/// Decision on an approval request.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// Approve the tool execution.
    Approve,
    /// Approve for this session (don't ask again for this tool).
    ApproveSession,
    /// Deny the tool execution.
    Deny,
}

// ===========================================================================
// Server → Client
// ===========================================================================

/// Requests from the server to a client.
///
/// These require a response from the client (unlike notifications which are
/// fire-and-forget). Used for permission prompts, user input requests, and
/// MCP message routing.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "camelCase")]
pub enum ServerRequest {
    /// Ask the client to approve a tool execution.
    #[serde(rename = "approval/askForApproval")]
    AskForApproval(AskForApprovalParams),
    /// Ask the client for user input (e.g., AskUserQuestion tool).
    #[serde(rename = "input/requestUserInput")]
    RequestUserInput(RequestUserInputParams),
    /// Route an MCP message to a client-managed MCP server.
    #[serde(rename = "mcp/routeMessage")]
    McpRouteMessage(McpRouteMessageParams),
    /// Invoke an SDK-registered hook callback.
    #[serde(rename = "hook/callback")]
    HookCallback(HookCallbackParams),
}

/// Parameters for `approval/askForApproval`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AskForApprovalParams {
    /// Unique request identifier (for correlation with response).
    pub request_id: String,
    /// Tool name that needs approval.
    pub tool_name: String,
    /// Tool input (JSON).
    pub input: Value,
    /// Human-readable description of what the tool will do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Suggested approval behaviors the SDK client may present to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_suggestions: Option<Vec<PermissionSuggestion>>,
    /// Filesystem path that triggered a permission block (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_path: Option<String>,
    /// Reason for the permission decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
}

/// A permission suggestion that can be presented to the user.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PermissionSuggestion {
    /// Suggested behavior (e.g., "allow", "deny").
    pub behavior: String,
    /// Human-readable reason for the suggestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Parameters for `input/requestUserInput`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RequestUserInputParams {
    /// Unique request identifier.
    pub request_id: String,
    /// Human-readable message or question.
    pub message: String,
    /// Structured questions (JSON array of question objects).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questions: Option<Value>,
}

/// Parameters for `mcp/routeMessage`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McpRouteMessageParams {
    /// Unique request identifier.
    pub request_id: String,
    /// Target MCP server name.
    pub server_name: String,
    /// MCP message payload.
    pub message: Value,
}

/// Parameters for `hook/callback`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HookCallbackParams {
    /// Unique request identifier (for correlation with response).
    pub request_id: String,
    /// Callback identifier (pre-registered at initialize time).
    pub callback_id: String,
    /// Hook event type (e.g., "PreToolUse").
    pub event_type: String,
    /// Hook input payload (tool name, input, context).
    #[serde(default)]
    pub input: Value,
}

/// Parameters for `hook/callbackResponse`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HookCallbackResponseParams {
    /// The request_id from the corresponding HookCallback.
    pub request_id: String,
    /// Hook output payload.
    #[serde(default)]
    pub output: Value,
    /// Error message if the callback failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Parameters for `control/updateEnv`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateEnvRequestParams {
    /// Environment variables to set or update.
    pub env: std::collections::HashMap<String, String>,
}

/// Parameters for `control/setThinking`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetThinkingRequestParams {
    /// Thinking configuration to apply.
    pub thinking: crate::ThinkingConfig,
}

/// Parameters for `control/rewindFiles`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RewindFilesRequestParams {
    /// Turn ID to rewind files to.
    pub turn_id: String,
}

/// Parameters for `control/keepAlive`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KeepAliveRequestParams {
    /// Optional timestamp (milliseconds since epoch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}
