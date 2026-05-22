//! Plugin command types.
//!
//! Commands are plugin-provided actions that can be invoked via slash commands
//! or other interfaces. They support shell commands, skill invocations, and
//! agent spawning.

use serde::Deserialize;
use serde::Serialize;

/// Default function for visible field.
fn default_true() -> bool {
    true
}

/// A command contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCommand {
    /// Command name (used as the slash command).
    pub name: String,

    /// Human-readable description.
    pub description: String,

    /// How the command is executed.
    pub handler: CommandHandler,

    /// Whether the command is visible in help/completions.
    #[serde(default = "default_true")]
    pub visible: bool,
}

/// Handler type for plugin commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandHandler {
    /// Execute a shell command.
    Shell {
        /// The command to execute.
        command: String,
        /// Optional timeout in seconds.
        #[serde(default)]
        timeout_sec: Option<i32>,
    },

    /// Invoke a skill.
    Skill {
        /// Name of the skill to invoke.
        skill_name: String,
    },

    /// Spawn an agent.
    Agent {
        /// Agent type to spawn.
        agent_type: String,
    },
}

#[cfg(test)]
#[path = "command.test.rs"]
mod tests;
