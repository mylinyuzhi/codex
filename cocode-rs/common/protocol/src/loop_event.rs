//! Event types emitted by the core loop.
//!
//! These events allow consumers to observe the progress of the agent's
//! execution without being coupled to implementation details.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::ApprovalDecision;
use crate::ApprovalRequest;
use crate::PermissionDecision;

// ============================================================================
// Compaction Types (defined before LoopEvent to avoid forward references)
// ============================================================================

/// Trigger type for compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactTrigger {
    /// Automatic compaction based on token thresholds.
    #[default]
    Auto,
    /// Manual compaction triggered by user.
    Manual,
}

impl CompactTrigger {
    /// Get the trigger as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            CompactTrigger::Auto => "auto",
            CompactTrigger::Manual => "manual",
        }
    }
}

impl std::fmt::Display for CompactTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Events emitted during loop execution.
///
/// These events provide a complete view of what the agent is doing,
/// enabling UI updates, logging, and debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoopEvent {
    // ========== Stream Lifecycle ==========
    /// A stream request has started.
    StreamRequestStart,
    /// A stream request has completed.
    StreamRequestEnd {
        /// Token usage for this request.
        usage: TokenUsage,
    },

    // ========== Turn Lifecycle ==========
    /// A new turn has started.
    TurnStarted {
        /// Unique identifier for this turn.
        turn_id: String,
        /// Turn number (1-indexed).
        turn_number: i32,
    },
    /// A turn has completed.
    TurnCompleted {
        /// Unique identifier for this turn.
        turn_id: String,
        /// Token usage for this turn.
        usage: TokenUsage,
    },

    // ========== Content Streaming ==========
    /// Text content delta from the model.
    TextDelta {
        /// Turn identifier.
        turn_id: String,
        /// The text delta.
        delta: String,
    },
    /// Thinking content delta (for models that support thinking).
    ThinkingDelta {
        /// Turn identifier.
        turn_id: String,
        /// The thinking delta.
        delta: String,
    },
    /// Tool call delta (partial tool call JSON).
    ToolCallDelta {
        /// Call identifier.
        call_id: String,
        /// The tool call delta.
        delta: String,
    },
    /// Raw SSE event passthrough.
    StreamEvent {
        /// The raw event data.
        event: RawStreamEvent,
    },

    // ========== Tool Execution ==========
    /// A tool use has been queued for execution.
    ToolUseQueued {
        /// Call identifier.
        call_id: String,
        /// Tool name.
        name: String,
        /// Tool input (JSON).
        input: Value,
    },
    /// A tool has started executing.
    ToolUseStarted {
        /// Call identifier.
        call_id: String,
        /// Tool name.
        name: String,
    },
    /// Progress update from a tool.
    ToolProgress {
        /// Call identifier.
        call_id: String,
        /// Progress information.
        progress: ToolProgressInfo,
    },
    /// A tool has completed execution.
    ToolUseCompleted {
        /// Call identifier.
        call_id: String,
        /// Tool output.
        output: ToolResultContent,
        /// Whether the tool returned an error.
        is_error: bool,
    },
    /// Tool execution was aborted.
    ToolExecutionAborted {
        /// Reason for abortion.
        reason: AbortReason,
    },

    // ========== Permission ==========
    /// User approval is required to proceed.
    ApprovalRequired {
        /// The approval request.
        request: ApprovalRequest,
    },
    /// User has responded to an approval request.
    ApprovalResponse {
        /// Request identifier.
        request_id: String,
        /// The user's decision (approve, approve-with-prefix, or deny).
        decision: ApprovalDecision,
    },
    /// A permission check was evaluated.
    PermissionChecked {
        /// Tool that was checked.
        tool: String,
        /// The decision result.
        decision: PermissionDecision,
    },

    // ========== Agent Events ==========
    /// A sub-agent has been spawned.
    SubagentSpawned {
        /// Agent identifier.
        agent_id: String,
        /// Type of agent.
        agent_type: String,
        /// Description of what the agent will do.
        description: String,
    },
    /// Progress update from a sub-agent.
    SubagentProgress {
        /// Agent identifier.
        agent_id: String,
        /// Progress information.
        progress: AgentProgress,
    },
    /// A sub-agent has completed.
    SubagentCompleted {
        /// Agent identifier.
        agent_id: String,
        /// Result from the agent.
        result: String,
    },
    /// A sub-agent has been moved to background.
    SubagentBackgrounded {
        /// Agent identifier.
        agent_id: String,
        /// Path to output file for monitoring.
        output_file: PathBuf,
    },

    // ========== Background Tasks ==========
    /// A background task has started.
    BackgroundTaskStarted {
        /// Task identifier.
        task_id: String,
        /// Type of task.
        task_type: TaskType,
    },
    /// Progress update from a background task.
    BackgroundTaskProgress {
        /// Task identifier.
        task_id: String,
        /// Progress information.
        progress: TaskProgress,
    },
    /// A background task has completed.
    BackgroundTaskCompleted {
        /// Task identifier.
        task_id: String,
        /// Result from the task.
        result: String,
    },

    // ========== Compaction ==========
    /// Context usage warning - above warning threshold but below auto-compact.
    ContextUsageWarning {
        /// Current estimated tokens.
        estimated_tokens: i32,
        /// Warning threshold.
        warning_threshold: i32,
        /// Percentage of context remaining.
        percent_left: f64,
    },
    /// Full compaction has started.
    CompactionStarted,
    /// Full compaction has completed.
    CompactionCompleted {
        /// Number of messages removed.
        removed_messages: i32,
        /// Tokens in the summary.
        summary_tokens: i32,
    },
    /// Micro-compaction has started.
    MicroCompactionStarted {
        /// Number of candidates identified.
        candidates: i32,
        /// Potential tokens to save.
        potential_savings: i32,
    },
    /// Micro-compaction was applied to tool results.
    MicroCompactionApplied {
        /// Number of results compacted.
        removed_results: i32,
        /// Tokens saved by compaction.
        tokens_saved: i32,
    },
    /// Session memory compaction was applied.
    SessionMemoryCompactApplied {
        /// Tokens saved.
        saved_tokens: i32,
        /// Tokens in the summary.
        summary_tokens: i32,
    },
    /// Compaction was skipped due to a hook rejection.
    CompactionSkippedByHook {
        /// Name of the hook that rejected compaction.
        hook_name: String,
        /// Reason provided by the hook.
        reason: String,
    },
    /// Compaction is being retried after a failure.
    CompactionRetry {
        /// Current attempt number (1-indexed).
        attempt: i32,
        /// Maximum retry attempts allowed.
        max_attempts: i32,
        /// Delay before retry in milliseconds.
        delay_ms: i32,
        /// Reason for the retry.
        reason: String,
    },
    /// Compaction failed after all retries exhausted.
    CompactionFailed {
        /// Total attempts made.
        attempts: i32,
        /// Last error message.
        error: String,
    },
    /// Memory attachments were cleared during compaction.
    MemoryAttachmentsCleared {
        /// UUIDs of cleared memory attachments.
        cleared_uuids: Vec<String>,
        /// Total tokens reclaimed.
        tokens_reclaimed: i32,
    },
    /// SessionStart hooks were executed after compaction.
    PostCompactHooksExecuted {
        /// Number of hooks that ran.
        hooks_executed: i32,
        /// Additional context provided by hooks.
        additional_context_count: i32,
    },
    /// Compact boundary marker was inserted.
    CompactBoundaryInserted {
        /// Trigger type (auto or manual).
        trigger: CompactTrigger,
        /// Tokens before compaction.
        pre_tokens: i32,
        /// Tokens after compaction.
        post_tokens: i32,
    },
    /// Invoked skills were restored after compaction.
    InvokedSkillsRestored {
        /// Skills that were restored.
        skills: Vec<String>,
    },
    /// Context was restored after compaction.
    ContextRestored {
        /// Number of files restored.
        files_count: i32,
        /// Whether todos were restored.
        has_todos: bool,
        /// Whether plan was restored.
        has_plan: bool,
    },

    // ========== Session Memory Extraction ==========
    /// Background session memory extraction has started.
    ///
    /// The extraction agent runs asynchronously during normal conversation to
    /// proactively update the session memory (summary.md).
    SessionMemoryExtractionStarted {
        /// Current token count in the conversation.
        current_tokens: i32,
        /// Number of tool calls since the last extraction.
        tool_calls_since: i32,
    },
    /// Background session memory extraction has completed successfully.
    SessionMemoryExtractionCompleted {
        /// Tokens in the new summary.
        summary_tokens: i32,
        /// ID of the last message that was summarized.
        last_summarized_id: String,
        /// Total number of messages that were summarized.
        messages_summarized: i32,
    },
    /// Background session memory extraction failed.
    SessionMemoryExtractionFailed {
        /// Error message describing the failure.
        error: String,
        /// Number of attempts made before giving up.
        attempts: i32,
    },

    // ========== Model Fallback ==========
    /// Model fallback has started.
    ModelFallbackStarted {
        /// Original model.
        from: String,
        /// Fallback model.
        to: String,
        /// Reason for fallback.
        reason: String,
    },
    /// Model fallback has completed.
    ModelFallbackCompleted,
    /// A message has been tombstoned (marked for removal).
    Tombstone {
        /// The tombstoned message.
        message: TombstonedMessage,
    },

    // ========== Retry Events ==========
    /// A retry is being attempted.
    Retry {
        /// Current attempt number.
        attempt: i32,
        /// Maximum attempts allowed.
        max_attempts: i32,
        /// Delay before retry (milliseconds).
        delay_ms: i32,
    },

    // ========== API Errors ==========
    /// An API error occurred.
    ApiError {
        /// The error.
        error: ApiErrorInfo,
        /// Retry information if retriable.
        retry_info: Option<RetryInfo>,
    },

    // ========== MCP Events ==========
    /// An MCP tool call has begun.
    McpToolCallBegin {
        /// Server name.
        server: String,
        /// Tool name.
        tool: String,
        /// Call identifier.
        call_id: String,
    },
    /// An MCP tool call has ended.
    McpToolCallEnd {
        /// Server name.
        server: String,
        /// Tool name.
        tool: String,
        /// Call identifier.
        call_id: String,
        /// Whether it was an error.
        is_error: bool,
    },
    /// MCP server startup status update.
    McpStartupUpdate {
        /// Server name.
        server: String,
        /// Startup status.
        status: McpStartupStatus,
    },
    /// MCP startup has completed.
    McpStartupComplete {
        /// Successfully started servers.
        servers: Vec<McpServerInfo>,
        /// Failed servers (name, error message).
        failed: Vec<(String, String)>,
    },

    // ========== Plan Mode ==========
    /// Plan mode has been entered.
    PlanModeEntered {
        /// Path to the plan file.
        plan_file: PathBuf,
    },
    /// Plan mode has been exited.
    PlanModeExited {
        /// Whether the plan was approved.
        approved: bool,
    },

    // ========== Hooks ==========
    /// A hook has been executed.
    HookExecuted {
        /// Type of hook event.
        hook_type: HookEventType,
        /// Name of the hook.
        hook_name: String,
    },

    // ========== Stream Stall Detection ==========
    /// A stream stall has been detected.
    StreamStallDetected {
        /// Turn identifier.
        turn_id: String,
        /// Timeout duration that was exceeded.
        #[serde(with = "humantime_serde")]
        timeout: Duration,
    },

    // ========== Prompt Caching ==========
    /// Prompt cache hit.
    PromptCacheHit {
        /// Number of tokens served from cache.
        cached_tokens: i32,
    },
    /// Prompt cache miss.
    PromptCacheMiss,

    // ========== Speculative Execution ==========
    /// Speculative execution has started.
    ///
    /// Tool execution is proceeding optimistically before full confirmation.
    SpeculativeStarted {
        /// Unique identifier for this speculation batch.
        speculation_id: String,
        /// Tool call IDs being executed speculatively.
        tool_calls: Vec<String>,
    },
    /// Speculative execution has been committed.
    ///
    /// The speculative results are confirmed and will be used.
    SpeculativeCommitted {
        /// Speculation batch identifier.
        speculation_id: String,
        /// Number of tool calls committed.
        committed_count: i32,
    },
    /// Speculative execution has been rolled back.
    ///
    /// The speculative results are discarded due to model reconsideration.
    SpeculativeRolledBack {
        /// Speculation batch identifier.
        speculation_id: String,
        /// Reason for rollback.
        reason: String,
        /// Tool calls that were rolled back.
        rolled_back_calls: Vec<String>,
    },

    // ========== Queue ==========
    /// A command was queued (Enter during streaming).
    ///
    /// This command will be processed as a new user turn after the
    /// current turn completes. Also injected as a system reminder
    /// for real-time steering.
    CommandQueued {
        /// Command identifier.
        id: String,
        /// Preview of the command (truncated).
        preview: String,
    },
    /// A queued command was dequeued and is being processed.
    CommandDequeued {
        /// Command identifier.
        id: String,
    },
    /// Queue state changed.
    ///
    /// Emitted when the queue count changes, allowing
    /// the UI to update its status display.
    QueueStateChanged {
        /// Number of commands in the queue.
        queued: i32,
    },

    // ========== Errors & Control ==========
    /// An error occurred in the loop.
    Error {
        /// The error.
        error: LoopError,
    },
    /// The loop was interrupted by the user.
    Interrupted,
    /// Maximum turns reached.
    MaxTurnsReached,
}

/// Raw SSE event from the stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawStreamEvent {
    /// Event type.
    pub event_type: String,
    /// Event data (JSON).
    pub data: Value,
}

/// Token usage information.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input tokens used.
    #[serde(default)]
    pub input_tokens: i64,
    /// Output tokens used.
    #[serde(default)]
    pub output_tokens: i64,
    /// Cache read tokens (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,
    /// Cache creation tokens (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<i64>,
    /// Reasoning tokens (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i64>,
}

impl TokenUsage {
    /// Create a new TokenUsage.
    pub fn new(input_tokens: i64, output_tokens: i64) -> Self {
        Self {
            input_tokens,
            output_tokens,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        }
    }

    /// Get total tokens used.
    pub fn total(&self) -> i64 {
        self.input_tokens + self.output_tokens
    }
}

/// Progress information from a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgressInfo {
    /// Progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Progress percentage (0-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percentage: Option<i32>,
    /// Bytes processed (for file operations).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_processed: Option<i64>,
    /// Total bytes (for file operations).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<i64>,
}

/// Content of a tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    /// Text content.
    Text(String),
    /// Structured content (JSON).
    Structured(Value),
}

impl Default for ToolResultContent {
    fn default() -> Self {
        ToolResultContent::Text(String::new())
    }
}

/// Reason for aborting tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbortReason {
    /// Fallback to non-streaming due to streaming error.
    StreamingFallback,
    /// A sibling tool call encountered an error.
    SiblingError,
    /// User interrupted the operation.
    UserInterrupted,
}

impl AbortReason {
    /// Get the reason as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            AbortReason::StreamingFallback => "streaming_fallback",
            AbortReason::SiblingError => "sibling_error",
            AbortReason::UserInterrupted => "user_interrupted",
        }
    }
}

impl std::fmt::Display for AbortReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Progress information from a sub-agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProgress {
    /// Progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Current step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<i32>,
    /// Total steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_steps: Option<i32>,
}

/// Type of background task.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    /// Shell command execution.
    Shell,
    /// Agent execution.
    Agent,
    /// File operation.
    FileOp,
    /// Other task type.
    Other(String),
}

/// Progress information from a background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    /// Progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Output produced so far.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Exit code (if completed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// A tombstoned message (marked for removal).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TombstonedMessage {
    /// Message role.
    pub role: String,
    /// Message content (summary or placeholder).
    pub content: String,
}

/// Retry information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryInfo {
    /// Current attempt number.
    pub attempt: i32,
    /// Maximum attempts allowed.
    pub max_attempts: i32,
    /// Delay before retry (milliseconds).
    pub delay_ms: i32,
    /// Whether the error is retriable.
    pub retriable: bool,
}

/// API error information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorInfo {
    /// Error code.
    pub code: String,
    /// Error message.
    pub message: String,
    /// HTTP status code (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,
}

/// MCP server startup status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpStartupStatus {
    /// Starting the server.
    Starting,
    /// Connecting to the server.
    Connecting,
    /// Server is ready.
    Ready,
    /// Server failed to start.
    Failed,
}

/// Information about an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    /// Server name.
    pub name: String,
    /// Number of tools provided.
    pub tool_count: i32,
    /// Tool names.
    #[serde(default)]
    pub tools: Vec<String>,
}

/// Type of hook event.
///
/// Mirrors `cocode_hooks::HookEventType` with identical variants and serde names
/// so that conversion between the two is straightforward.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEventType {
    /// Before a tool is used.
    PreToolUse,
    /// After a tool completes successfully.
    PostToolUse,
    /// After a tool use fails.
    PostToolUseFailure,
    /// When the user submits a prompt.
    UserPromptSubmit,
    /// When a session starts.
    SessionStart,
    /// When a session ends.
    SessionEnd,
    /// When the agent stops.
    Stop,
    /// When a sub-agent starts.
    SubagentStart,
    /// When a sub-agent stops.
    SubagentStop,
    /// Before context compaction occurs.
    PreCompact,
    /// A notification event (informational, no blocking).
    Notification,
    /// When a permission is requested.
    PermissionRequest,
}

impl HookEventType {
    /// Get the hook type as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreToolUse => "pre_tool_use",
            Self::PostToolUse => "post_tool_use",
            Self::PostToolUseFailure => "post_tool_use_failure",
            Self::UserPromptSubmit => "user_prompt_submit",
            Self::SessionStart => "session_start",
            Self::SessionEnd => "session_end",
            Self::Stop => "stop",
            Self::SubagentStart => "subagent_start",
            Self::SubagentStop => "subagent_stop",
            Self::PreCompact => "pre_compact",
            Self::Notification => "notification",
            Self::PermissionRequest => "permission_request",
        }
    }
}

impl std::fmt::Display for HookEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// An error that occurred in the loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopError {
    /// Error code.
    pub code: String,
    /// Error message.
    pub message: String,
    /// Whether this error is recoverable.
    #[serde(default)]
    pub recoverable: bool,
}

// ============================================================================
// Additional Compaction Types
// ============================================================================

/// Memory attachment information for tracking during compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAttachment {
    /// Unique identifier for this attachment.
    pub uuid: String,
    /// Type of attachment (e.g., "memory", "file", "tool_result").
    pub attachment_type: AttachmentType,
    /// Token count for this attachment.
    pub token_count: i32,
    /// Whether this attachment has been cleared.
    #[serde(default)]
    pub cleared: bool,
}

/// Type of attachment in the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentType {
    /// Memory attachment (session memory, context).
    Memory,
    /// File content attachment.
    File,
    /// Tool result attachment.
    ToolResult,
    /// Skill attachment.
    Skill,
    /// Task status attachment.
    TaskStatus,
    /// Hook output attachment.
    HookOutput,
    /// System reminder attachment.
    SystemReminder,
    /// Other attachment type.
    Other(String),
}

impl AttachmentType {
    /// Get the attachment type as a string.
    pub fn as_str(&self) -> &str {
        match self {
            AttachmentType::Memory => "memory",
            AttachmentType::File => "file",
            AttachmentType::ToolResult => "tool_result",
            AttachmentType::Skill => "skill",
            AttachmentType::TaskStatus => "task_status",
            AttachmentType::HookOutput => "hook_output",
            AttachmentType::SystemReminder => "system_reminder",
            AttachmentType::Other(s) => s,
        }
    }
}

/// Compact telemetry data for analytics and monitoring.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactTelemetry {
    /// Tokens before compaction.
    pub pre_tokens: i32,
    /// Tokens after compaction.
    pub post_tokens: i32,
    /// Cache read tokens used.
    #[serde(default)]
    pub cache_read_tokens: i32,
    /// Cache creation tokens used.
    #[serde(default)]
    pub cache_creation_tokens: i32,
    /// Token breakdown by category.
    #[serde(default)]
    pub token_breakdown: TokenBreakdown,
    /// Compaction trigger type.
    pub trigger: Option<CompactTrigger>,
    /// Whether streaming was used for summarization.
    #[serde(default)]
    pub has_started_streaming: bool,
    /// Number of retry attempts made.
    #[serde(default)]
    pub retry_attempts: i32,
}

/// Token breakdown for telemetry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenBreakdown {
    /// Total tokens.
    #[serde(default)]
    pub total_tokens: i32,
    /// Human message tokens.
    #[serde(default)]
    pub human_message_tokens: i32,
    /// Human message percentage.
    #[serde(default)]
    pub human_message_pct: f64,
    /// Assistant message tokens.
    #[serde(default)]
    pub assistant_message_tokens: i32,
    /// Assistant message percentage.
    #[serde(default)]
    pub assistant_message_pct: f64,
    /// Local command output tokens.
    #[serde(default)]
    pub local_command_output_tokens: i32,
    /// Local command output percentage.
    #[serde(default)]
    pub local_command_output_pct: f64,
    /// Attachment token counts by type.
    #[serde(default)]
    pub attachment_tokens: HashMap<String, i32>,
    /// Tool request tokens by tool name.
    #[serde(default)]
    pub tool_request_tokens: HashMap<String, i32>,
    /// Tool result tokens by tool name.
    #[serde(default)]
    pub tool_result_tokens: HashMap<String, i32>,
    /// Tokens from duplicate file reads.
    #[serde(default)]
    pub duplicate_read_tokens: i32,
    /// Count of duplicate file reads.
    #[serde(default)]
    pub duplicate_read_file_count: i32,
}

/// Compact boundary marker metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactBoundaryMetadata {
    /// Trigger type for this compaction.
    pub trigger: CompactTrigger,
    /// Tokens before compaction.
    pub pre_tokens: i32,
    /// Tokens after compaction.
    #[serde(default)]
    pub post_tokens: Option<i32>,
    /// Transcript file path for full history.
    #[serde(default)]
    pub transcript_path: Option<PathBuf>,
    /// Whether recent messages were preserved verbatim.
    #[serde(default)]
    pub recent_messages_preserved: bool,
}

/// Hook additional context from post-compact hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookAdditionalContext {
    /// Content provided by the hook.
    pub content: String,
    /// Name of the hook that provided the context.
    pub hook_name: String,
    /// Whether to suppress output in the UI.
    #[serde(default)]
    pub suppress_output: bool,
}

/// Persisted tool result reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedToolResult {
    /// Path to the persisted file.
    pub path: PathBuf,
    /// Original size in bytes.
    pub original_size: i64,
    /// Original token count.
    pub original_tokens: i32,
    /// Tool use ID.
    pub tool_use_id: String,
}

impl PersistedToolResult {
    /// Format as XML reference for injection into messages.
    pub fn to_xml_reference(&self) -> String {
        format!(
            "<persisted-output path=\"{}\" original_size=\"{}\" original_tokens=\"{}\" />",
            self.path.display(),
            self.original_size,
            self.original_tokens
        )
    }
}

#[cfg(test)]
#[path = "loop_event.test.rs"]
mod tests;
