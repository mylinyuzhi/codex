//! MCP (Model Context Protocol) server configuration types.
//!
//! These types define how plugins can contribute MCP servers. The actual
//! MCP client integration is deferred to the MCP client implementation.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

/// Default function for auto_start field.
fn default_true() -> bool {
    true
}

/// Configuration for an MCP server contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique name for this MCP server.
    pub name: String,

    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,

    /// Transport configuration.
    pub transport: McpTransport,

    /// Environment variables to set when starting the server.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Whether to automatically start this server.
    #[serde(default = "default_true")]
    pub auto_start: bool,
}

/// Transport configuration for MCP servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    /// Standard input/output transport (subprocess).
    Stdio {
        /// Command to execute.
        command: String,
        /// Command arguments.
        #[serde(default)]
        args: Vec<String>,
    },

    /// HTTP transport.
    Http {
        /// Server URL.
        url: String,
    },
}

impl McpServerConfig {
    /// Resolve variable substitution patterns in the config.
    ///
    /// Supported patterns:
    /// - `${COCODE_PLUGIN_ROOT}` → plugin install directory path
    /// - `${env.VAR_NAME}` → environment variable value
    /// - `${user_config.KEY}` → per-plugin user config value (requires config map)
    pub fn resolve_variables(
        &mut self,
        plugin_root: &Path,
        user_config: Option<&HashMap<String, serde_json::Value>>,
    ) {
        let root_str = plugin_root.to_string_lossy().to_string();

        let resolve = |s: &str| -> String { resolve_variable_string(s, &root_str, user_config) };

        // Resolve transport fields
        match &mut self.transport {
            McpTransport::Stdio { command, args } => {
                *command = resolve(command);
                for arg in args.iter_mut() {
                    *arg = resolve(arg);
                }
            }
            McpTransport::Http { url } => {
                *url = resolve(url);
            }
        }

        // Resolve env values
        let resolved_env: HashMap<String, String> = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), resolve(v)))
            .collect();
        self.env = resolved_env;
    }
}

/// Resolve variable patterns in a single string.
fn resolve_variable_string(
    s: &str,
    plugin_root: &str,
    user_config: Option<&HashMap<String, serde_json::Value>>,
) -> String {
    let mut result = s.to_string();

    // Replace ${COCODE_PLUGIN_ROOT}
    result = result.replace("${COCODE_PLUGIN_ROOT}", plugin_root);

    // Replace ${env.VAR_NAME} patterns
    while let Some(start) = result.find("${env.") {
        let Some(end) = result[start..].find('}') else {
            break;
        };
        let end = start + end;
        let var_name = &result[start + 6..end];
        let value = std::env::var(var_name).unwrap_or_default();
        result = format!("{}{}{}", &result[..start], value, &result[end + 1..]);
    }

    // Replace ${user_config.KEY} patterns
    if let Some(config) = user_config {
        while let Some(start) = result.find("${user_config.") {
            let Some(end) = result[start..].find('}') else {
                break;
            };
            let end = start + end;
            let key = &result[start + 14..end];
            let value = config
                .get(key)
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            result = format!("{}{}{}", &result[..start], value, &result[end + 1..]);
        }
    }

    result
}

#[cfg(test)]
#[path = "mcp.test.rs"]
mod tests;
