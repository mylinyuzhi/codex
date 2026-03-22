//! LSP server loading from plugin directories.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use tracing::warn;

use crate::contribution::PluginContribution;
use crate::dir_scanner::scan_plugin_dir;

/// LSP server manifest filename.
pub const LSP_JSON: &str = ".lsp.json";

/// Configuration for an LSP server contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    /// Unique name for this LSP server (e.g., "rust-analyzer").
    pub name: String,

    #[serde(default)]
    pub description: Option<String>,

    /// Language IDs this server handles (e.g., ["rust", "toml"]).
    #[serde(default)]
    pub languages: Vec<String>,

    /// Command to start the server.
    pub command: String,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub env: HashMap<String, String>,

    /// File patterns that trigger this server (e.g., ["*.rs", "Cargo.toml"]).
    #[serde(default)]
    pub file_patterns: Vec<String>,

    /// Root URI markers — files that indicate the project root.
    #[serde(default)]
    pub root_markers: Vec<String>,
}

/// Load LSP server configurations from a directory.
pub fn load_lsp_servers_from_dir(dir: &Path, plugin_name: &str) -> Vec<PluginContribution> {
    scan_plugin_dir(
        dir,
        LSP_JSON,
        plugin_name,
        "LSP server",
        load_lsp_server_from_file,
    )
}

/// Load LSP server configurations from a single `.lsp.json` file.
///
/// The file can contain either a single config object or an array.
pub fn load_lsp_servers_from_file(path: &Path, plugin_name: &str) -> Vec<PluginContribution> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            // Try array first, then single object
            if let Ok(configs) = serde_json::from_str::<Vec<LspServerConfig>>(&content) {
                configs
                    .into_iter()
                    .map(|config| PluginContribution::LspServer {
                        config,
                        plugin_name: plugin_name.to_string(),
                    })
                    .collect()
            } else if let Ok(config) = serde_json::from_str::<LspServerConfig>(&content) {
                vec![PluginContribution::LspServer {
                    config,
                    plugin_name: plugin_name.to_string(),
                }]
            } else {
                warn!(
                    plugin = %plugin_name,
                    path = %path.display(),
                    "Failed to parse LSP server configuration"
                );
                Vec::new()
            }
        }
        Err(e) => {
            warn!(
                plugin = %plugin_name,
                path = %path.display(),
                error = %e,
                "Failed to read LSP server configuration"
            );
            Vec::new()
        }
    }
}

/// Load a single LSP server config from a JSON file.
fn load_lsp_server_from_file(path: &Path, plugin_name: &str) -> anyhow::Result<PluginContribution> {
    let content = std::fs::read_to_string(path)?;
    let config: LspServerConfig = serde_json::from_str(&content)?;

    debug!(
        plugin = %plugin_name,
        server = %config.name,
        "Loaded LSP server configuration"
    );

    Ok(PluginContribution::LspServer {
        config,
        plugin_name: plugin_name.to_string(),
    })
}

#[cfg(test)]
#[path = "lsp_loader.test.rs"]
mod tests;
