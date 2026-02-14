//! Skill command types.
//!
//! Defines the prompt-based skill commands and slash commands that users
//! can invoke. Each skill is represented as a [`SkillPromptCommand`] with
//! associated metadata and prompt content.

use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::path::PathBuf;

use crate::source::LoadedFrom;
use crate::source::SkillSource;

/// Execution context for a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillContext {
    /// Run in the main conversation context.
    #[default]
    Main,

    /// Fork a new agent context for execution.
    Fork,
}

impl fmt::Display for SkillContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Main => write!(f, "main"),
            Self::Fork => write!(f, "fork"),
        }
    }
}

/// A skill that injects a prompt into the conversation.
///
/// This is the primary representation of a loaded skill. The prompt text
/// is either read from a file (referenced in `SKILL.toml`) or specified
/// inline in the TOML metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPromptCommand {
    /// Unique skill name (used as the slash command identifier).
    pub name: String,

    /// Human-readable description shown in help/completion.
    pub description: String,

    /// Prompt text injected when the skill is invoked.
    pub prompt: String,

    /// Optional list of tools the skill is allowed to use.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,

    // -- Classification flags --
    /// Whether this skill can be invoked by the user as a `/command`.
    #[serde(default = "default_true")]
    pub user_invocable: bool,

    /// Whether the LLM is blocked from invoking this skill via the Skill tool.
    #[serde(default)]
    pub disable_model_invocation: bool,

    /// Computed from `!user_invocable`. Hidden skills don't appear in `/help`.
    #[serde(default)]
    pub is_hidden: bool,

    // -- Source tracking --
    /// Where the skill was loaded from.
    #[serde(default = "default_source")]
    pub source: SkillSource,

    /// Simplified source categorization.
    #[serde(default = "default_loaded_from")]
    pub loaded_from: LoadedFrom,

    // -- Execution config --
    /// Execution context: main conversation or forked agent.
    #[serde(default)]
    pub context: SkillContext,

    /// Agent type to use when `context = Fork`.
    #[serde(default)]
    pub agent: Option<String>,

    /// Model override (e.g., "sonnet", "opus", "haiku").
    #[serde(default)]
    pub model: Option<String>,

    /// Base directory of the skill (for relative path resolution).
    #[serde(default)]
    pub base_dir: Option<PathBuf>,

    // -- Metadata --
    /// Guidance for the LLM on when to invoke this skill.
    #[serde(default)]
    pub when_to_use: Option<String>,

    /// Usage hint shown in help output (e.g., "<pr-number>").
    #[serde(default)]
    pub argument_hint: Option<String>,

    /// Alternative command names for this skill.
    #[serde(default)]
    pub aliases: Vec<String>,

    /// Optional interface with hook definitions.
    /// Populated from SKILL.toml when hooks are defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface: Option<crate::interface::SkillInterface>,

    /// Command type classification.
    /// `Prompt` for regular skills, `LocalJsx` for bundled UI commands.
    #[serde(default = "default_command_type")]
    pub command_type: CommandType,
}

fn default_true() -> bool {
    true
}

fn default_command_type() -> CommandType {
    CommandType::Prompt
}

fn default_source() -> SkillSource {
    SkillSource::Bundled
}

fn default_loaded_from() -> LoadedFrom {
    LoadedFrom::Bundled
}

impl SkillPromptCommand {
    /// Returns `true` if this skill can be invoked by the user as a `/command`.
    pub fn is_user_invocable(&self) -> bool {
        self.user_invocable
    }

    /// Returns `true` if the LLM can invoke this skill via the Skill tool.
    ///
    /// Checks both `command_type` (must be `Prompt`) and `disable_model_invocation`.
    pub fn is_llm_invocable(&self) -> bool {
        self.command_type == CommandType::Prompt && !self.disable_model_invocation
    }

    /// Returns `true` if this skill should appear in `/help` and command lists.
    pub fn is_visible_in_help(&self) -> bool {
        !self.is_hidden
    }
}

impl fmt::Display for SkillPromptCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{} - {}", self.name, self.description)
    }
}

/// A slash command that can be invoked by the user.
///
/// Slash commands include both skill-based commands and system/plugin commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommand {
    /// Command name (without leading slash).
    pub name: String,

    /// Human-readable description.
    pub description: String,

    /// The type of command.
    pub command_type: CommandType,
}

impl fmt::Display for SlashCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.command_type {
            CommandType::Prompt => "prompt",
            CommandType::Local => "local",
            CommandType::LocalJsx => "local-jsx",
        };
        write!(f, "/{} [{}] - {}", self.name, kind, self.description)
    }
}

/// The type of a slash command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandType {
    /// A prompt-based skill loaded from SKILL.toml.
    Prompt,

    /// A built-in local command (e.g., /help, /clear).
    Local,

    /// A plugin-provided JSX command.
    LocalJsx,
}

impl fmt::Display for CommandType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prompt => write!(f, "prompt"),
            Self::Local => write!(f, "local"),
            Self::LocalJsx => write!(f, "local-jsx"),
        }
    }
}

#[cfg(test)]
#[path = "command.test.rs"]
mod tests;
