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

/// Sandbox configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SandboxConfig {
    /// Sandbox mode.
    #[serde(default)]
    pub mode: SandboxMode,
    /// Whether to allow network access in sandbox.
    #[serde(default)]
    pub network_access: bool,
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
