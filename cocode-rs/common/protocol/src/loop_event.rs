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
use strum::Display;
use strum::IntoStaticStr;

use crate::ApprovalDecision;
use crate::ApprovalRequest;
use crate::PermissionDecision;

// ============================================================================
// Compaction Types (defined before LoopEvent to avoid forward references)
// ============================================================================

/// Trigger type for compaction.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Display, IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
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
        (*self).into()
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
        /// Display color from agent definition (for TUI rendering).
        color: Option<String>,
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
    /// All running agents were killed (e.g., via Ctrl+F).
    AllAgentsKilled {
        /// Number of agents killed.
        count: usize,
        /// IDs of the killed agents.
        agent_ids: Vec<String>,
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
    /// Auto-compaction circuit breaker opened after consecutive failures.
    ///
    /// When 3 consecutive compaction attempts fail, the circuit breaker opens
    /// and auto-compaction is disabled for the remainder of the session.
    /// Manual compaction still works.
    CompactionCircuitBreakerOpen {
        /// Number of consecutive failures that triggered the breaker.
        consecutive_failures: i32,
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

    // ========== User Questions ==========
    /// The model is asking the user a question (AskUserQuestion tool).
    ///
    /// The TUI should display a question overlay and send back the user's
    /// answers via `UserCommand::QuestionResponse`.
    QuestionAsked {
        /// Unique request identifier (used to correlate the response).
        request_id: String,
        /// The questions to display (JSON array of question objects).
        questions: Value,
    },

    /// An MCP server is requesting user input via elicitation.
    ///
    /// The TUI should display an elicitation overlay (form or URL mode) and
    /// send back the user's response via `UserCommand::ElicitationResponse`.
    ElicitationRequested {
        /// Unique request identifier (used to correlate the response).
        request_id: String,
        /// Name of the MCP server requesting input.
        server_name: String,
        /// Human-readable message to display.
        message: String,
        /// Elicitation mode: "form" or "url".
        mode: String,
        /// JSON Schema for form mode (None for URL mode).
        schema: Option<Value>,
        /// URL for URL mode (None for form mode).
        url: Option<String>,
    },

    // ========== Plan Mode ==========
    /// Plan mode has been entered.
    PlanModeEntered {
        /// Path to the plan file (None when entered via Tab cycling).
        plan_file: Option<PathBuf>,
    },
    /// Plan mode has been exited.
    PlanModeExited {
        /// Whether the plan was approved.
        approved: bool,
    },
    /// Conversation context was cleared (e.g., after plan mode exit with clear option).
    ContextCleared {
        /// The new permission mode applied after clearing.
        new_mode: crate::PermissionMode,
    },

    // ========== Permission Mode ==========
    /// Permission mode has been changed (e.g., via Shift+Tab cycling).
    PermissionModeChanged {
        /// The new permission mode.
        mode: crate::PermissionMode,
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

    /// Stream watchdog warning — no SSE event received for `warning_timeout`.
    ///
    /// This is the first tier of the two-tier watchdog. The stream continues
    /// after this event; the abort tier fires if silence persists.
    StreamWatchdogWarning {
        /// Seconds elapsed since last event.
        elapsed_secs: i64,
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

    // ========== Plugin Events ==========
    /// Plugin agents have been loaded and are available for use.
    ///
    /// Emitted after session initialization when plugin agents are registered
    /// with the subagent manager. The TUI uses this to update its agent
    /// autocomplete.
    PluginAgentsLoaded {
        /// Agent definitions (name, agent_type, description).
        agents: Vec<PluginAgentInfo>,
    },

    /// Plugin data ready for UI display.
    ///
    /// Sent in response to a request for plugin summary data.
    /// Contains installed plugins and marketplace info for the Plugin Manager overlay.
    PluginDataReady {
        /// Installed plugin summaries.
        installed: Vec<PluginSummaryInfo>,
        /// Known marketplace summaries.
        marketplaces: Vec<MarketplaceSummaryInfo>,
    },

    /// Output styles data ready for the picker overlay.
    OutputStylesReady {
        /// Available output style items: (name, source_label, description).
        styles: Vec<OutputStyleItem>,
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

    // ========== System Reminder Display ==========
    /// A system reminder display event for UI notification.
    ///
    /// Emitted for silent reminders (e.g., already-read files) so the UI can
    /// display notifications without the reminder consuming API tokens.
    /// The `reminder_type` identifies the attachment type and `payload` carries
    /// type-specific display data.
    SystemReminderDisplay {
        /// The attachment type name (e.g., "already_read_file").
        reminder_type: String,
        /// Type-specific display payload (e.g., list of file paths).
        payload: Value,
    },

    // ========== Rewind ==========
    /// A rewind completed successfully.
    RewindCompleted {
        /// The turn number that was rewound.
        rewound_turn: i32,
        /// Number of files restored.
        restored_files: i32,
        /// Number of messages removed from conversation history.
        messages_removed: i32,
        /// The rewind mode used.
        mode: RewindMode,
        /// The original user prompt text at the rewound turn (for input restoration).
        restored_prompt: Option<String>,
    },
    /// A rewind attempt failed.
    RewindFailed {
        /// Error message.
        error: String,
    },

    /// Rewind checkpoints ready for the selector overlay.
    RewindCheckpointsReady {
        /// Available checkpoints in chronological order (oldest first).
        checkpoints: Vec<RewindCheckpointItem>,
    },
    /// Diff stats ready for a specific rewind checkpoint (lazy computation).
    DiffStatsReady {
        /// The turn number these stats apply to.
        turn_number: i32,
        /// The computed diff stats.
        stats: RewindDiffStats,
    },

    // ========== Summarize ==========
    /// Partial compaction (summarize from a user-selected turn) completed.
    SummarizeCompleted {
        /// The turn number from which summarization started.
        from_turn: i32,
        /// Tokens in the generated summary.
        summary_tokens: i32,
    },
    /// Partial compaction (summarize from a user-selected turn) failed.
    SummarizeFailed {
        /// Error message.
        error: String,
    },
}

/// Mode for a rewind operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RewindMode {
    /// Rewind both code changes and conversation history.
    CodeAndConversation,
    /// Rewind conversation only (keep file changes).
    ConversationOnly,
    /// Rewind code only (keep conversation history).
    CodeOnly,
}

/// Summary of an available checkpoint for the rewind selector overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindCheckpointItem {
    /// The turn number.
    pub turn_number: i32,
    /// Number of files modified in this turn.
    pub file_count: i32,
    /// Display text for the user message at this turn.
    pub user_message_preview: String,
    /// Whether a ghost commit (full working tree snapshot) is available.
    pub has_ghost_commit: bool,
    /// File paths modified in this turn (for diff preview).
    pub modified_files: Vec<String>,
    /// Cumulative diff stats for rewinding to this turn (computed lazily).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_stats: Option<RewindDiffStats>,
}

/// Line-level diff statistics for a rewind preview.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RewindDiffStats {
    /// Number of files that would change.
    pub files_changed: i32,
    /// Total lines that would be added.
    pub insertions: i32,
    /// Total lines that would be removed.
    pub deletions: i32,
}

/// Info about an available output style (for the picker overlay).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStyleItem {
    /// Style name.
    pub name: String,
    /// Source label (e.g., "built-in", "custom", "project", "plugin").
    pub source: String,
    /// Optional description.
    pub description: Option<String>,
}

/// Lightweight agent info for plugin agent events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAgentInfo {
    /// Agent name (display name).
    pub name: String,
    /// Agent type identifier (used in @agent-type mentions).
    pub agent_type: String,
    /// Short description of what the agent does.
    pub description: String,
}

/// Summary info for an installed plugin (for UI display).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSummaryInfo {
    /// Plugin name.
    pub name: String,
    /// Short description.
    pub description: String,
    /// Version string.
    pub version: String,
    /// Whether the plugin is currently enabled.
    pub enabled: bool,
    /// Installation scope (user/project/managed/flag).
    pub scope: String,
    /// Number of skills contributed.
    pub skills_count: i32,
    /// Number of hooks contributed.
    pub hooks_count: i32,
    /// Number of agents contributed.
    pub agents_count: i32,
}

/// Summary info for a known marketplace (for UI display).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceSummaryInfo {
    /// Marketplace name.
    pub name: String,
    /// Source type (github, git, url, etc.).
    pub source_type: String,
    /// Source URL or path.
    pub source: String,
    /// Whether auto-update is enabled.
    pub auto_update: bool,
    /// Number of plugins (0 if manifest not yet loaded).
    pub plugin_count: i32,
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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
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
        (*self).into()
    }
}

/// Progress information from a sub-agent.
///
/// Supports two-tier progress reporting:
/// - `summary`: Accumulated work summary (preserved across updates, like `reportToolProgress`)
/// - `activity`: Current transient activity (replaced on each update, like `updateTaskProgress`)
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
    /// Accumulated work summary (preserved across updates).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Current transient activity (replaced on each update).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
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
    /// After context compaction completes.
    PostCompact,
    /// A notification event (informational, no blocking).
    Notification,
    /// When a permission is requested.
    PermissionRequest,
    /// When an agent team teammate is about to go idle.
    TeammateIdle,
    /// When a task is being marked as completed.
    TaskCompleted,
    /// When configuration changes at runtime.
    ConfigChange,
    /// When a git worktree is created.
    WorktreeCreate,
    /// When a git worktree is removed.
    WorktreeRemove,
    /// Initial setup phase before session starts.
    ///
    /// Fired during initialization for hooks that need to run before
    /// the session is fully established.
    Setup,
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
            Self::PostCompact => "post_compact",
            Self::Notification => "notification",
            Self::PermissionRequest => "permission_request",
            Self::TeammateIdle => "teammate_idle",
            Self::TaskCompleted => "task_completed",
            Self::ConfigChange => "config_change",
            Self::WorktreeCreate => "worktree_create",
            Self::WorktreeRemove => "worktree_remove",
            Self::Setup => "setup",
        }
    }
}

impl std::fmt::Display for HookEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for HookEventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PreToolUse" | "pre_tool_use" => Ok(Self::PreToolUse),
            "PostToolUse" | "post_tool_use" => Ok(Self::PostToolUse),
            "PostToolUseFailure" | "post_tool_use_failure" => Ok(Self::PostToolUseFailure),
            "UserPromptSubmit" | "user_prompt_submit" => Ok(Self::UserPromptSubmit),
            "SessionStart" | "session_start" => Ok(Self::SessionStart),
            "SessionEnd" | "session_end" => Ok(Self::SessionEnd),
            "Stop" | "stop" => Ok(Self::Stop),
            "SubagentStart" | "subagent_start" => Ok(Self::SubagentStart),
            "SubagentStop" | "subagent_stop" => Ok(Self::SubagentStop),
            "PreCompact" | "pre_compact" => Ok(Self::PreCompact),
            "PostCompact" | "post_compact" => Ok(Self::PostCompact),
            "Notification" | "notification" => Ok(Self::Notification),
            "PermissionRequest" | "permission_request" => Ok(Self::PermissionRequest),
            "TeammateIdle" | "teammate_idle" => Ok(Self::TeammateIdle),
            "TaskCompleted" | "task_completed" => Ok(Self::TaskCompleted),
            "ConfigChange" | "config_change" => Ok(Self::ConfigChange),
            "WorktreeCreate" | "worktree_create" => Ok(Self::WorktreeCreate),
            "WorktreeRemove" | "worktree_remove" => Ok(Self::WorktreeRemove),
            "Setup" | "setup" => Ok(Self::Setup),
            other => Err(format!("unknown hook event type: {other}")),
        }
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
