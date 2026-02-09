//! Agent loading from plugin directories.
//!
//! Loads AGENT.toml files from plugin-specified agent directories.

use std::path::Path;

use cocode_subagent::AgentDefinition;
use tracing::debug;
use tracing::warn;
use walkdir::WalkDir;

use crate::contribution::PluginContribution;

/// Agent manifest filename.
pub const AGENT_TOML: &str = "AGENT.toml";

/// Load agent definitions from a directory.
///
/// Scans the directory for AGENT.toml files and loads them into
/// PluginContribution::Agent variants.
///
/// # Arguments
/// * `dir` - Directory to scan for AGENT.toml files
/// * `plugin_name` - Name of the plugin providing these agents
///
/// # Example AGENT.toml format:
/// ```toml
/// name = "code-review"
/// description = "Reviews code for quality"
/// agent_type = "code-review"
/// tools = ["Read", "Grep", "Glob"]
/// disallowed_tools = ["Write", "Edit"]
/// model = "claude-sonnet"  # Optional
/// max_turns = 20           # Optional
/// ```
pub fn load_agents_from_dir(dir: &Path, plugin_name: &str) -> Vec<PluginContribution> {
    if !dir.is_dir() {
        debug!(
            plugin = %plugin_name,
            path = %dir.display(),
            "Agent path not found or not a directory"
        );
        return Vec::new();
    }

    let mut results = Vec::new();

    // Walk the directory looking for AGENT.toml files
    for entry in WalkDir::new(dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir() {
            let agent_path = entry.path().join(AGENT_TOML);
            if agent_path.is_file() {
                match load_agent_from_file(&agent_path, plugin_name) {
                    Ok(contrib) => results.push(contrib),
                    Err(e) => {
                        warn!(
                            plugin = %plugin_name,
                            path = %agent_path.display(),
                            error = %e,
                            "Failed to load agent definition"
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
        "Loaded agents from plugin"
    );

    results
}

/// Load a single agent definition from a TOML file.
fn load_agent_from_file(path: &Path, plugin_name: &str) -> anyhow::Result<PluginContribution> {
    let content = std::fs::read_to_string(path)?;
    let definition: AgentDefinition = toml::from_str(&content)?;

    debug!(
        plugin = %plugin_name,
        agent = %definition.name,
        agent_type = %definition.agent_type,
        "Loaded agent definition"
    );

    Ok(PluginContribution::Agent {
        definition,
        plugin_name: plugin_name.to_string(),
    })
}

#[cfg(test)]
#[path = "agent_loader.test.rs"]
mod tests;
