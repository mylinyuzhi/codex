//! MCP server loading from plugin directories.
//!
//! Loads MCP.toml files from plugin-specified MCP server directories.

use std::path::Path;

use tracing::debug;
use tracing::warn;
use walkdir::WalkDir;

use crate::contribution::PluginContribution;
use crate::mcp::McpServerConfig;

/// MCP server manifest filename.
pub const MCP_TOML: &str = "MCP.toml";

/// Load MCP server configurations from a directory.
///
/// Scans the directory for MCP.toml files and loads them into
/// PluginContribution::McpServer variants.
///
/// # Arguments
/// * `dir` - Directory to scan for MCP.toml files
/// * `plugin_name` - Name of the plugin providing these servers
///
/// # Example MCP.toml format:
/// ```toml
/// name = "filesystem"
/// description = "Provides file system access"
/// auto_start = true
///
/// [transport]
/// type = "stdio"
/// command = "npx"
/// args = ["-y", "@anthropic/mcp-server-filesystem"]
///
/// [env]
/// MCP_DEBUG = "true"
/// ```
pub fn load_mcp_servers_from_dir(dir: &Path, plugin_name: &str) -> Vec<PluginContribution> {
    if !dir.is_dir() {
        debug!(
            plugin = %plugin_name,
            path = %dir.display(),
            "MCP server path not found or not a directory"
        );
        return Vec::new();
    }

    let mut results = Vec::new();

    // Walk the directory looking for MCP.toml files
    for entry in WalkDir::new(dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir() {
            let mcp_path = entry.path().join(MCP_TOML);
            if mcp_path.is_file() {
                match load_mcp_server_from_file(&mcp_path, plugin_name) {
                    Ok(contrib) => results.push(contrib),
                    Err(e) => {
                        warn!(
                            plugin = %plugin_name,
                            path = %mcp_path.display(),
                            error = %e,
                            "Failed to load MCP server configuration"
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
        "Loaded MCP servers from plugin"
    );

    results
}

/// Load a single MCP server configuration from a TOML file.
///
/// Resolves variable substitution patterns (`${COCODE_PLUGIN_ROOT}`, `${env.VAR}`)
/// using the parent plugin directory as the plugin root.
fn load_mcp_server_from_file(path: &Path, plugin_name: &str) -> anyhow::Result<PluginContribution> {
    let content = std::fs::read_to_string(path)?;
    let mut config: McpServerConfig = toml::from_str(&content)?;

    // Resolve variables using the plugin directory as root.
    // The plugin root is the grandparent of the MCP.toml file (plugin_dir/mcp/server/MCP.toml).
    // Walk up to find the directory that contains PLUGIN.toml.
    let plugin_root =
        find_plugin_root(path).unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
    config.resolve_variables(&plugin_root, None);

    debug!(
        plugin = %plugin_name,
        server = %config.name,
        "Loaded MCP server configuration"
    );

    Ok(PluginContribution::McpServer {
        config,
        plugin_name: plugin_name.to_string(),
    })
}

/// Walk up from a path to find the nearest directory containing PLUGIN.toml.
fn find_plugin_root(from: &Path) -> Option<std::path::PathBuf> {
    let mut current = from.parent();
    while let Some(dir) = current {
        if dir.join("PLUGIN.toml").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

#[cfg(test)]
#[path = "mcp_loader.test.rs"]
mod tests;
