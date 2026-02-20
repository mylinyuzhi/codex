//! LSP server loading from plugin directories.
//!
//! Loads `.lsp.json` files from plugin-specified LSP server directories.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use tracing::warn;
use walkdir::WalkDir;

use crate::contribution::PluginContribution;

/// LSP server manifest filename.
pub const LSP_JSON: &str = ".lsp.json";

/// Configuration for an LSP server contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    /// Unique name for this LSP server (e.g., "rust-analyzer").
    pub name: String,

    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,

    /// Language IDs this server handles (e.g., ["rust", "toml"]).
    #[serde(default)]
    pub languages: Vec<String>,

    /// Command to start the server.
    pub command: String,

    /// Command arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables to set when starting the server.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// File patterns that trigger this server (e.g., ["*.rs", "Cargo.toml"]).
    #[serde(default)]
    pub file_patterns: Vec<String>,

    /// Root URI markers — files that indicate the project root (e.g., ["Cargo.toml"]).
    #[serde(default)]
    pub root_markers: Vec<String>,
}

/// Load LSP server configurations from a directory.
///
/// Scans the directory for `.lsp.json` files and loads them into
/// `PluginContribution::LspServer` variants.
pub fn load_lsp_servers_from_dir(dir: &Path, plugin_name: &str) -> Vec<PluginContribution> {
    if !dir.is_dir() {
        debug!(
            plugin = %plugin_name,
            path = %dir.display(),
            "LSP server path not found or not a directory"
        );
        return Vec::new();
    }

    let mut results = Vec::new();

    for entry in WalkDir::new(dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if entry.file_type().is_dir() {
            let lsp_path = entry.path().join(LSP_JSON);
            if lsp_path.is_file() {
                match load_lsp_server_from_file(&lsp_path, plugin_name) {
                    Ok(contrib) => results.push(contrib),
                    Err(e) => {
                        warn!(
                            plugin = %plugin_name,
                            path = %lsp_path.display(),
                            error = %e,
                            "Failed to load LSP server configuration"
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
        "Loaded LSP servers from plugin"
    );

    results
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
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_lsp_config() {
        let json = r#"{
            "name": "rust-analyzer",
            "description": "Rust language server",
            "languages": ["rust"],
            "command": "rust-analyzer",
            "args": [],
            "file_patterns": ["*.rs", "Cargo.toml"],
            "root_markers": ["Cargo.toml"]
        }"#;

        let config: LspServerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.name, "rust-analyzer");
        assert_eq!(config.languages, vec!["rust"]);
        assert_eq!(config.command, "rust-analyzer");
    }

    #[test]
    fn test_load_lsp_from_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let lsp_dir = tmp.path().join("lsp");
        fs::create_dir_all(&lsp_dir).unwrap();
        fs::write(
            lsp_dir.join(LSP_JSON),
            r#"{
                "name": "test-lsp",
                "languages": ["test"],
                "command": "test-lsp-server"
            }"#,
        )
        .unwrap();

        let results = load_lsp_servers_from_dir(tmp.path(), "test-plugin");
        assert_eq!(results.len(), 1);
        assert!(results[0].is_lsp_server());
        assert_eq!(results[0].name(), "test-lsp");
    }

    #[test]
    fn test_load_lsp_from_file_array() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".lsp.json");
        fs::write(
            &path,
            r#"[
                {"name": "lsp1", "languages": ["a"], "command": "cmd1"},
                {"name": "lsp2", "languages": ["b"], "command": "cmd2"}
            ]"#,
        )
        .unwrap();

        let results = load_lsp_servers_from_file(&path, "test-plugin");
        assert_eq!(results.len(), 2);
    }
}
