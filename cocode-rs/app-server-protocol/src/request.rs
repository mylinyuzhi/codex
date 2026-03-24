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
    SessionStart(SessionStartRequestParams),
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
    /// Permission mode override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// Environment variables to set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::HashMap<String, String>>,
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
