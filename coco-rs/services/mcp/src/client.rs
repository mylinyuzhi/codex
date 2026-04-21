//! MCP client lifecycle management.
//!
//! TS: services/mcp/client.ts (117KB) — connection management, tool discovery,
//! resource listing, transport selection.
//!
//! Wraps `coco-rmcp-client::RmcpClient` which provides actual MCP protocol
//! communication via the `rmcp` SDK (stdio + HTTP/SSE transports, OAuth,
//! session recovery).

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use coco_mcp_types::CallToolResult;
use coco_rmcp_client::OAuthCredentialsStoreMode;
use coco_rmcp_client::RmcpClient;
use coco_rmcp_client::SendElicitation;
use tokio::sync::RwLock;
use tracing::info;
use tracing::warn;

use crate::naming;
use crate::types::ConnectedMcpServer;
use crate::types::McpCapabilities;
use crate::types::McpConnectionState;
use crate::types::McpServerConfig;
use crate::types::McpToolDefinition;
use crate::types::ScopedMcpServerConfig;

/// Default MCP tool call timeout (ms) — ~27.8 hours like TS.
const DEFAULT_TOOL_TIMEOUT_MS: u64 = 100_000_000;

/// Default MCP initialization timeout.
const DEFAULT_INIT_TIMEOUT: Duration = Duration::from_secs(60);

/// MCP connection manager — manages lifecycle of all MCP server connections.
///
/// Internally delegates to `coco_rmcp_client::RmcpClient` for actual MCP
/// protocol communication (stdio, HTTP/SSE, OAuth, session recovery).
pub struct McpConnectionManager {
    configs: HashMap<String, ScopedMcpServerConfig>,
    connections: Arc<RwLock<HashMap<String, McpConnectionState>>>,
    /// Holds the underlying rmcp clients, keyed by server name.
    rmcp_clients: Arc<RwLock<HashMap<String, Arc<RmcpClient>>>>,
    tool_timeout_ms: u64,
    config_home: PathBuf,
}

impl McpConnectionManager {
    pub fn new(config_home: PathBuf) -> Self {
        Self {
            configs: HashMap::new(),
            connections: Arc::new(RwLock::new(HashMap::new())),
            rmcp_clients: Arc::new(RwLock::new(HashMap::new())),
            tool_timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
            config_home,
        }
    }

    pub fn new_with_runtime_config(
        config_home: PathBuf,
        config: &coco_config::McpRuntimeConfig,
    ) -> Self {
        let mut manager = Self::new(config_home);
        if let Some(tool_timeout_ms) = config.tool_timeout_ms {
            manager.tool_timeout_ms = tool_timeout_ms.max(1) as u64;
        }
        manager
    }

    /// Register a server configuration.
    pub fn register_server(&mut self, config: ScopedMcpServerConfig) {
        info!(server = %config.name, "registering MCP server");
        self.configs.insert(config.name.clone(), config);
    }

    /// Register multiple server configurations.
    pub fn register_all(&mut self, configs: Vec<ScopedMcpServerConfig>) {
        for config in configs {
            self.register_server(config);
        }
    }

    /// Connect to a server by name using `rmcp` SDK.
    pub async fn connect(
        &self,
        server_name: &str,
        send_elicitation: SendElicitation,
    ) -> Result<(), McpClientError> {
        let config =
            self.configs
                .get(server_name)
                .ok_or_else(|| McpClientError::ServerNotFound {
                    name: server_name.to_string(),
                })?;

        info!(server = %server_name, transport = ?config.config, "connecting to MCP server");

        // Set state to Pending
        {
            let mut conns = self.connections.write().await;
            conns.insert(
                server_name.to_string(),
                McpConnectionState::Pending {
                    reconnect_attempts: 0,
                },
            );
        }

        let result = self
            .do_connect(server_name, &config.config, send_elicitation)
            .await;

        match result {
            Ok(connected) => {
                let mut conns = self.connections.write().await;
                conns.insert(
                    server_name.to_string(),
                    McpConnectionState::Connected(connected),
                );
                Ok(())
            }
            Err(e) => {
                let mut conns = self.connections.write().await;
                conns.insert(
                    server_name.to_string(),
                    McpConnectionState::Failed {
                        error: e.to_string(),
                    },
                );
                Err(e)
            }
        }
    }

    /// Create rmcp client and initialize based on transport type.
    async fn do_connect(
        &self,
        server_name: &str,
        config: &McpServerConfig,
        send_elicitation: SendElicitation,
    ) -> Result<ConnectedMcpServer, McpClientError> {
        let client = match config {
            McpServerConfig::Stdio(stdio) => {
                let program = OsString::from(&stdio.command);
                let args: Vec<OsString> = stdio.args.iter().map(OsString::from).collect();
                let env = if stdio.env.is_empty() {
                    None
                } else {
                    Some(stdio.env.clone())
                };
                RmcpClient::new_stdio_client(
                    program,
                    args,
                    env,
                    &[],
                    stdio.cwd.as_ref().map(PathBuf::from),
                )
                .await
                .map_err(|e| McpClientError::SpawnFailed {
                    message: format!("stdio spawn failed: {e}"),
                })?
            }
            McpServerConfig::Sse(sse) => RmcpClient::new_streamable_http_client(
                server_name,
                &sse.url,
                /*bearer_token*/ None,
                Some(sse.headers.clone()),
                /*env_http_headers*/ None,
                OAuthCredentialsStoreMode::Auto,
                self.config_home.clone(),
            )
            .await
            .map_err(|e| McpClientError::SpawnFailed {
                message: format!("SSE connect failed: {e}"),
            })?,
            McpServerConfig::Http(http) => RmcpClient::new_streamable_http_client(
                server_name,
                &http.url,
                /*bearer_token*/ None,
                Some(http.headers.clone()),
                /*env_http_headers*/ None,
                OAuthCredentialsStoreMode::Auto,
                self.config_home.clone(),
            )
            .await
            .map_err(|e| McpClientError::SpawnFailed {
                message: format!("HTTP connect failed: {e}"),
            })?,
            _ => {
                return Err(McpClientError::UnsupportedTransport);
            }
        };

        // Initialize MCP handshake
        let init_params = coco_mcp_types::InitializeRequestParams {
            capabilities: coco_mcp_types::ClientCapabilities {
                experimental: None,
                roots: None,
                sampling: None,
                elicitation: None,
            },
            client_info: coco_mcp_types::Implementation {
                name: "coco".to_string(),
                title: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                user_agent: None,
            },
            protocol_version: "2024-11-05".to_string(),
        };

        let init_result = client
            .initialize(init_params, Some(DEFAULT_INIT_TIMEOUT), send_elicitation)
            .await
            .map_err(|e| McpClientError::SpawnFailed {
                message: format!("MCP initialization failed: {e}"),
            })?;

        // Discover tools
        let tools_result = client.list_tools(None, Some(DEFAULT_INIT_TIMEOUT)).await;
        let tools: Vec<McpToolDefinition> = match tools_result {
            Ok(result) => result
                .tools
                .into_iter()
                .map(|t| McpToolDefinition {
                    name: t.name,
                    description: t.description,
                    input_schema: serde_json::to_value(t.input_schema).unwrap_or_default(),
                })
                .collect(),
            Err(e) => {
                warn!(server = %server_name, "failed to list tools: {e}");
                Vec::new()
            }
        };

        let client = Arc::new(client);
        {
            let mut clients = self.rmcp_clients.write().await;
            clients.insert(server_name.to_string(), Arc::clone(&client));
        }

        let caps = &init_result.capabilities;
        let capabilities = McpCapabilities {
            tools: !tools.is_empty(),
            resources: caps.resources.is_some(),
            prompts: caps.prompts.is_some(),
            channel: false,
            channel_permission: false,
        };

        info!(
            server = %server_name,
            tools = tools.len(),
            "MCP server connected and initialized"
        );

        Ok(ConnectedMcpServer {
            name: server_name.to_string(),
            capabilities,
            instructions: init_result.instructions,
            tools,
            resources: Vec::new(),
            commands: Vec::new(),
        })
    }

    /// Call an MCP tool on a connected server.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, McpClientError> {
        let clients = self.rmcp_clients.read().await;
        let client = clients
            .get(server_name)
            .ok_or_else(|| McpClientError::ServerNotFound {
                name: server_name.to_string(),
            })?;

        let timeout = Duration::from_millis(self.tool_timeout_ms);
        client
            .call_tool(tool_name.to_string(), arguments, Some(timeout))
            .await
            .map_err(|e| McpClientError::ToolCallFailed {
                message: e.to_string(),
            })
    }

    /// Call an MCP tool using its full wire name (mcp__server__tool).
    pub async fn call_tool_by_wire_name(
        &self,
        wire_name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, McpClientError> {
        let (server, tool) =
            naming::parse_mcp_tool_id(wire_name).ok_or_else(|| McpClientError::ToolCallFailed {
                message: format!("invalid MCP tool name: {wire_name}"),
            })?;
        self.call_tool(&server, &tool, arguments).await
    }

    /// Get the current state of a server connection.
    /// Return the names of every registered server config, regardless
    /// of connection state. Used by external code (e.g. the SDK
    /// `mcp/status` handler) to enumerate servers whose state can
    /// then be queried via [`Self::get_state`].
    pub fn registered_server_names(&self) -> Vec<String> {
        self.configs.keys().cloned().collect()
    }

    pub async fn get_state(&self, server_name: &str) -> Option<McpConnectionState> {
        let conns = self.connections.read().await;
        conns.get(server_name).cloned()
    }

    /// Get all connected servers.
    pub async fn connected_servers(&self) -> Vec<ConnectedMcpServer> {
        let conns = self.connections.read().await;
        conns
            .values()
            .filter_map(|state| {
                if let McpConnectionState::Connected(server) = state {
                    Some(server.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all tools from all connected servers.
    pub async fn all_tools(&self) -> Vec<(String, McpToolDefinition)> {
        let servers = self.connected_servers().await;
        let mut tools = Vec::new();
        for server in &servers {
            for tool in &server.tools {
                tools.push((server.name.clone(), tool.clone()));
            }
        }
        tools
    }

    /// Disconnect a specific server.
    pub async fn disconnect(&self, server_name: &str) {
        let mut conns = self.connections.write().await;
        conns.remove(server_name);
        let mut clients = self.rmcp_clients.write().await;
        clients.remove(server_name);
        info!(server = %server_name, "disconnected MCP server");
    }

    /// Disconnect all servers.
    pub async fn disconnect_all(&self) {
        let mut conns = self.connections.write().await;
        let count = conns.len();
        conns.clear();
        let mut clients = self.rmcp_clients.write().await;
        clients.clear();
        info!("disconnected {count} MCP servers");
    }

    /// Get the tool call timeout in milliseconds.
    pub fn tool_timeout_ms(&self) -> u64 {
        self.tool_timeout_ms
    }

    /// Start watching MCP config files for changes.
    ///
    /// Returns a receiver that emits events when `.mcp.json` files change.
    /// Caller should spawn a task to handle events (e.g. reload configs,
    /// reconnect servers).
    pub fn start_config_watcher(
        &self,
        project_root: Option<&PathBuf>,
    ) -> anyhow::Result<tokio::sync::broadcast::Receiver<crate::config_watcher::McpConfigChanged>>
    {
        crate::config_watcher::watch_mcp_configs(&self.config_home, project_root)
    }
}

/// Truncate a tool description to the maximum length.
pub fn truncate_tool_description(description: &str) -> String {
    const MAX_LEN: usize = 2048;
    if description.len() <= MAX_LEN {
        description.to_string()
    } else {
        format!("{}...(truncated)", &description[..MAX_LEN])
    }
}

/// MCP client errors.
#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    #[error("MCP server not found: {name}")]
    ServerNotFound { name: String },
    #[error("failed to spawn MCP server: {message}")]
    SpawnFailed { message: String },
    #[error("unsupported transport type")]
    UnsupportedTransport,
    #[error("MCP session expired")]
    SessionExpired,
    #[error("MCP authentication required")]
    AuthRequired { auth_url: Option<String> },
    #[error("MCP tool call failed: {message}")]
    ToolCallFailed { message: String },
    #[error("MCP tool call timed out")]
    ToolCallTimeout,
}

#[cfg(test)]
#[path = "client.test.rs"]
mod tests;
