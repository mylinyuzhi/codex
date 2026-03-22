//! Command loading from plugin directories.

use std::path::Path;

use tracing::debug;

use crate::command::PluginCommand;
use crate::contribution::PluginContribution;
use crate::dir_scanner::scan_plugin_dir;

/// Command manifest filename.
pub const COMMAND_JSON: &str = "command.json";

/// Load command definitions from a directory.
pub fn load_commands_from_dir(dir: &Path, plugin_name: &str) -> Vec<PluginContribution> {
    scan_plugin_dir(
        dir,
        COMMAND_JSON,
        plugin_name,
        "Command",
        load_command_from_file,
    )
}

/// Load a single command definition from a JSON file.
fn load_command_from_file(path: &Path, plugin_name: &str) -> anyhow::Result<PluginContribution> {
    let content = std::fs::read_to_string(path)?;
    let command: PluginCommand = serde_json::from_str(&content)?;

    debug!(
        plugin = %plugin_name,
        command = %command.name,
        "Loaded command definition"
    );

    Ok(PluginContribution::Command {
        command,
        plugin_name: plugin_name.to_string(),
    })
}

#[cfg(test)]
#[path = "command_loader.test.rs"]
mod tests;
