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
///
/// TS: `Command.source` field. Variants mirror `LoadedFrom` in
/// `skills/loadSkillsDir.ts` plus `'builtin'` for hardcoded slash commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    /// On-disk skill directory (general SKILL.md catch-all).
    Skills,
    /// Plugin-provided skill or command.
    Plugin,
    /// Compiled-in bundled skill.
    Bundled,
    /// MCP-server-provided skill.
    Mcp,
    /// User-scope on-disk skill (`~/.coco/skills/`). TS: `userSettings`.
    User,
    /// Project-scope on-disk skill (`.claude/skills/`). TS: `projectSettings`.
    Project,
    /// Enterprise/policy-managed skill. TS: `policySettings`.
    Managed,
    /// Hardcoded built-in slash command (e.g. `/help`, `/clear`).
    /// TS: `source: 'builtin'`.
    Builtin,
    /// Legacy `.claude/commands/` flat-`.md` path. TS: `commands_DEPRECATED`.
    CommandsDeprecated,
}

impl CommandSource {
    /// Wire string used by TS Skill tool listing / analytics.
    pub fn as_str(self) -> &'static str {
        match self {
            CommandSource::Skills => "skills",
            CommandSource::Plugin => "plugin",
            CommandSource::Bundled => "bundled",
            CommandSource::Mcp => "mcp",
            CommandSource::User => "userSettings",
            CommandSource::Project => "projectSettings",
            CommandSource::Managed => "policySettings",
            CommandSource::Builtin => "builtin",
            CommandSource::CommandsDeprecated => "commands_DEPRECATED",
        }
    }
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

/// UI-facing projection of a slash command. The TUI receives a `Vec` of
/// these at startup (and again after `/reload-plugins`) so the
/// autocomplete popup and command palette can render and rank without
/// reaching into [`CommandBase`] every time.
///
/// Lives in `coco-types` (rather than `coco-tui`) so it can travel on a
/// [`crate::TuiOnlyEvent`] variant — events are the only path between
/// the agent driver and the TUI, and event payload types must be
/// foundation-layer.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommandInfo {
    /// Canonical command name without the leading `/`.
    pub name: String,
    /// Short description shown dimmed in the popup. `None` when the
    /// source command registered without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Alternate names that also match this command. Searched by the
    /// ranker so `/cls` finds `/clear` when `cls` is an alias.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Hint string rendered next to the description when the command
    /// takes arguments (e.g. `"<file>"` for `/add-dir`). Mirrors
    /// [`CommandBase::argument_hint`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
}
