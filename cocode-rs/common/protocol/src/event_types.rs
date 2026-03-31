//! Shared event supporting types.
//!
//! These types are used
//! across multiple crates (core, tools, message, TUI, app-server). They
//! are independent of any specific event enum.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use strum::Display;
use strum::IntoStaticStr;

// ============================================================================
// Token & Result Types
// ============================================================================

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

// ============================================================================
// Tool & Execution Types
// ============================================================================

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

// ============================================================================
// Sandbox Types
// ============================================================================

/// Type of sandbox access being requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxAccessType {
    /// Network access (outbound HTTP, etc.)
    Network,
    /// File system write access outside sandbox.
    FileSystem,
    /// Process execution.
    ProcessExec,
}

impl SandboxAccessType {
    /// Get a human-readable label for this access type.
    pub fn label(&self) -> &'static str {
        match self {
            SandboxAccessType::Network => "Network Access",
            SandboxAccessType::FileSystem => "File System Access",
            SandboxAccessType::ProcessExec => "Process Execution",
        }
    }
}

// ============================================================================
// Rewind Types
// ============================================================================

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

// ============================================================================
// Agent & Task Types
// ============================================================================

/// Progress information from a sub-agent.
///
/// Supports two-tier progress reporting:
/// - `summary`: Accumulated work summary (preserved across updates, like `reportToolProgress`)
/// - `activity`: Current transient activity (replaced on each update, like `updateTaskProgress`)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

// ============================================================================
// Error & Retry Types
// ============================================================================

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

// ============================================================================
// MCP Types
// ============================================================================

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

// ============================================================================
// Hook Types
// ============================================================================

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
    Setup,
    /// When an MCP server sends an elicitation request (user input).
    Elicitation,
    /// After an elicitation has been answered.
    ElicitationResult,
    /// When instruction files (CLAUDE.md) are loaded or reloaded.
    InstructionsLoaded,
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
            Self::Elicitation => "elicitation",
            Self::ElicitationResult => "elicitation_result",
            Self::InstructionsLoaded => "instructions_loaded",
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
            "Elicitation" | "elicitation" => Ok(Self::Elicitation),
            "ElicitationResult" | "elicitation_result" => Ok(Self::ElicitationResult),
            "InstructionsLoaded" | "instructions_loaded" => Ok(Self::InstructionsLoaded),
            other => Err(format!("unknown hook event type: {other}")),
        }
    }
}

// ============================================================================
// Stream Types
// ============================================================================

/// Raw SSE event from the stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawStreamEvent {
    /// Event type.
    pub event_type: String,
    /// Event data (JSON).
    pub data: Value,
}

/// A tombstoned message (marked for removal).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TombstonedMessage {
    /// Message role.
    pub role: String,
    /// Message content (summary or placeholder).
    pub content: String,
}

// ============================================================================
// Compaction Types
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

// ============================================================================
// Plugin UI Types (used by TuiEvent)
// ============================================================================

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

// ============================================================================
// Rewind UI Types (used by TuiEvent)
// ============================================================================

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

#[cfg(test)]
#[path = "event_types.test.rs"]
mod tests;
