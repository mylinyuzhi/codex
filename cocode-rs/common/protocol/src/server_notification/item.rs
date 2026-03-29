//! Thread item types — the canonical representation of operations performed
//! by the agent (messages, tool calls, file changes, etc.).
//!
//! Follows the codex-rs `exec_events.rs` Thread/Turn/Item model.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// ThreadItem (top-level envelope)
// ---------------------------------------------------------------------------

/// A discrete operation within a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ThreadItem {
    /// Unique identifier for this item.
    pub id: String,
    /// The item payload.
    #[serde(flatten)]
    pub details: ThreadItemDetails,
}

/// Typed payloads for each item kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadItemDetails {
    /// Assistant text response.
    AgentMessage(AgentMessageItem),
    /// Extended thinking / reasoning output.
    Reasoning(ReasoningItem),
    /// Shell command execution (Bash tool).
    CommandExecution(CommandExecutionItem),
    /// File changes (Edit / Write / apply-patch tools).
    FileChange(FileChangeItem),
    /// MCP tool invocation.
    McpToolCall(McpToolCallItem),
    /// Web search or web fetch.
    WebSearch(WebSearchItem),
    /// Sub-agent spawned via Agent/Task tool.
    Subagent(SubagentItem),
    /// Catch-all for other built-in tools (Read, Glob, Grep, etc.).
    ToolCall(GenericToolCallItem),
    /// Non-fatal error surfaced as an item.
    Error(ErrorItem),
}

// ---------------------------------------------------------------------------
// ItemStatus (shared lifecycle)
// ---------------------------------------------------------------------------

/// Lifecycle status of a thread item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    /// Operation is running.
    #[default]
    InProgress,
    /// Operation finished successfully.
    Completed,
    /// Operation failed.
    Failed,
    /// Tool was declined by user or permission system.
    Declined,
}

// ---------------------------------------------------------------------------
// Item detail types
// ---------------------------------------------------------------------------

/// Assistant text response (accumulated from `TextDelta` events).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AgentMessageItem {
    /// Accumulated text so far.
    pub text: String,
}

/// Extended thinking output (accumulated from `ThinkingDelta` events).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReasoningItem {
    /// Accumulated reasoning text.
    pub text: String,
}

/// A shell command executed by the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CommandExecutionItem {
    /// The command string.
    pub command: String,
    /// Combined stdout + stderr output.
    pub aggregated_output: String,
    /// Process exit code (None while running).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Current status.
    pub status: ItemStatus,
}

/// A set of file changes produced by the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileChangeItem {
    /// Individual file changes.
    pub changes: Vec<FileChange>,
    /// Current status.
    pub status: ItemStatus,
}

/// A single file change within a `FileChangeItem`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileChange {
    /// File path (relative or absolute).
    pub path: String,
    /// Kind of change.
    pub kind: FileChangeKind,
}

/// Kind of file modification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    /// New file created.
    Add,
    /// Existing file deleted.
    Delete,
    /// Existing file modified.
    Update,
}

/// An MCP tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct McpToolCallItem {
    /// MCP server name.
    pub server: String,
    /// Tool name.
    pub tool: String,
    /// Tool arguments.
    #[serde(default)]
    pub arguments: Value,
    /// Result content (if completed successfully).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<McpToolCallResult>,
    /// Error info (if failed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<McpToolCallError>,
    /// Current status.
    pub status: ItemStatus,
}

/// Successful result from an MCP tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct McpToolCallResult {
    /// Content blocks returned by the MCP server.
    pub content: Vec<Value>,
    /// Structured content (if provided by MCP server).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
}

/// Error from a failed MCP tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct McpToolCallError {
    /// Error message.
    pub message: String,
}

/// A web search or web fetch operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WebSearchItem {
    /// Search query or URL.
    pub query: String,
    /// Current status.
    pub status: ItemStatus,
}

/// A sub-agent spawned via the Agent/Task tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SubagentItem {
    /// Agent identifier.
    pub agent_id: String,
    /// Agent type (e.g., "general-purpose", "Explore").
    pub agent_type: String,
    /// Short description of the agent's task.
    pub description: String,
    /// Whether the agent is running in background.
    #[serde(default)]
    pub is_background: bool,
    /// Agent result (set on completion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Current status.
    pub status: ItemStatus,
}

/// A generic built-in tool call (Read, Glob, Grep, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GenericToolCallItem {
    /// Tool name.
    pub tool: String,
    /// Tool input (JSON).
    #[serde(default)]
    pub input: Value,
    /// Tool output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Whether the tool returned an error.
    #[serde(default)]
    pub is_error: bool,
    /// Current status.
    pub status: ItemStatus,
}

/// A non-fatal error surfaced as a thread item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ErrorItem {
    /// Error message.
    pub message: String,
}
