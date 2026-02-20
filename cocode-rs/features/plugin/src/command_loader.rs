//! Command loading from plugin directories.
//!
//! Loads command.json files from plugin-specified command directories.

use std::path::Path;

use tracing::debug;
use tracing::warn;
use walkdir::WalkDir;

use crate::command::PluginCommand;
use crate::contribution::PluginContribution;

/// Command manifest filename.
pub const COMMAND_JSON: &str = "command.json";

/// Load command definitions from a directory.
///
/// Scans the directory for command.json files and loads them into
/// PluginContribution::Command variants.
///
/// # Arguments
/// * `dir` - Directory to scan for command.json files
/// * `plugin_name` - Name of the plugin providing these commands
///
/// # Example command.json format:
/// ```json
/// {
///   "name": "build",
///   "description": "Build the project",
///   "visible": true,
///   "handler": {
///     "type": "shell",
///     "command": "cargo build",
///     "timeout_sec": 300
///   }
/// }
/// ```
pub fn load_commands_from_dir(dir: &Path, plugin_name: &str) -> Vec<PluginContribution> {
    if !dir.is_dir() {
        debug!(
            plugin = %plugin_name,
            path = %dir.display(),
            "Command path not found or not a directory"
        );
        return Vec::new();
    }

    let mut results = Vec::new();

    // Walk the directory looking for command.json files
    for entry in WalkDir::new(dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if entry.file_type().is_dir() {
            let command_path = entry.path().join(COMMAND_JSON);
            if command_path.is_file() {
                match load_command_from_file(&command_path, plugin_name) {
                    Ok(contrib) => results.push(contrib),
                    Err(e) => {
                        warn!(
                            plugin = %plugin_name,
                            path = %command_path.display(),
                            error = %e,
                            "Failed to load command definition"
                        );
                    }
                }
            }
        }
    }

    debug!(
        plugin = %plugin_name,
        path = %dir.display(),
        count = results.len(),
        "Loaded commands from plugin"
    );

    results
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
