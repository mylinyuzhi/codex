//! Session configuration types used by SDK and future IDE clients.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// MCP server configuration for SDK clients.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpServerConfig {
    /// Subprocess-based MCP server (stdio transport).
    Stdio {
        /// Command to run.
        command: String,
        /// Command arguments.
        #[serde(default)]
        args: Vec<String>,
        /// Environment variables.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        env: Option<HashMap<String, String>>,
    },
    /// SSE-based MCP server.
    Sse {
        /// Server URL.
        url: String,
    },
    /// HTTP-based MCP server.
    Http {
        /// Server URL.
        url: String,
    },
}

/// Agent definition for SDK-provided custom agents.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentDefinitionConfig {
    /// Short description of when to use this agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// System prompt or instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Tool whitelist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// Tool blacklist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disallowed_tools: Option<Vec<String>>,
    /// Model override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Maximum turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,
    /// Whether the agent defaults to background execution.
    #[serde(default)]
    pub background: bool,
    /// Isolation mode for the agent's execution environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<AgentIsolationMode>,
    /// Memory scope for persistent agent memory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<AgentMemoryScope>,
    /// Skill names to load for this agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    /// MCP server name references required by this agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<String>>,
    /// Hook definitions scoped to this agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Vec<AgentHookConfig>>,
    /// Critical reminder prepended to the agent's prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical_reminder: Option<String>,
    /// Whether `prompt` replaces the entire system prompt (true) or appends (false).
    #[serde(default)]
    pub use_custom_prompt: bool,
    /// Display color hint (e.g., "cyan", "blue", "green", "orange").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Permission mode override (e.g., "default", "bypassPermissions").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// Whether to fork the parent conversation context to this agent.
    #[serde(default)]
    pub fork_context: bool,
}

/// Isolation mode for agent execution environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AgentIsolationMode {
    /// No isolation — agent shares parent's working directory.
    None,
    /// Git worktree isolation — agent runs in a detached worktree.
    Worktree,
}

/// Memory scope for persistent agent memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AgentMemoryScope {
    /// User-level memory.
    User,
    /// Project-level memory.
    Project,
    /// Local (gitignored) memory.
    Local,
}

/// A hook definition scoped to an agent.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentHookConfig {
    /// The event type (e.g., "PreToolUse", "PostToolUse", "Stop").
    pub event: String,
    /// Optional matcher pattern (e.g., tool name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    /// The command to execute.
    pub command: String,
    /// Optional timeout in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i32>,
}

/// Hook matcher for SDK-provided hooks.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HookMatcherConfig {
    /// Tool name pattern to match (glob-style).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Handler command.
    pub command: String,
    /// Handler arguments.
    #[serde(default)]
    pub args: Vec<String>,
}

/// SDK hook callback configuration.
///
/// Pre-registered at initialize time following the Claude Agent SDK pattern.
/// When a hook event fires, the server emits `ServerRequest::HookCallback`
/// with the callback_id, and the SDK client responds.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HookCallbackConfig {
    /// Unique callback identifier (UUID, pre-registered at init).
    pub callback_id: String,
    /// Hook event type (e.g., "PreToolUse", "PostToolUse").
    pub event: String,
    /// Tool name pattern to match (glob-style, None = all tools).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    /// Timeout in milliseconds for the callback response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<i32>,
}

/// Sandbox configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SandboxConfig {
    /// Sandbox mode.
    #[serde(default)]
    pub mode: SandboxMode,
    /// Whether to allow network access in sandbox.
    #[serde(default)]
    pub network_access: bool,
    /// Auto-allow Bash tool when sandboxed (avoids approval prompts).
    #[serde(default)]
    pub auto_allow_bash_if_sandboxed: bool,
    /// Commands excluded from sandbox restrictions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_commands: Vec<String>,
}

/// Sandbox execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    /// No sandboxing.
    #[default]
    None,
    /// Read-only filesystem access.
    ReadOnly,
    /// Strict sandboxing.
    Strict,
}

/// Thinking / reasoning configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ThinkingConfig {
    /// Thinking mode.
    pub mode: ThinkingMode,
    /// Maximum thinking tokens (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
}

/// Thinking mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    /// Model decides when to think.
    #[default]
    Adaptive,
    /// Always use extended thinking.
    Enabled,
    /// Never use extended thinking.
    Disabled,
}

/// System prompt configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SystemPromptConfig {
    /// Raw system prompt string.
    Raw(String),
    /// Structured system prompt with preset and optional append.
    Structured {
        /// Base preset to use.
        preset: String,
        /// Text to append to the preset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        append: Option<String>,
    },
}

/// Tools configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ToolsConfig {
    /// Explicit list of tool names.
    List(Vec<String>),
    /// Preset-based configuration.
    Preset {
        /// Preset name (e.g., "default").
        preset: String,
    },
}

/// Output format configuration for structured output.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutputFormatConfig {
    /// JSON Schema for the expected output.
    pub schema: Value,
}

// ---------------------------------------------------------------------------
// Hook input/output types (schema-derivable for multi-language codegen)
// ---------------------------------------------------------------------------

/// Input payload for `PreToolUse` hook callbacks.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PreToolUseHookInput {
    /// Name of the tool about to be executed.
    pub tool_name: String,
    /// Tool input parameters.
    #[serde(default)]
    pub tool_input: Value,
    /// Tool use identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
}

/// Input payload for `PostToolUse` hook callbacks.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PostToolUseHookInput {
    /// Name of the tool that was executed.
    pub tool_name: String,
    /// Tool input parameters.
    #[serde(default)]
    pub tool_input: Value,
    /// Tool output (if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<String>,
    /// Whether the tool execution failed.
    #[serde(default)]
    pub is_error: bool,
    /// Tool use identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
}

/// Output payload for hook callback responses.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HookCallbackOutput {
    /// What to do with the tool execution.
    pub behavior: HookBehavior,
    /// Message to include (for deny/error behaviors).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Modified tool input (for allow behavior with changes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Value>,
}

/// Hook callback behavior decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookBehavior {
    /// Allow the tool execution to proceed.
    Allow,
    /// Deny the tool execution.
    Deny,
    /// Report an error for this tool execution.
    Error,
}

// ---------------------------------------------------------------------------
// Additional hook input types (matching Claude Code SDK hook events)
// ---------------------------------------------------------------------------

/// Input payload for `Stop` hook callbacks (session/turn ending).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StopHookInput {
    /// Reason for stopping.
    pub stop_reason: String,
}

/// Input payload for `SubagentStart` hook callbacks.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubagentStartHookInput {
    /// Agent type being started.
    pub agent_type: String,
    /// Prompt being sent to the agent.
    pub prompt: String,
    /// Agent identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

/// Input payload for `SubagentStop` hook callbacks.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubagentStopHookInput {
    /// Agent type that stopped.
    pub agent_type: String,
    /// Agent identifier.
    pub agent_id: String,
    /// Agent output (if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// Input payload for `UserPromptSubmit` hook callbacks.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UserPromptSubmitHookInput {
    /// The user's prompt text.
    pub prompt: String,
}

/// Input payload for `Notification` hook callbacks.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotificationHookInput {
    /// Notification type.
    pub notification_type: String,
    /// Notification payload.
    #[serde(default)]
    pub payload: Value,
}
