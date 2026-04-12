//! SDK protocol types matching the SDK entry point schemas.
//!
//! TS: entrypoints/sdk/coreSchemas.ts, controlSchemas.ts, coreTypes.ts
//!
//! Defines request/response types for the SDK control protocol used
//! by SDK consumers (Python SDK, etc.) to communicate with the CLI
//! process via NDJSON over stdin/stdout.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Model Usage
// ---------------------------------------------------------------------------

/// Token usage and cost for a model turn.
///
/// TS: coreSchemas.ts — ModelUsageSchema.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    #[serde(default)]
    pub cache_read_input_tokens: i64,
    #[serde(default)]
    pub cache_creation_input_tokens: i64,
    #[serde(default)]
    pub web_search_requests: i64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub context_window: i64,
    #[serde(default)]
    pub max_output_tokens: i64,
}

// ---------------------------------------------------------------------------
// Thinking Configuration
// ---------------------------------------------------------------------------

/// Thinking mode configuration.
///
/// TS: coreSchemas.ts — ThinkingConfigSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThinkingConfig {
    /// Claude decides when and how much to think.
    Adaptive,
    /// Fixed thinking token budget.
    Enabled {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        budget_tokens: Option<i64>,
    },
    /// No extended thinking.
    Disabled,
}

// ---------------------------------------------------------------------------
// Permission Types
// ---------------------------------------------------------------------------

/// Permission mode for tool execution.
///
/// TS: coreSchemas.ts — PermissionModeSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Default,
    Auto,
    Plan,
}

/// Scope for permission updates.
///
/// TS: coreSchemas.ts — PermissionUpdateDestinationSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionScope {
    UserSettings,
    ProjectSettings,
    LocalSettings,
    Session,
    CliArg,
}

/// Permission update entry.
///
/// TS: coreSchemas.ts — PermissionUpdateSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionUpdate {
    pub tool_name: String,
    pub scope: PermissionScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

// ---------------------------------------------------------------------------
// SDK Request (from consumer to CLI)
// ---------------------------------------------------------------------------

/// SDK request envelope.
///
/// TS: controlSchemas.ts — SDKControlRequest discriminated union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum SdkRequest {
    /// Initialize SDK session.
    Initialize {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system_prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        append_system_prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        json_schema: Option<HashMap<String, serde_json::Value>>,
        #[serde(default)]
        prompt_suggestions: bool,
    },
    /// Interrupt the current conversation turn.
    Interrupt,
    /// Tool permission request from agent to SDK.
    CanUseTool {
        tool_name: String,
        tool_use_id: String,
        input: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        permission_suggestions: Vec<PermissionUpdate>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocked_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decision_reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
    },
    /// Set the model for subsequent turns.
    SetModel {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// Set permission mode.
    SetPermissionMode { mode: PermissionMode },
    /// Set maximum thinking tokens.
    SetMaxThinkingTokens { max_thinking_tokens: Option<i64> },
    /// Query MCP server status.
    McpStatus,
    /// Get context window usage breakdown.
    GetContextUsage,
    /// Rewind file changes to a specific message.
    RewindFiles {
        user_message_id: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// Cancel a pending async message.
    CancelAsyncMessage { message_uuid: String },
    /// Seeds the read-file-state cache.
    SeedReadState { path: String, mtime: i64 },
    /// Send a JSON-RPC message to an MCP server.
    McpMessage {
        server_name: String,
        message: serde_json::Value,
    },
    /// Replace the set of dynamically managed MCP servers.
    McpSetServers {
        servers: HashMap<String, serde_json::Value>,
    },
    /// Reconnect a disconnected MCP server.
    McpReconnect { server_name: String },
    /// Enable or disable an MCP server.
    McpToggle { server_name: String, enabled: bool },
    /// Reload plugins from disk.
    ReloadPlugins,
    /// Stop a running task.
    StopTask { task_id: String },
    /// Apply flag settings.
    ApplyFlagSettings {
        settings: HashMap<String, serde_json::Value>,
    },
    /// Get effective settings.
    GetSettings,
    /// Request user input (elicitation).
    Elicitation {
        mcp_server_name: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<ElicitationMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        elicitation_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        requested_schema: Option<HashMap<String, serde_json::Value>>,
    },
}

/// Elicitation mode.
///
/// TS: controlSchemas.ts — SDKControlElicitationRequest.mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElicitationMode {
    Form,
    Url,
}

// ---------------------------------------------------------------------------
// SDK Response (from CLI to consumer)
// ---------------------------------------------------------------------------

/// SDK response envelope.
///
/// TS: controlSchemas.ts — SDKControlResponse discriminated union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum SdkResponse {
    /// Successful response with payload.
    Success {
        request_id: String,
        #[serde(flatten)]
        response: serde_json::Value,
    },
    /// Error response.
    Error { request_id: String, error: String },
}

// ---------------------------------------------------------------------------
// Initialize Response
// ---------------------------------------------------------------------------

/// Response from SDK session initialization.
///
/// TS: controlSchemas.ts — SDKControlInitializeResponseSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResponse {
    pub commands: Vec<SlashCommand>,
    pub agents: Vec<AgentInfo>,
    pub output_style: String,
    pub available_output_styles: Vec<String>,
    pub models: Vec<ModelInfo>,
    pub account: AccountInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
}

/// Slash command descriptor.
///
/// TS: coreSchemas.ts — SlashCommandSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Agent information.
///
/// TS: coreSchemas.ts — AgentInfoSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Model information.
///
/// TS: coreSchemas.ts — ModelInfoSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// Account information.
///
/// TS: coreSchemas.ts — AccountInfoSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

// ---------------------------------------------------------------------------
// Permission Response
// ---------------------------------------------------------------------------

/// Permission response from SDK consumer.
///
/// TS: controlSchemas.ts — SDKControlPermissionResponse.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "snake_case")]
pub enum SdkPermissionResponse {
    /// Allow the tool use.
    Allow,
    /// Deny the tool use.
    Deny,
    /// Allow and add a permanent permission rule.
    AllowAlways {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<PermissionScope>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        updates: Vec<PermissionUpdate>,
    },
}

// ---------------------------------------------------------------------------
// Elicitation Response
// ---------------------------------------------------------------------------

/// Elicitation response from SDK consumer.
///
/// TS: controlSchemas.ts — SDKControlElicitationResponseSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationResponse {
    pub action: ElicitationAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<HashMap<String, serde_json::Value>>,
}

/// Elicitation action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElicitationAction {
    Accept,
    Decline,
    Cancel,
}

// ---------------------------------------------------------------------------
// Hook Events
// ---------------------------------------------------------------------------

/// Hook event types (matching the TS HOOK_EVENTS const array).
///
/// TS: coreTypes.ts — HOOK_EVENTS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    Notification,
    UserPromptSubmit,
    SessionStart,
    SessionEnd,
    Stop,
    StopFailure,
    SubagentStart,
    SubagentStop,
    PreCompact,
    PostCompact,
    PermissionRequest,
    PermissionDenied,
    Setup,
    TeammateIdle,
    TaskCreated,
    TaskCompleted,
    Elicitation,
    ElicitationResult,
    ConfigChange,
    WorktreeCreate,
    WorktreeRemove,
    InstructionsLoaded,
    CwdChanged,
    FileChanged,
}

#[cfg(test)]
#[path = "sdk.test.rs"]
mod tests;
