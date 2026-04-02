# coco-mcp — Crate Plan

TS source: `src/services/mcp/` (23 files, ~12K LOC)

## Dependencies

```
coco-mcp depends on:
  - coco-types   (Message, PermissionDecision, HookEventType)
  - coco-config  (Settings — MCP server configs from settings files)
  - coco-error
  - reqwest, tokio (HTTP, async)
  - serde, serde_json
  - utils/keyring-store (OAuth token secure storage)

coco-mcp does NOT depend on:
  - coco-tool    (no Tool trait — MCP tools are wrapped by coco-tools McpTool)
  - coco-inference (no LLM calls)
  - coco-query   (no agent loop)
  - any app/ crate
```

## Modules

```
coco-mcp/src/
  types.rs          # ConfigScope, Transport, McpServerConfig union, ConnectionState
  config.rs         # Multi-source config loading, dedup, policy filtering
  client.rs         # Client connection, transport selection, reconnection
  auth.rs           # OAuth flows, token storage/refresh, XAA token exchange
  auth/xaa.rs       # Cross-App Access token exchange, JWT grants
  auth/idp.rs       # IdP OIDC discovery, token caching
  auth/port.rs      # OAuth redirect URI, port finder
  naming.rs         # Server/tool name normalization, mcp__server__tool parsing
  transport.rs      # Transport abstractions (stdio, HTTP, SSE, WS, SDK)
  channel.rs        # Channel server gating, permission relay, message wrapping
  elicitation.rs    # URL/form elicitation request/result handling
  session.rs        # Session-level lifecycle (from useManageMCPConnections)
  registry.rs       # Official MCP server registry cache
  utils.rs          # Tool/command/resource filtering helpers
```

## Data Definitions

### Config Types (from `types.ts`)

```rust
pub enum ConfigScope {
    Local,       // .claude/mcp.json in project
    User,        // ~/.claude/mcp.json
    Project,     // .claude/ project config
    Dynamic,     // runtime-added
    Enterprise,  // managed enterprise config
    ClaudeAi,    // claude.ai organization servers
    Managed,     // enterprise/managed config file
}

pub enum McpTransport {
    Stdio,
    Sse,
    SseIde,
    Http,
    WebSocket,
    Sdk,
    ClaudeAiProxy,
}

/// Server config — union type with per-transport fields.
pub enum McpServerConfig {
    Stdio(McpStdioConfig),
    Sse(McpSseConfig),
    Http(McpHttpConfig),
    WebSocket(McpWsConfig),
    Sdk(McpSdkConfig),
    ClaudeAiProxy(McpClaudeAiProxyConfig),
}

pub struct McpStdioConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<PathBuf>,
}

pub struct McpSseConfig {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub oauth: Option<OAuthConfig>,
}

pub struct McpHttpConfig {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub oauth: Option<OAuthConfig>,
}

pub struct McpWsConfig {
    pub url: String,
    pub headers: HashMap<String, String>,
}

pub struct McpSdkConfig {
    pub name: String,
}

pub struct McpClaudeAiProxyConfig {
    pub url: String,
    pub server_id: String,
}

/// Config with scope metadata attached.
pub struct ScopedMcpServerConfig {
    pub config: McpServerConfig,
    pub scope: ConfigScope,
    pub plugin_source: Option<String>,
}
```

### Connection State (from `types.ts`)

```rust
pub enum McpConnectionState {
    Connected(ConnectedMcpServer),
    Failed { error: String },
    NeedsAuth { auth_url: Option<String> },
    Pending { reconnect_attempts: i32 },
    Disabled,
}

pub struct ConnectedMcpServer {
    pub name: String,
    pub client: McpClient,
    pub capabilities: McpCapabilities,
    pub instructions: Option<String>,
    pub tools: Vec<McpToolDefinition>,
    pub resources: Vec<McpResource>,
    pub commands: Vec<McpPrompt>,
}

pub struct McpToolDefinition {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

pub struct McpCapabilities {
    pub tools: bool,
    pub resources: bool,
    pub prompts: bool,
    pub channel: bool,           // experimental.claude/channel
    pub channel_permission: bool, // experimental.claude/channel/permission
}
```

### Tool Naming (from `mcpStringUtils.ts`, `normalization.ts`)

```rust
/// MCP tool name convention: mcp__<normalized_server>__<normalized_tool>
pub fn build_mcp_tool_name(server: &str, tool: &str) -> String;
pub fn parse_mcp_tool_name(name: &str) -> Option<McpToolInfo>;
pub fn normalize_name_for_mcp(name: &str) -> String;

pub struct McpToolInfo {
    pub server_name: String,
    pub tool_name: Option<String>,
}
```

### OAuth (from `auth.ts`)

```rust
pub struct OAuthConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
    pub scopes: Vec<String>,
}

pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub token_type: String,
}

/// OAuth client provider for MCP SDK.
/// Token storage via utils/keyring-store (platform keychain).
pub struct ClaudeAuthProvider {
    pub server_name: String,
    pub redirect_uri: String,
}
```

### Elicitation (from `elicitationHandler.ts`)

```rust
pub struct ElicitationRequest {
    pub server_name: String,
    pub request_id: String,
    pub mode: ElicitationMode,
    pub message: Option<String>,
}

pub enum ElicitationMode {
    Url { url: String },
    Form { schema: Value },
}

pub enum ElicitResult {
    Completed { data: Value },
    Cancelled,
    Timeout,
}
```

## Core Logic

### Config Loading Pipeline (from `config.ts`)

```rust
/// Load MCP configs from all sources, deduplicated.
/// Sources (in priority order): managed > enterprise > claudeai > project > user > local > dynamic
pub async fn get_all_mcp_configs(settings: &Settings) -> Result<AllMcpConfigs, McpError>;

pub struct AllMcpConfigs {
    pub configs: HashMap<String, ScopedMcpServerConfig>,
    pub errors: Vec<McpConfigError>,
}

/// Add/remove MCP server config to project or user scope.
pub async fn add_mcp_config(name: &str, config: McpServerConfig, scope: ConfigScope) -> Result<(), McpError>;
pub async fn remove_mcp_config(name: &str) -> Result<(), McpError>;

/// Check if server is user-disabled.
pub fn is_mcp_server_disabled(name: &str, settings: &Settings) -> bool;

/// Environment variable expansion: ${VAR}, ${VAR:-default}
pub fn expand_env_vars(s: &str) -> String;
```

### Client Connection (from `client.ts`)

```rust
/// Connect to an MCP server by name. Caches clients for reuse.
/// Selects transport based on config type (stdio/SSE/HTTP/WS/SDK).
pub async fn ensure_connected_client(
    name: &str,
    config: &ScopedMcpServerConfig,
    cancel: CancellationToken,
) -> Result<ConnectedMcpServer, McpError>;

/// Fetch tools, commands, and resources from a connected server.
pub async fn get_capabilities(
    client: &McpClient,
) -> Result<(Vec<McpToolDefinition>, Vec<McpPrompt>, Vec<McpResource>), McpError>;

/// Execute an MCP tool call with elicitation retry on auth failure.
pub async fn call_mcp_tool(
    client: &McpClient,
    server_name: &str,
    tool_name: &str,
    input: Value,
    cancel: CancellationToken,
) -> Result<McpToolResult, McpError>;

/// Transform raw MCP result to content blocks.
/// Handles: text, images (resize/downsample), binary (persist to cache).
/// Truncates at 100KB.
pub fn transform_mcp_result(raw: Value) -> Result<Vec<ContentBlock>, McpError>;
```

### OAuth Flow (from `auth.ts`)

```rust
/// Full OAuth 2.0 flow: discovery → PKCE → browser → callback → token exchange.
/// Starts local HTTP server on random port (49152-65535) for redirect.
pub async fn perform_mcp_oauth_flow(
    config: &OAuthConfig,
    server_name: &str,
) -> Result<OAuthTokens, McpError>;

/// Check and refresh OAuth token if expired.
pub async fn check_and_refresh_token(
    server_name: &str,
) -> Result<OAuthTokens, McpError>;

/// Revoke stored OAuth tokens for a server.
pub async fn revoke_server_tokens(server_name: &str) -> Result<(), McpError>;
```

### Session Lifecycle (from `useManageMCPConnections.ts`)

```rust
/// Manages MCP connections for a session.
/// - Connects to all configured servers on session start
/// - Handles reconnection with exponential backoff
/// - Syncs tools/resources/commands to AppState
/// - Listens for tool/resource list change notifications
pub struct McpSessionManager {
    connections: HashMap<String, McpConnectionState>,
    configs: HashMap<String, ScopedMcpServerConfig>,
}

impl McpSessionManager {
    pub async fn start(&mut self, settings: &Settings, cancel: CancellationToken);
    pub async fn reconnect(&mut self, server_name: &str);
    pub fn connected_tools(&self) -> Vec<McpToolDefinition>;
    pub fn connected_resources(&self) -> HashMap<String, Vec<McpResource>>;
    pub fn connection_state(&self, name: &str) -> Option<&McpConnectionState>;
}
```

### Channel Servers (from `channelNotification.ts`, `channelPermissions.ts`)

```rust
/// Channel servers relay messages + permission decisions via MCP notifications.
/// Tool permission dialog can race: local UI vs channel reply vs hook vs classifier.

/// Generate short permission request ID (5 chars, FNV-1a based).
pub fn short_request_id(tool_use_id: &str) -> String;

/// Check if server has channel capability.
pub fn is_channel_server(server: &ConnectedMcpServer) -> bool;

/// Check plugin allowlist (enterprise-gated).
pub fn is_channel_allowed(server_name: &str, plugin_source: Option<&str>) -> bool;
```

## TS File -> Module Mapping

| TS file | Rust module | LOC |
|---------|------------|-----|
| `types.ts` | `types.rs` | 258 |
| `config.ts` | `config.rs` | 1,578 |
| `client.ts` | `client.rs` | 3,348 |
| `auth.ts` | `auth.rs` | 2,465 |
| `xaa.ts` | `auth/xaa.rs` | 511 |
| `xaaIdpLogin.ts` | `auth/idp.rs` | 487 |
| `oauthPort.ts` | `auth/port.rs` | 78 |
| `elicitationHandler.ts` | `elicitation.rs` | 313 |
| `channelNotification.ts` | `channel.rs` | 316 |
| `channelPermissions.ts` | `channel.rs` | 240 |
| `channelAllowlist.ts` | `channel.rs` | 76 |
| `normalization.ts` | `naming.rs` | 23 |
| `mcpStringUtils.ts` | `naming.rs` | 106 |
| `headersHelper.ts` | `config.rs` | 138 |
| `envExpansion.ts` | `config.rs` | 38 |
| `utils.ts` | `utils.rs` | 575 |
| `officialRegistry.ts` | `registry.rs` | 72 |
| `claudeai.ts` | `config.rs` | 164 |
| `SdkControlTransport.ts` | `transport.rs` | 136 |
| `InProcessTransport.ts` | `transport.rs` | 63 |
| `useManageMCPConnections.ts` | `session.rs` | 1,141 |
| `vscodeSdkMcp.ts` | (coco-bridge) | 112 |
| `MCPConnectionManager.tsx` | (coco-tui) | ~100 |
