//! Agent loading from plugin directories.

use std::path::Path;

use cocode_subagent::AgentDefinition;
use tracing::debug;

use crate::contribution::PluginContribution;
use crate::dir_scanner::scan_plugin_dir;

/// Agent manifest filename.
pub const AGENT_JSON: &str = "agent.json";

/// Load agent definitions from a directory.
pub fn load_agents_from_dir(dir: &Path, plugin_name: &str) -> Vec<PluginContribution> {
    scan_plugin_dir(dir, AGENT_JSON, plugin_name, "Agent", load_agent_from_file)
}

/// Load a single agent definition from a JSON file.
fn load_agent_from_file(path: &Path, plugin_name: &str) -> anyhow::Result<PluginContribution> {
    let content = std::fs::read_to_string(path)?;
    let definition: AgentDefinition = serde_json::from_str(&content)?;

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
