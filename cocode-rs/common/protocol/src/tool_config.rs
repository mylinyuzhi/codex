//! Tool execution configuration.
//!
//! Defines settings for tool execution concurrency and timeouts.

use serde::Deserialize;
use serde::Serialize;

/// Type of apply_patch tool to use.
///
/// This determines how the apply_patch tool is exposed to the model:
/// - `Function`: JSON function tool (default) - model provides structured JSON input
/// - `Freeform`: String-schema function tool - model outputs patch text directly
/// - `Shell`: No tool sent; model uses shell to invoke apply_patch via prompt instructions
///
/// Configured per-model via `ModelInfo.apply_patch_tool_type`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApplyPatchToolType {
    /// String-schema function tool (default, for GPT-5.2+, codex models).
    #[default]
    Freeform,
    /// JSON function tool with "input" parameter (for gpt-oss).
    Function,
    /// Shell-based, prompt instructions only (for GPT-5, o3, o4-mini).
    Shell,
}

/// Default maximum number of concurrent tool executions.
pub const DEFAULT_MAX_TOOL_CONCURRENCY: i32 = 10;

/// Default global fallback for maximum tool result size (400K characters).
///
/// The primary persistence threshold is the per-tool `Tool::max_result_size_chars()`
/// (e.g. 30K for Bash, 100K for Read). This constant serves as a global fallback
/// for tools that don't override `max_result_size_chars()`.
pub const DEFAULT_MAX_RESULT_SIZE: i32 = 400_000;

/// Default preview size for persisted large results (2K characters).
///
/// When a result exceeds `DEFAULT_MAX_RESULT_SIZE`, this many characters
/// from the start of the result are kept as a preview in the context.
pub const DEFAULT_RESULT_PREVIEW_SIZE: i32 = 2_000;

/// Tool execution configuration.
///
/// Controls how tools are executed, including concurrency limits and timeouts.
///
/// # Environment Variables
///
/// - `COCODE_MAX_TOOL_USE_CONCURRENCY`: Maximum concurrent tool executions (default: 10)
/// - `MCP_TOOL_TIMEOUT`: Timeout in milliseconds for MCP tool calls
///
/// # Example
///
/// ```json
/// {
///   "tool": {
///     "max_tool_concurrency": 5,
///     "mcp_tool_timeout": 30000
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ToolConfig {
    /// Maximum number of concurrent tool executions.
    #[serde(default = "default_max_tool_concurrency")]
    pub max_tool_concurrency: i32,

    /// Timeout in milliseconds for MCP tool calls.
    #[serde(default)]
    pub mcp_tool_timeout: Option<i32>,

    /// Global fallback for maximum tool result size (characters).
    ///
    /// The primary persistence threshold is the per-tool `Tool::max_result_size_chars()`.
    /// This field is kept as a global fallback configuration value.
    /// See also: `result_preview_size`, `enable_result_persistence`.
    #[serde(default = "default_max_result_size")]
    pub max_result_size: i32,

    /// Preview size for persisted large results.
    ///
    /// When a result exceeds `max_result_size`, this many characters
    /// from the start are kept as a preview in the context.
    #[serde(default = "default_result_preview_size")]
    pub result_preview_size: i32,

    /// Enable large result persistence (default: true).
    ///
    /// When enabled, tool results exceeding `max_result_size` are automatically
    /// persisted to disk with a preview kept in context.
    #[serde(default = "default_true")]
    pub enable_result_persistence: bool,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            max_tool_concurrency: DEFAULT_MAX_TOOL_CONCURRENCY,
            mcp_tool_timeout: None,
            max_result_size: DEFAULT_MAX_RESULT_SIZE,
            result_preview_size: DEFAULT_RESULT_PREVIEW_SIZE,
            enable_result_persistence: true,
        }
    }
}

fn default_max_tool_concurrency() -> i32 {
    DEFAULT_MAX_TOOL_CONCURRENCY
}

fn default_max_result_size() -> i32 {
    DEFAULT_MAX_RESULT_SIZE
}

fn default_result_preview_size() -> i32 {
    DEFAULT_RESULT_PREVIEW_SIZE
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
#[path = "tool_config.test.rs"]
mod tests;
