//! Server notifications — events sent from the server to all clients.
//!
//! These cover the complete lifecycle visible to any frontend: turn/item
//! progression, content streaming, approval requests, sub-agent events,
//! context management, and system-level signals.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::ThreadItem;
use crate::Usage;

/// Events emitted by the server to all connected clients.
///
/// The `method` field uses slash-delimited paths (e.g. `"turn/started"`)
/// following the codex-rs convention for JSON-RPC-style routing.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "params", rename_all = "camelCase")]
pub enum ServerNotification {
    // ── Session lifecycle ──────────────────────────────────────────────
    /// A new session has been created.
    #[serde(rename = "session/started")]
    SessionStarted(SessionStartedParams),

    // ── Turn lifecycle ─────────────────────────────────────────────────
    /// A new turn has started (prompt sent to model).
    #[serde(rename = "turn/started")]
    TurnStarted(TurnStartedParams),
    /// A turn has completed successfully.
    #[serde(rename = "turn/completed")]
    TurnCompleted(TurnCompletedParams),
    /// A turn has failed.
    #[serde(rename = "turn/failed")]
    TurnFailed(TurnFailedParams),

    // ── Item lifecycle ─────────────────────────────────────────────────
    /// A new item has been created (typically in-progress).
    #[serde(rename = "item/started")]
    ItemStarted(ItemEventParams),
    /// An existing item has been updated (e.g. output appended).
    #[serde(rename = "item/updated")]
    ItemUpdated(ItemEventParams),
    /// An item has reached a terminal state (completed or failed).
    #[serde(rename = "item/completed")]
    ItemCompleted(ItemEventParams),

    // ── Content streaming (real-time deltas) ───────────────────────────
    /// Incremental assistant text.
    #[serde(rename = "agentMessage/delta")]
    AgentMessageDelta(AgentMessageDeltaParams),
    /// Incremental reasoning / thinking text.
    #[serde(rename = "reasoning/delta")]
    ReasoningDelta(ReasoningDeltaParams),

    // ── Sub-agent events ───────────────────────────────────────────────
    /// A sub-agent has been spawned.
    #[serde(rename = "subagent/spawned")]
    SubagentSpawned(SubagentSpawnedParams),
    /// A sub-agent has completed.
    #[serde(rename = "subagent/completed")]
    SubagentCompletedParams(SubagentCompletedParams),
    /// A sub-agent has been moved to background execution.
    #[serde(rename = "subagent/backgrounded")]
    SubagentBackgrounded(SubagentBackgroundedParams),

    // ── MCP events ─────────────────────────────────────────────────────
    /// MCP server startup status.
    #[serde(rename = "mcp/startupStatus")]
    McpStartupStatus(McpStartupStatusParams),
    /// MCP startup completed (all servers attempted).
    #[serde(rename = "mcp/startupComplete")]
    McpStartupComplete(McpStartupCompleteParams),

    // ── Context management ─────────────────────────────────────────────
    /// Context was compacted to stay within window limits.
    #[serde(rename = "context/compacted")]
    ContextCompacted(ContextCompactedParams),
    /// Context usage is approaching limits.
    #[serde(rename = "context/usageWarning")]
    ContextUsageWarning(ContextUsageWarningParams),

    // ── Background task events ─────────────────────────────────────────
    /// A background task has started.
    #[serde(rename = "task/started")]
    TaskStarted(TaskStartedParams),
    /// A background task has completed.
    #[serde(rename = "task/completed")]
    TaskCompleted(TaskCompletedParams),

    // ── Turn lifecycle (additional) ─────────────────────────────────
    /// The current turn was interrupted.
    #[serde(rename = "turn/interrupted")]
    TurnInterrupted(TurnInterruptedParams),
    /// Maximum turns limit reached.
    #[serde(rename = "turn/maxReached")]
    MaxTurnsReached(MaxTurnsReachedParams),

    // ── Model events ────────────────────────────────────────────────
    /// Model fallback started (switching to a different model).
    #[serde(rename = "model/fallbackStarted")]
    ModelFallbackStarted(ModelFallbackStartedParams),

    // ── Permission events ───────────────────────────────────────────
    /// Permission mode has changed.
    #[serde(rename = "permission/modeChanged")]
    PermissionModeChanged(PermissionModeChangedParams),

    // ── Session result ─────────────────────────────────────────────
    /// Aggregated session result emitted before session/ended.
    #[serde(rename = "session/result")]
    SessionResult(SessionResultParams),

    // ── Prompt suggestions ──────────────────────────────────────────
    /// Follow-up prompt suggestions after a turn completes.
    #[serde(rename = "prompt/suggestion")]
    PromptSuggestion(PromptSuggestionParams),

    // ── System-level events ────────────────────────────────────────────
    /// A non-fatal error occurred.
    #[serde(rename = "error")]
    Error(ErrorNotificationParams),
    /// API rate limit information.
    #[serde(rename = "rateLimit")]
    RateLimit(RateLimitParams),
    /// Keepalive echo from the server.
    #[serde(rename = "keepAlive")]
    KeepAlive(KeepAliveParams),
    /// Session has ended (clean termination).
    #[serde(rename = "session/ended")]
    SessionEnded(SessionEndedParams),
}

// ---------------------------------------------------------------------------
// Param structs
// ---------------------------------------------------------------------------

/// Parameters for `session/started`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionStartedParams {
    /// Session identifier.
    pub session_id: String,
    /// Protocol version (for future version negotiation).
    #[serde(default = "default_protocol_version")]
    pub protocol_version: String,
    /// Available model identifiers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    /// Available slash commands / skills.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<CommandInfo>>,
}

fn default_protocol_version() -> String {
    "1".to_string()
}

/// Information about an available command or skill.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CommandInfo {
    /// Command name (e.g., "/commit").
    pub name: String,
    /// Short description of the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Parameters for `turn/started`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TurnStartedParams {
    /// Turn identifier.
    pub turn_id: String,
    /// Turn number (1-indexed).
    pub turn_number: i32,
}

/// Parameters for `turn/completed`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TurnCompletedParams {
    /// Turn identifier.
    pub turn_id: String,
    /// Token usage for this turn.
    pub usage: Usage,
}

/// Parameters for `turn/failed`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TurnFailedParams {
    /// Error message.
    pub error: String,
}

/// Parameters for item lifecycle events.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ItemEventParams {
    /// The item (with current state).
    pub item: ThreadItem,
}

/// Parameters for `agentMessage/delta`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentMessageDeltaParams {
    /// Item identifier of the message being streamed.
    pub item_id: String,
    /// Turn identifier.
    pub turn_id: String,
    /// Incremental text content.
    pub delta: String,
}

/// Parameters for `reasoning/delta`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReasoningDeltaParams {
    /// Item identifier of the reasoning block.
    pub item_id: String,
    /// Turn identifier.
    pub turn_id: String,
    /// Incremental reasoning text.
    pub delta: String,
}

/// Parameters for `subagent/spawned`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubagentSpawnedParams {
    /// Agent identifier.
    pub agent_id: String,
    /// Agent type.
    pub agent_type: String,
    /// Short description.
    pub description: String,
    /// Display color hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Parameters for `subagent/completed`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubagentCompletedParams {
    /// Agent identifier.
    pub agent_id: String,
    /// Agent result text.
    pub result: String,
}

/// Parameters for `subagent/backgrounded`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubagentBackgroundedParams {
    /// Agent identifier.
    pub agent_id: String,
    /// Path to the output file for monitoring.
    pub output_file: String,
}

/// Parameters for `mcp/startupStatus`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McpStartupStatusParams {
    /// Server name.
    pub server: String,
    /// Status description.
    pub status: String,
}

/// Parameters for `mcp/startupComplete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McpStartupCompleteParams {
    /// Successfully started servers.
    pub servers: Vec<McpServerInfoParams>,
    /// Failed servers.
    pub failed: Vec<McpServerFailure>,
}

/// Info about a successfully started MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McpServerInfoParams {
    /// Server name.
    pub name: String,
    /// Number of tools provided.
    pub tool_count: i32,
}

/// Info about a failed MCP server startup.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McpServerFailure {
    /// Server name.
    pub name: String,
    /// Error message.
    pub error: String,
}

/// Parameters for `context/compacted`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextCompactedParams {
    /// Messages removed during compaction.
    pub removed_messages: i32,
    /// Tokens in the compacted summary.
    pub summary_tokens: i32,
}

/// Parameters for `context/usageWarning`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextUsageWarningParams {
    /// Current estimated token count.
    pub estimated_tokens: i32,
    /// Warning threshold.
    pub warning_threshold: i32,
    /// Percentage of context remaining.
    pub percent_left: f64,
}

/// Parameters for `task/started`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskStartedParams {
    /// Task identifier.
    pub task_id: String,
    /// Type of task (e.g., "shell", "agent").
    pub task_type: String,
}

/// Parameters for `task/completed`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCompletedParams {
    /// Task identifier.
    pub task_id: String,
    /// Task result text.
    pub result: String,
    /// Whether the task failed.
    #[serde(default)]
    pub is_error: bool,
}

/// Parameters for `turn/interrupted`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TurnInterruptedParams {
    /// Turn identifier (if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

/// Parameters for `turn/maxReached`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MaxTurnsReachedParams {
    /// Maximum turns limit that was reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,
}

/// Parameters for `model/fallbackStarted`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelFallbackStartedParams {
    /// Original model.
    pub from_model: String,
    /// Fallback model.
    pub to_model: String,
    /// Reason for fallback.
    pub reason: String,
}

/// Parameters for `permission/modeChanged`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PermissionModeChangedParams {
    /// The new permission mode.
    pub mode: String,
}

/// Parameters for `error`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ErrorNotificationParams {
    /// Error message.
    pub message: String,
    /// Error category (e.g. "api", "tool", "internal").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Whether the error is retryable.
    #[serde(default)]
    pub retryable: bool,
}

/// Parameters for `rateLimit`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RateLimitParams {
    /// Rate limit details.
    pub info: Value,
}

/// Parameters for `keepAlive`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KeepAliveParams {
    /// Timestamp (milliseconds since epoch).
    pub timestamp: i64,
}

/// Parameters for `session/result`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionResultParams {
    /// Session identifier.
    pub session_id: String,
    /// Total number of turns in the session.
    pub total_turns: i32,
    /// Total cost in cents (if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_cost_cents: Option<i64>,
    /// Total session duration in milliseconds.
    pub duration_ms: i64,
    /// Total time spent on API calls in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_api_ms: Option<i64>,
    /// Aggregated token usage across all turns.
    pub usage: Usage,
    /// Why the session ended.
    pub stop_reason: SessionEndedReason,
    /// Structured output (if output_format schema was provided).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<Value>,
}

/// Parameters for `prompt/suggestion`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PromptSuggestionParams {
    /// Suggested follow-up prompts.
    pub suggestions: Vec<String>,
}

/// Reason why a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndedReason {
    /// All turns completed normally.
    Completed,
    /// Maximum turn limit reached.
    MaxTurns,
    /// Budget limit exceeded.
    MaxBudget,
    /// An error terminated the session.
    Error,
    /// User interrupted the session.
    UserInterrupt,
    /// stdin was closed (SDK client disconnected).
    StdinClosed,
}

/// Parameters for `session/ended`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionEndedParams {
    /// Reason for session termination.
    pub reason: SessionEndedReason,
}
