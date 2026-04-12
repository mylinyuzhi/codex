use serde::Deserialize;
use serde::Serialize;

use crate::ThinkingLevel;

/// Where a command can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandAvailability {
    ClaudeAi,
    Console,
}

/// How a command was loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    Skills,
    Plugin,
    Bundled,
    Mcp,
}

/// Common fields for all commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandBase {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub availability: Vec<CommandAvailability>,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    #[serde(default)]
    pub user_invocable: bool,
    #[serde(default)]
    pub is_sensitive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_from: Option<CommandSource>,
    /// Safety classification for remote/bridge mode filtering.
    #[serde(default)]
    pub safety: CommandSafety,
    /// Whether non-interactive (SDK/headless) mode is supported.
    #[serde(default)]
    pub supports_non_interactive: bool,
}

/// Safety classification for remote/bridge filtering.
///
/// TS: `REMOTE_SAFE_COMMANDS` and `BRIDGE_SAFE_COMMANDS` sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandSafety {
    /// Safe in all contexts (remote, bridge, local).
    AlwaysSafe,
    /// Safe for bridge mode (mobile/web clients) but not remote.
    BridgeSafe,
    /// Only safe when running locally in the terminal.
    #[default]
    LocalOnly,
}

impl CommandSafety {
    /// Whether a command with this safety level is allowed in the given context.
    pub fn permits(self, required: CommandSafety) -> bool {
        match required {
            CommandSafety::LocalOnly => true,
            CommandSafety::BridgeSafe => {
                matches!(self, CommandSafety::AlwaysSafe | CommandSafety::BridgeSafe)
            }
            CommandSafety::AlwaysSafe => self == CommandSafety::AlwaysSafe,
        }
    }
}

/// Command execution type.
///
/// TS: `type: 'prompt' | 'local' | 'local-jsx'`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandType {
    /// Expands to a model prompt (skills). TS: `type: 'prompt'`.
    Prompt(PromptCommandData),
    /// Executes locally, returns text. TS: `type: 'local'`.
    Local(LocalCommandData),
    /// Opens a TUI overlay/modal. TS: `type: 'local-jsx'`.
    LocalOverlay(LocalCommandData),
}

/// Context for prompt command execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum CommandContext {
    #[default]
    Inline,
    Fork,
}

/// Data for a prompt-type command (sent to LLM).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptCommandData {
    pub progress_message: String,
    #[serde(default)]
    pub content_length: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub context: CommandContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
    /// Hook config — deserialized by coco-hooks, not typed here (avoids L1→L4 dep).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<serde_json::Value>,
}

/// Data for a local-type command (executed locally).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalCommandData {
    /// Module path or identifier for the local command handler.
    pub handler: String,
}
