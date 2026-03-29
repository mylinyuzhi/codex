//! Server notifications — events sent from the server to all clients.
//!
//! Uses the `server_notification_definitions!` macro (inspired by codex-rs)
//! to generate the [`ServerNotification`] enum with serde tag/rename,
//! `to_params()`, and `method()` accessor.
//!
//! The `schemars::JsonSchema` derive is gated behind the `schema` feature
//! so that downstream crates that don't need codegen aren't burdened.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use super::item::ThreadItem;
use super::usage::Usage;

// ---------------------------------------------------------------------------
// Declarative macro — generates ServerNotification enum + helpers
// ---------------------------------------------------------------------------

/// Generates the `ServerNotification` enum with JSON-RPC method routing.
///
/// Each variant maps to a wire method name (e.g. `"turn/started"`) and a
/// payload type. The macro generates:
/// - The enum with `#[serde(tag = "method", content = "params")]`
/// - `to_params()` → `serde_json::Value` conversion
/// - `method()` → wire method name accessor
macro_rules! server_notification_definitions {
    ($(
        $(#[$meta:meta])*
        $variant:ident => $wire:literal ($payload:ty)
    ),* $(,)?) => {
        /// Events emitted by the server to all connected clients.
        ///
        /// The `method` field uses slash-delimited paths (e.g. `"turn/started"`)
        /// following the codex-rs convention for JSON-RPC-style routing.
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        #[serde(tag = "method", content = "params")]
        pub enum ServerNotification {
            $(
                $(#[$meta])*
                #[serde(rename = $wire)]
                $variant($payload),
            )*
        }

        impl ServerNotification {
            /// Convert notification params to a JSON value.
            pub fn to_params(self) -> Result<serde_json::Value, serde_json::Error> {
                match self {
                    $(Self::$variant(params) => serde_json::to_value(params),)*
                }
            }

            /// Get the wire method name.
            pub fn method(&self) -> &'static str {
                match self {
                    $(Self::$variant(_) => $wire,)*
                }
            }
        }
    };
}

// ---------------------------------------------------------------------------
// ServerNotification enum — all variants defined via macro
// ---------------------------------------------------------------------------

server_notification_definitions! {
    // ── Session lifecycle ──────────────────────────────────────────────
    /// A new session has been created.
    SessionStarted => "session/started" (SessionStartedParams),
    /// Aggregated session result emitted before session/ended.
    SessionResult => "session/result" (SessionResultParams),
    /// Session has ended (clean termination).
    SessionEnded => "session/ended" (SessionEndedParams),

    // ── Turn lifecycle ─────────────────────────────────────────────────
    /// A new turn has started (prompt sent to model).
    TurnStarted => "turn/started" (TurnStartedParams),
    /// A turn has completed successfully.
    TurnCompleted => "turn/completed" (TurnCompletedParams),
    /// A turn has failed.
    TurnFailed => "turn/failed" (TurnFailedParams),
    /// The current turn was interrupted.
    TurnInterrupted => "turn/interrupted" (TurnInterruptedParams),
    /// Maximum turns limit reached.
    MaxTurnsReached => "turn/maxReached" (MaxTurnsReachedParams),
    /// A retry is being attempted after a transient failure.
    TurnRetry => "turn/retry" (TurnRetryParams),

    // ── Item lifecycle ─────────────────────────────────────────────────
    /// A new item has been created (typically in-progress).
    ItemStarted => "item/started" (ItemEventParams),
    /// An existing item has been updated.
    ItemUpdated => "item/updated" (ItemEventParams),
    /// An item has reached a terminal state.
    ItemCompleted => "item/completed" (ItemEventParams),

    // ── Content streaming ──────────────────────────────────────────────
    /// Incremental assistant text.
    AgentMessageDelta => "agentMessage/delta" (AgentMessageDeltaParams),
    /// Incremental reasoning / thinking text.
    ReasoningDelta => "reasoning/delta" (ReasoningDeltaParams),

    // ── Sub-agent events ───────────────────────────────────────────────
    /// A sub-agent has been spawned.
    SubagentSpawned => "subagent/spawned" (SubagentSpawnedParams),
    /// A sub-agent has completed.
    SubagentCompleted => "subagent/completed" (SubagentCompletedParams),
    /// A sub-agent has been moved to background execution.
    SubagentBackgrounded => "subagent/backgrounded" (SubagentBackgroundedParams),
    /// Sub-agent progress update.
    SubagentProgress => "subagent/progress" (SubagentProgressParams),

    // ── MCP events ─────────────────────────────────────────────────────
    /// MCP server startup status.
    McpStartupStatus => "mcp/startupStatus" (McpStartupStatusParams),
    /// MCP startup completed (all servers attempted).
    McpStartupComplete => "mcp/startupComplete" (McpStartupCompleteParams),

    // ── Context management ─────────────────────────────────────────────
    /// Context was compacted to stay within window limits.
    ContextCompacted => "context/compacted" (ContextCompactedParams),
    /// Context usage is approaching limits.
    ContextUsageWarning => "context/usageWarning" (ContextUsageWarningParams),
    /// Compaction has started.
    CompactionStarted => "context/compactionStarted" (CompactionStartedParams),
    /// Compaction has failed.
    CompactionFailed => "context/compactionFailed" (CompactionFailedParams),
    /// Context was cleared (e.g., after plan mode exit).
    ContextCleared => "context/cleared" (ContextClearedParams),

    // ── Background task events ─────────────────────────────────────────
    /// A background task has started.
    TaskStarted => "task/started" (TaskStartedParams),
    /// A background task has completed.
    TaskCompleted => "task/completed" (TaskCompletedParams),
    /// Background task progress update.
    TaskProgress => "task/progress" (TaskProgressParams),
    /// All running agents were killed.
    AgentsKilled => "agents/killed" (AgentsKilledParams),

    // ── Model events ───────────────────────────────────────────────────
    /// Model fallback started (switching to a different model).
    ModelFallbackStarted => "model/fallbackStarted" (ModelFallbackStartedParams),
    /// Model fallback completed (returned to original or stabilized).
    ModelFallbackCompleted => "model/fallbackCompleted" (ModelFallbackCompletedParams),

    // ── Permission events ──────────────────────────────────────────────
    /// Permission mode has changed.
    PermissionModeChanged => "permission/modeChanged" (PermissionModeChangedParams),

    // ── Prompt suggestions ─────────────────────────────────────────────
    /// Follow-up prompt suggestions after a turn completes.
    PromptSuggestion => "prompt/suggestion" (PromptSuggestionParams),

    // ── System-level events ────────────────────────────────────────────
    /// A non-fatal error occurred.
    Error => "error" (ErrorNotificationParams),
    /// API rate limit information.
    RateLimit => "rateLimit" (RateLimitParams),
    /// Keepalive echo from the server.
    KeepAlive => "keepAlive" (KeepAliveParams),

    // ── IDE integration events ─────────────────────────────────────────
    /// IDE selection/focus changed.
    IdeSelectionChanged => "ide/selectionChanged" (IdeSelectionChangedParams),
    /// IDE diagnostics updated.
    IdeDiagnosticsUpdated => "ide/diagnosticsUpdated" (IdeDiagnosticsUpdatedParams),

    // ── Plan mode ──────────────────────────────────────────────────────
    /// Plan mode state changed (entered or exited).
    PlanModeChanged => "plan/modeChanged" (PlanModeChangedParams),

    // ── Queue ──────────────────────────────────────────────────────────
    /// Command queue state changed.
    QueueStateChanged => "queue/stateChanged" (QueueStateChangedParams),
    /// A command was queued during streaming.
    CommandQueued => "queue/commandQueued" (CommandQueuedParams),
    /// A queued command was dequeued for processing.
    CommandDequeued => "queue/commandDequeued" (CommandDequeuedParams),

    // ── Rewind ─────────────────────────────────────────────────────────
    /// A rewind operation completed successfully.
    RewindCompleted => "rewind/completed" (RewindCompletedParams),
    /// A rewind operation failed.
    RewindFailed => "rewind/failed" (RewindFailedParams),

    // ── Cost ───────────────────────────────────────────────────────────
    /// Cost warning threshold reached.
    CostWarning => "cost/warning" (CostWarningParams),

    // ── Sandbox ────────────────────────────────────────────────────────
    /// Sandbox enforcement state changed.
    SandboxStateChanged => "sandbox/stateChanged" (SandboxStateChangedParams),
    /// Sandbox violations detected.
    SandboxViolationsDetected => "sandbox/violationsDetected" (SandboxViolationsDetectedParams),

    // ── Mode ───────────────────────────────────────────────────────────
    /// Fast mode toggled.
    FastModeChanged => "model/fastModeChanged" (FastModeChangedParams),

    // ── Agent registry ─────────────────────────────────────────────────
    /// Plugin agents have been registered and are available.
    AgentsRegistered => "agents/registered" (AgentsRegisteredParams),

    // ── Hook ───────────────────────────────────────────────────────────
    /// A hook was executed.
    HookExecuted => "hook/executed" (HookExecutedParams),

    // ── Summarize ──────────────────────────────────────────────────────
    /// Partial compaction completed.
    SummarizeCompleted => "summarize/completed" (SummarizeCompletedParams),
    /// Partial compaction failed.
    SummarizeFailed => "summarize/failed" (SummarizeFailedParams),

    // ── Stream health ──────────────────────────────────────────────────
    /// Stream stall detected (no data for extended period).
    StreamStallDetected => "stream/stallDetected" (StreamStallDetectedParams),
    /// Stream watchdog warning (silence approaching timeout).
    StreamWatchdogWarning => "stream/watchdogWarning" (StreamWatchdogWarningParams),

    // ── Stream lifecycle ───────────────────────────────────────────────
    /// A stream request has completed with token usage.
    StreamRequestEnd => "stream/requestEnd" (StreamRequestEndParams)
}

// ---------------------------------------------------------------------------
// Param structs
// ---------------------------------------------------------------------------

/// Parameters for `session/started`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SessionStartedParams {
    pub session_id: String,
    #[serde(default = "default_protocol_version")]
    pub protocol_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<CommandInfo>>,
}

fn default_protocol_version() -> String {
    "1".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CommandInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TurnStartedParams {
    pub turn_id: String,
    pub turn_number: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TurnCompletedParams {
    pub turn_id: String,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TurnFailedParams {
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ItemEventParams {
    pub item: ThreadItem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AgentMessageDeltaParams {
    pub item_id: String,
    pub turn_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReasoningDeltaParams {
    pub item_id: String,
    pub turn_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SubagentSpawnedParams {
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SubagentCompletedParams {
    pub agent_id: String,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SubagentBackgroundedParams {
    pub agent_id: String,
    pub output_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct McpStartupStatusParams {
    pub server: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct McpStartupCompleteParams {
    pub servers: Vec<McpServerInfoParams>,
    pub failed: Vec<McpServerFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct McpServerInfoParams {
    pub name: String,
    pub tool_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct McpServerFailure {
    pub name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContextCompactedParams {
    pub removed_messages: i32,
    pub summary_tokens: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContextUsageWarningParams {
    pub estimated_tokens: i32,
    pub warning_threshold: i32,
    pub percent_left: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TaskStartedParams {
    pub task_id: String,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TaskCompletedParams {
    pub task_id: String,
    pub result: String,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TurnInterruptedParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MaxTurnsReachedParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ModelFallbackStartedParams {
    pub from_model: String,
    pub to_model: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PermissionModeChangedParams {
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ErrorNotificationParams {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<ErrorCategory>,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_info: Option<ErrorInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Api,
    Tool,
    Internal,
    Network,
    Config,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ErrorInfo {
    ContextWindowExceeded,
    RateLimitExceeded,
    AuthenticationFailed,
    ServerOverloaded {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<i64>,
    },
    ToolExecutionFailed {
        tool_name: String,
    },
    HttpConnectionFailed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        http_status_code: Option<i32>,
    },
    BudgetExceeded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RateLimitParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct KeepAliveParams {
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SessionResultParams {
    pub session_id: String,
    pub total_turns: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_cost_cents: Option<i64>,
    pub duration_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_api_ms: Option<i64>,
    pub usage: Usage,
    pub stop_reason: SessionEndedReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PromptSuggestionParams {
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SessionEndedReason {
    Completed,
    MaxTurns,
    MaxBudget,
    Error,
    UserInterrupt,
    StdinClosed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SessionEndedParams {
    pub reason: SessionEndedReason,
}

// ── IDE params ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdeSelectionChangedParams {
    pub file_path: String,
    #[serde(default)]
    pub selected_text: String,
    pub start_line: i32,
    pub end_line: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdeDiagnosticsUpdatedParams {
    pub file_path: String,
    pub new_count: i32,
    pub diagnostics: Vec<IdeDiagnosticInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IdeDiagnosticInfo {
    pub message: String,
    pub severity: String,
    pub line: i32,
}

// ── Plan mode params ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlanModeChangedParams {
    pub entered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<bool>,
}

// ── Queue params ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueueStateChangedParams {
    pub queued: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CommandQueuedParams {
    pub id: String,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CommandDequeuedParams {
    pub id: String,
}

// ── Rewind params ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RewindCompletedParams {
    pub rewound_turn: i32,
    pub restored_files: i32,
    pub messages_removed: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RewindFailedParams {
    pub error: String,
}

// ── Cost params ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CostWarningParams {
    pub current_cost_cents: i32,
    pub threshold_cents: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_cents: Option<i32>,
}

// ── Sandbox params ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SandboxStateChangedParams {
    pub active: bool,
    pub enforcement: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SandboxViolationsDetectedParams {
    pub count: i32,
}

// ── Fast mode params ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FastModeChangedParams {
    pub active: bool,
}

// ── Agent registry params ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AgentsRegisteredParams {
    pub agents: Vec<AgentInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AgentInfo {
    pub name: String,
    pub agent_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ── Hook params ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HookExecutedParams {
    pub hook_type: String,
    pub hook_name: String,
}

// ── Summarize params ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SummarizeCompletedParams {
    pub from_turn: i32,
    pub summary_tokens: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SummarizeFailedParams {
    pub error: String,
}

// ── Protocol notification types ────────────────────────────────────────

/// Parameters for `turn/retry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TurnRetryParams {
    pub attempt: i32,
    pub max_attempts: i32,
    pub delay_ms: i32,
}

/// Parameters for `subagent/progress`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SubagentProgressParams {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Parameters for `context/compactionStarted`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CompactionStartedParams {}

/// Parameters for `context/compactionFailed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CompactionFailedParams {
    pub error: String,
    pub attempts: i32,
}

/// Parameters for `context/cleared`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContextClearedParams {
    pub new_mode: String,
}

/// Parameters for `task/progress`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TaskProgressParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Parameters for `agents/killed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AgentsKilledParams {
    pub count: i32,
    pub agent_ids: Vec<String>,
}

/// Parameters for `model/fallbackCompleted`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ModelFallbackCompletedParams {}

/// Parameters for `stream/stallDetected`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StreamStallDetectedParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

/// Parameters for `stream/watchdogWarning`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StreamWatchdogWarningParams {
    pub elapsed_secs: i64,
}

/// Parameters for `stream/requestEnd`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StreamRequestEndParams {
    pub usage: Usage,
}

/// Parameters for `keepAlive` (re-export alias).
pub type KeepAliveNotifParams = KeepAliveParams;
