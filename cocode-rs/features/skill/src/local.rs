//! Local (built-in) command definitions.
//!
//! Local commands are slash commands that execute locally without going
//! through the LLM. They are registered as [`SlashCommand`] objects with
//! `CommandType::Local` and can be dispatched by both REPL and TUI.

use crate::command::CommandType;
use crate::command::SlashCommand;

/// A built-in local command definition.
///
/// Each local command has a name, description, and optional aliases.
/// The actual execution logic lives in the application layer (REPL/TUI),
/// which matches on the command name to perform the action.
#[derive(Debug, Clone)]
pub struct LocalCommandDef {
    /// Command name (without leading slash).
    pub name: &'static str,

    /// Human-readable description.
    pub description: &'static str,

    /// Alternative names for this command.
    pub aliases: &'static [&'static str],
}

impl LocalCommandDef {
    /// Convert to a [`SlashCommand`].
    pub fn to_slash_command(&self) -> SlashCommand {
        SlashCommand {
            name: self.name.to_string(),
            description: self.description.to_string(),
            command_type: CommandType::Local,
        }
    }
}

/// Returns the list of built-in local commands.
///
/// These commands are available in both REPL and TUI modes.
pub fn builtin_local_commands() -> &'static [LocalCommandDef] {
    &[
        LocalCommandDef {
            name: "help",
            description: "Show available commands and skills",
            aliases: &["h", "?"],
        },
        LocalCommandDef {
            name: "status",
            description: "Show session status",
            aliases: &[],
        },
        LocalCommandDef {
            name: "clear",
            description: "Clear the screen",
            aliases: &[],
        },
        LocalCommandDef {
            name: "model",
            description: "Switch the active model",
            aliases: &[],
        },
        LocalCommandDef {
            name: "compact",
            description: "Compact conversation context",
            aliases: &[],
        },
        LocalCommandDef {
            name: "skills",
            description: "List available skills",
            aliases: &[],
        },
        LocalCommandDef {
            name: "todos",
            description: "List current tasks",
            aliases: &["tasks"],
        },
        LocalCommandDef {
            name: "output-style",
            description: "Manage response output styles",
            aliases: &[],
        },
        LocalCommandDef {
            name: "exit",
            description: "Exit the session",
            aliases: &["quit", "q"],
        },
        LocalCommandDef {
            name: "cancel",
            description: "Cancel current operation",
            aliases: &[],
        },
    ]
}

/// Look up a local command by name or alias.
///
/// Returns the matching [`LocalCommandDef`] if found.
pub fn find_local_command(name: &str) -> Option<&'static LocalCommandDef> {
    builtin_local_commands()
        .iter()
        .find(|cmd| cmd.name == name || cmd.aliases.contains(&name))
}

#[cfg(test)]
#[path = "local.test.rs"]
mod tests;
