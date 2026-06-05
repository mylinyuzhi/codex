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
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use coco_error::BoxedError;
use coco_mcp_types::CallToolRequestParams;
use coco_mcp_types::CallToolResult;
use coco_mcp_types::InitializeResult;
use coco_mcp_types::JSONRPC_VERSION;
use coco_mcp_types::ListToolsResult;
use coco_mcp_types::ReadResourceRequestParams;
use coco_mcp_types::ReadResourceResult;
use coco_rmcp_client::McpAuthStatus;
use coco_rmcp_client::OAuthCredentialsStoreMode;
use coco_rmcp_client::RmcpClient;
use coco_rmcp_client::SendElicitation;
use coco_rmcp_client::determine_streamable_http_auth_status;
use coco_rmcp_client::perform_oauth_login_return_url;
use tokio::sync::RwLock;
use tracing::info;
use tracing::warn;

use crate::naming;
use crate::types::ConnectedMcpServer;
use crate::types::McpCapabilities;
use crate::types::McpConnectionState;
use crate::types::McpOAuthConfig;
use crate::types::McpPrompt;
use crate::types::McpResource;
use crate::types::McpServerConfig;
use crate::types::McpToolDefinition;
use crate::types::ScopedMcpServerConfig;

/// Default MCP tool call timeout (ms) — ~27.8 hours like TS.
const DEFAULT_TOOL_TIMEOUT_MS: u64 = 100_000_000;

/// Default MCP initialization timeout.
const DEFAULT_INIT_TIMEOUT: Duration = Duration::from_secs(60);

pub type SdkRouteFuture =
    Pin<Box<dyn Future<Output = std::result::Result<serde_json::Value, String>> + Send>>;
pub type SdkRouteMessage =
    Arc<dyn Fn(String, serde_json::Value) -> SdkRouteFuture + Send + Sync + 'static>;

/// MCP connection manager — manages lifecycle of all MCP server connections.
///
/// Internally delegates to `coco_rmcp_client::RmcpClient` for actual MCP
/// protocol communication (stdio, HTTP/SSE, OAuth, session recovery).
#[derive(Clone)]
pub struct McpConnectionManager {
    configs: HashMap<String, ScopedMcpServerConfig>,
    connections: Arc<RwLock<HashMap<String, McpConnectionState>>>,
    /// Holds the underlying rmcp clients, keyed by server name.
    rmcp_clients: Arc<RwLock<HashMap<String, Arc<RmcpClient>>>>,
    sdk_route_message: Option<SdkRouteMessage>,
    /// Monotonic counter for inner JSON-RPC `id` values used when
    /// bridging messages through `mcp/routeMessage`. Per-manager so two
    /// SDK MCP servers (or two concurrent `tools/call` invocations) get
    /// distinct ids. Wrapped in `Arc` so `Clone` impl shares the counter
    /// across cloned managers — they cooperate rather than restarting
    /// from 0 each clone.
    sdk_route_next_id: Arc<AtomicI64>,
    tool_timeout_ms: u64,
    config_home: PathBuf,
}

impl McpConnectionManager {
    pub fn new(config_home: PathBuf) -> Self {
        Self {
            configs: HashMap::new(),
            connections: Arc::new(RwLock::new(HashMap::new())),
            rmcp_clients: Arc::new(RwLock::new(HashMap::new())),
            sdk_route_message: None,
            sdk_route_next_id: Arc::new(AtomicI64::new(0)),
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
    ///
    /// Also seeds `connections[name] = Pending` so a concurrent
    /// `mcp/status` query before `connect()` runs reports the server
    /// as `pending` (TS-canonical) rather than `disconnected`.
    /// `connect()` overwrites the entry on success / failure.
    ///
    /// Uses `try_write` because:
    ///
    /// 1. `register_server` may run inside an async context (e.g.
    ///    the `mcp/setServers` handler) — `blocking_write` would
    ///    panic the runtime.
    /// 2. The only concurrent contender for the connections lock is
    ///    `connect()`, which sets its own `Pending` state at the
    ///    start of its critical section
    ///    (`McpConnectionManager::connect`). If we lose the race for
    ///    the lock, `connect()` is already writing `Pending` — the
    ///    wire status is correct either way.
    ///
    /// So a missed `try_write` is **observably equivalent** to a
    /// successful one. No silent regression.
    pub fn register_server(&mut self, config: ScopedMcpServerConfig) {
        let name = config.name.clone();
        info!(server = %name, "registering MCP server");
        self.configs.insert(name.clone(), config);
        if let Ok(mut conns) = self.connections.try_write() {
            conns.entry(name).or_insert(McpConnectionState::Pending {
                reconnect_attempts: 0,
            });
        }
    }

    /// Register multiple server configurations.
    pub fn register_all(&mut self, configs: Vec<ScopedMcpServerConfig>) {
        for config in configs {
            self.register_server(config);
        }
    }

    /// Install the control-channel router used by SDK-hosted MCP servers.
    ///
    /// SDK MCP servers run in the SDK client process. The manager still owns
    /// lifecycle and tool catalog state, but JSON-RPC messages are forwarded
    /// through this callback instead of through a child process or HTTP
    /// transport.
    pub fn set_sdk_route_message(&mut self, route: SdkRouteMessage) {
        self.sdk_route_message = Some(route);
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
            McpServerConfig::Sse(sse) => {
                ensure_xaa_tokens(server_name, &sse.url, sse.oauth.as_ref(), &self.config_home)
                    .await?;
                RmcpClient::new_streamable_http_client(
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
                })?
            }
            McpServerConfig::Http(http) => {
                ensure_xaa_tokens(
                    server_name,
                    &http.url,
                    http.oauth.as_ref(),
                    &self.config_home,
                )
                .await?;
                RmcpClient::new_streamable_http_client(
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
                })?
            }
            McpServerConfig::Sdk(sdk) => {
                return self.do_connect_sdk(server_name, sdk).await;
            }
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

        // Discover resources and prompts when the server advertises them —
        // mirrors the TS connect fan-out (tools/list + resources/list +
        // prompts/list). Prompts surface as MCP slash-commands; resources back
        // the List/Read MCP resource tools.
        let resources = if capabilities.resources {
            fetch_resources(&client, server_name).await
        } else {
            Vec::new()
        };
        let commands = if capabilities.prompts {
            fetch_prompts(&client, server_name).await
        } else {
            Vec::new()
        };

        info!(
            server = %server_name,
            tools = tools.len(),
            resources = resources.len(),
            prompts = commands.len(),
            "MCP server connected and initialized"
        );

        Ok(ConnectedMcpServer {
            name: server_name.to_string(),
            capabilities,
            instructions: init_result.instructions,
            tools,
            resources,
            commands,
        })
    }

    /// Call an MCP tool on a connected server.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, McpClientError> {
        if let Some(config) = self.configs.get(server_name)
            && matches!(config.config, McpServerConfig::Sdk(_))
        {
            return self.call_sdk_tool(server_name, tool_name, arguments).await;
        }

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

    async fn do_connect_sdk(
        &self,
        server_name: &str,
        sdk: &crate::types::McpSdkConfig,
    ) -> Result<ConnectedMcpServer, McpClientError> {
        let route = self
            .sdk_route_message
            .as_ref()
            .cloned()
            .ok_or(McpClientError::UnsupportedTransport)?;

        let init = route_sdk_jsonrpc(
            &route,
            server_name,
            self.sdk_route_next_id.fetch_add(1, Ordering::Relaxed),
            "initialize",
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "coco",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )
        .await?;
        let init_result: InitializeResult = parse_sdk_jsonrpc_result(init)?;

        // MCP notifications have no result. Older SDK-side shims may still
        // return an ack; either way the notification should not block connect.
        let _ = route(
            server_name.to_string(),
            serde_json::json!({
                "jsonrpc": JSONRPC_VERSION,
                "method": "notifications/initialized",
            }),
        )
        .await;

        let tools_response = route_sdk_jsonrpc(
            &route,
            server_name,
            self.sdk_route_next_id.fetch_add(1, Ordering::Relaxed),
            "tools/list",
            serde_json::json!({}),
        )
        .await?;
        let tools_result: ListToolsResult = parse_sdk_jsonrpc_result(tools_response)?;
        let tools = tools_result
            .tools
            .into_iter()
            .map(|t| McpToolDefinition {
                name: t.name,
                description: t.description,
                input_schema: serde_json::to_value(t.input_schema).unwrap_or_default(),
            })
            .collect::<Vec<_>>();

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
            sdk_name = %sdk.name,
            tools = tools.len(),
            "SDK MCP server connected and initialized"
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

    async fn call_sdk_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, McpClientError> {
        let route = self
            .sdk_route_message
            .as_ref()
            .cloned()
            .ok_or(McpClientError::UnsupportedTransport)?;
        let response = route_sdk_jsonrpc(
            &route,
            server_name,
            self.sdk_route_next_id.fetch_add(1, Ordering::Relaxed),
            "tools/call",
            serde_json::to_value(CallToolRequestParams {
                name: tool_name.to_string(),
                arguments,
            })
            .map_err(|e| McpClientError::ToolCallFailed {
                message: format!("serialize SDK MCP tools/call: {e}"),
            })?,
        )
        .await?;
        parse_sdk_jsonrpc_result(response)
    }

    /// Read an MCP resource from a connected server.
    pub async fn read_resource(
        &self,
        server_name: &str,
        resource_uri: &str,
    ) -> Result<ReadResourceResult, McpClientError> {
        let clients = self.rmcp_clients.read().await;
        let client = clients
            .get(server_name)
            .ok_or_else(|| McpClientError::ServerNotFound {
                name: server_name.to_string(),
            })?;

        let timeout = Duration::from_millis(self.tool_timeout_ms);
        client
            .read_resource(
                ReadResourceRequestParams {
                    uri: resource_uri.to_string(),
                },
                Some(timeout),
            )
            .await
            .map_err(|e| McpClientError::ToolCallFailed {
                message: format!("resource read failed: {e}"),
            })
    }

    /// Start or refresh OAuth authentication for a server.
    ///
    /// For OAuth-capable HTTP/SSE servers without stored tokens this
    /// returns the authorization URL immediately and reconnects in the
    /// background after the local callback completes.
    pub async fn authenticate(
        &self,
        server_name: &str,
        send_elicitation: SendElicitation,
    ) -> Result<String, McpClientError> {
        let config = self.configs.get(server_name).cloned().ok_or_else(|| {
            McpClientError::ServerNotFound {
                name: server_name.to_string(),
            }
        })?;
        let Some((url, headers)) = oauth_login_target(&config.config) else {
            return Ok(format!(
                "MCP server '{server_name}' does not use OAuth authentication."
            ));
        };

        let status = determine_streamable_http_auth_status(
            server_name,
            &url,
            /*bearer_token_env_var*/ None,
            Some(headers.clone()),
            /*env_http_headers*/ None,
            OAuthCredentialsStoreMode::Auto,
            &self.config_home,
        )
        .await
        .map_err(|e| McpClientError::ToolCallFailed {
            message: format!("failed to determine MCP auth status: {e}"),
        })?;

        match status {
            McpAuthStatus::Unsupported => Ok(format!(
                "MCP server '{server_name}' does not support OAuth authentication."
            )),
            McpAuthStatus::BearerToken => Ok(format!(
                "MCP server '{server_name}' is configured with bearer-token authentication; no OAuth login is needed."
            )),
            McpAuthStatus::OAuth => {
                self.spawn_reconnect(server_name.to_string(), send_elicitation);
                Ok(format!(
                    "OAuth credentials are already available for MCP server '{server_name}'. Reconnecting in the background."
                ))
            }
            McpAuthStatus::NotLoggedIn => {
                let handle = perform_oauth_login_return_url(
                    server_name,
                    &url,
                    OAuthCredentialsStoreMode::Auto,
                    Some(headers),
                    /*env_http_headers*/ None,
                    &[],
                    /*timeout_secs*/ None,
                    /*callback_port*/ None,
                    self.config_home.clone(),
                )
                .await
                .map_err(|e| McpClientError::ToolCallFailed {
                    message: format!("failed to start MCP OAuth login: {e}"),
                })?;
                let authorization_url = handle.authorization_url().to_string();
                self.spawn_reconnect_after_oauth(
                    server_name.to_string(),
                    handle,
                    send_elicitation,
                    authorization_url.clone(),
                );
                Ok(format!(
                    "Authentication started for MCP server '{server_name}'. Open this URL in your browser to continue:\n{authorization_url}\nThe server will reconnect automatically after OAuth completes."
                ))
            }
        }
    }

    fn spawn_reconnect(&self, server_name: String, send_elicitation: SendElicitation) {
        let manager = self.clone();
        tokio::spawn(async move {
            if let Err(error) = manager.connect(&server_name, send_elicitation).await {
                warn!(server = %server_name, error = %error, "MCP reconnect after auth failed");
            }
        });
    }

    fn spawn_reconnect_after_oauth(
        &self,
        server_name: String,
        handle: coco_rmcp_client::OauthLoginHandle,
        send_elicitation: SendElicitation,
        authorization_url: String,
    ) {
        let manager = self.clone();
        tokio::spawn(async move {
            if let Err(error) = handle.wait().await {
                warn!(server = %server_name, error = %error, "MCP OAuth login failed");
                let mut conns = manager.connections.write().await;
                conns.insert(
                    server_name,
                    McpConnectionState::NeedsAuth {
                        auth_url: Some(authorization_url),
                    },
                );
                return;
            }
            if let Err(error) = manager.connect(&server_name, send_elicitation).await {
                warn!(server = %server_name, error = %error, "MCP reconnect after OAuth login failed");
            }
        });
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
    ) -> Result<tokio::sync::broadcast::Receiver<crate::config_watcher::McpConfigChanged>, BoxedError>
    {
        crate::config_watcher::watch_mcp_configs(
            &self.config_home,
            project_root.map(PathBuf::as_path),
        )
    }
}

/// List a connected server's resources, mapping into [`McpResource`].
/// A failure is logged and treated as "no resources" — it must not abort
/// connect (TS does the same via `Promise.all` with per-fetch catch).
async fn fetch_resources(client: &RmcpClient, server_name: &str) -> Vec<McpResource> {
    match client
        .list_resources(None, Some(DEFAULT_INIT_TIMEOUT))
        .await
    {
        Ok(result) => result
            .resources
            .into_iter()
            .map(|r| McpResource {
                uri: r.uri,
                name: r.name,
                description: r.description,
                mime_type: r.mime_type,
            })
            .collect(),
        Err(e) => {
            warn!(server = %server_name, "failed to list MCP resources: {e}");
            Vec::new()
        }
    }
}

/// List a connected server's prompts, mapping into [`McpPrompt`] (surfaced as
/// MCP slash-commands). Failures are logged and treated as "no prompts".
async fn fetch_prompts(client: &RmcpClient, server_name: &str) -> Vec<McpPrompt> {
    match client.list_prompts(None, Some(DEFAULT_INIT_TIMEOUT)).await {
        Ok(result) => result
            .prompts
            .into_iter()
            .map(|p| McpPrompt {
                name: p.name,
                description: p.description,
            })
            .collect(),
        Err(e) => {
            warn!(server = %server_name, "failed to list MCP prompts: {e}");
            Vec::new()
        }
    }
}

async fn ensure_xaa_tokens(
    server_name: &str,
    url: &str,
    oauth: Option<&McpOAuthConfig>,
    config_home: &std::path::Path,
) -> Result<(), McpClientError> {
    let Some(oauth) = oauth else {
        return Ok(());
    };
    let Some(xaa) = &oauth.xaa else {
        return Ok(());
    };

    if coco_rmcp_client::has_valid_oauth_tokens(
        server_name,
        url,
        OAuthCredentialsStoreMode::Auto,
        config_home,
    )
    .map_err(|error| McpClientError::SpawnFailed {
        message: format!("failed to inspect stored OAuth credentials: {error}"),
    })? {
        info!(
            server = %server_name,
            "stored OAuth credentials found; skipping XAA exchange"
        );
        return Ok(());
    }

    let client_id = required_xaa_field(
        "oauth.clientId or oauth.xaa.clientId",
        oauth.client_id.as_ref().or(xaa.client_id.as_ref()),
    )?;
    let config = crate::xaa::XaaConfig {
        client_id: client_id.to_string(),
        client_secret: required_xaa_field("oauth.xaa.clientSecret", xaa.client_secret.as_ref())?
            .to_string(),
        idp_client_id: required_xaa_field("oauth.xaa.idpClientId", xaa.idp_client_id.as_ref())?
            .to_string(),
        idp_client_secret: xaa.idp_client_secret.clone(),
        idp_id_token: required_xaa_field("oauth.xaa.idpIdToken", xaa.idp_id_token.as_ref())?
            .to_string(),
        idp_token_endpoint: required_xaa_field(
            "oauth.xaa.idpTokenEndpoint",
            xaa.idp_token_endpoint.as_ref(),
        )?
        .to_string(),
        scope: xaa.scope.clone(),
    };

    let http_client = reqwest::Client::new();
    let result = crate::xaa::perform_cross_app_access(&http_client, url, &config)
        .await
        .map_err(|error| McpClientError::SpawnFailed {
            message: format!("XAA authentication failed: {error}"),
        })?;
    if !result.token_type.is_empty() && !result.token_type.eq_ignore_ascii_case("bearer") {
        warn!(
            server = %server_name,
            token_type = %result.token_type,
            "XAA returned non-bearer token type; persisting as OAuth bearer token"
        );
    }
    coco_rmcp_client::save_oauth_access_token(coco_rmcp_client::OAuthAccessTokenSave {
        server_name,
        url,
        client_id: &config.client_id,
        access_token: result.access_token,
        refresh_token: result.refresh_token,
        expires_in: result.expires_in,
        scopes: result.scope,
        store_mode: OAuthCredentialsStoreMode::Auto,
        config_home,
    })
    .map_err(|error| McpClientError::SpawnFailed {
        message: format!("failed to persist XAA credentials: {error}"),
    })
}

fn required_xaa_field<'a>(
    field: &str,
    value: Option<&'a String>,
) -> Result<&'a str, McpClientError> {
    value
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| McpClientError::SpawnFailed {
            message: format!("XAA config missing required field {field}"),
        })
}

fn oauth_login_target(config: &McpServerConfig) -> Option<(String, HashMap<String, String>)> {
    match config {
        McpServerConfig::Sse(sse) => Some((sse.url.clone(), sse.headers.clone())),
        McpServerConfig::Http(http) => Some((http.url.clone(), http.headers.clone())),
        McpServerConfig::Stdio(_)
        | McpServerConfig::WebSocket(_)
        | McpServerConfig::Sdk(_)
        | McpServerConfig::ClaudeAiProxy(_) => None,
    }
}

async fn route_sdk_jsonrpc(
    route: &SdkRouteMessage,
    server_name: &str,
    id: i64,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, McpClientError> {
    route(
        server_name.to_string(),
        serde_json::json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id,
            "method": method,
            "params": params,
        }),
    )
    .await
    .map_err(|message| McpClientError::ToolCallFailed { message })
}

fn parse_sdk_jsonrpc_result<T>(message: serde_json::Value) -> Result<T, McpClientError>
where
    T: serde::de::DeserializeOwned,
{
    if let Some(error) = message.get("error") {
        return Err(McpClientError::ToolCallFailed {
            message: format!("SDK MCP returned error: {error}"),
        });
    }
    let result = message
        .get("result")
        .cloned()
        .ok_or_else(|| McpClientError::ToolCallFailed {
            message: format!("SDK MCP response missing result: {message}"),
        })?;
    serde_json::from_value(result).map_err(|e| McpClientError::ToolCallFailed {
        message: format!("parse SDK MCP response: {e}"),
    })
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

// `McpClientError` keeps its `thiserror` shape (many existing call sites
// construct variants directly via `Self::Variant { .. }`); we layer the
// `coco-error` traits on top so callers can match on `StatusCode` and
// drive retry / classification without the mass-rewrite that a full
// snafu migration would require.
impl coco_error::StackError for McpClientError {
    fn debug_fmt(&self, layer: usize, buf: &mut Vec<String>) {
        buf.push(format!("{layer}: {self}"));
    }

    fn next(&self) -> Option<&dyn coco_error::StackError> {
        None
    }
}

impl coco_error::ErrorExt for McpClientError {
    fn status_code(&self) -> coco_error::StatusCode {
        use coco_error::StatusCode;
        match self {
            Self::ServerNotFound { .. } => StatusCode::ProviderNotFound,
            Self::SpawnFailed { .. } => StatusCode::ConnectionFailed,
            Self::UnsupportedTransport => StatusCode::Unsupported,
            Self::SessionExpired => StatusCode::AuthenticationFailed,
            Self::AuthRequired { .. } => StatusCode::AuthenticationFailed,
            Self::ToolCallFailed { .. } => StatusCode::Internal,
            Self::ToolCallTimeout => StatusCode::Timeout,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
#[path = "client.test.rs"]
mod tests;
