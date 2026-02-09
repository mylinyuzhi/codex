//! MCP (Model Context Protocol) server configuration types.
//!
//! These types define how plugins can contribute MCP servers. The actual
//! MCP client integration is deferred to the MCP client implementation.

use std::collections::HashMap;

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

#[cfg(test)]
#[path = "mcp.test.rs"]
mod tests;
