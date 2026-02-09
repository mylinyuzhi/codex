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
fn load_mcp_server_from_file(path: &Path, plugin_name: &str) -> anyhow::Result<PluginContribution> {
    let content = std::fs::read_to_string(path)?;
    let config: McpServerConfig = toml::from_str(&content)?;

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

#[cfg(test)]
#[path = "mcp_loader.test.rs"]
mod tests;
