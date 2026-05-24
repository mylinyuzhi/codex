# coco-mcp

MCP server lifecycle, config, auth, discovery, naming, channel permissions. Delegates wire protocol to `coco-rmcp-client` via the `rmcp` SDK.

## TS Source
- `services/mcp/MCPConnectionManager.tsx` — connection manager (Rust: `client.rs`)
- `services/mcp/config.ts` — config loading + scopes
- `services/mcp/types.ts` — core types
- `services/mcp/client.ts`, `services/mcp/InProcessTransport.ts`, `services/mcp/SdkControlTransport.ts` — transports
- `services/mcp/auth.ts`, `services/mcp/oauthPort.ts` — OAuth
- `services/mcp/channelAllowlist.ts`, `services/mcp/channelPermissions.ts`, `services/mcp/channelNotification.ts` — channel permission relay
- `services/mcp/claudeai.ts`, `services/mcp/xaa.ts`, `services/mcp/xaaIdpLogin.ts` — claude.ai (XAA) integration
- `services/mcp/elicitationHandler.ts` — elicitation
- `services/mcp/envExpansion.ts`, `services/mcp/headersHelper.ts`, `services/mcp/mcpStringUtils.ts`, `services/mcp/normalization.ts` — utils
- `services/mcp/officialRegistry.ts` — official registry
- `services/mcp/useManageMCPConnections.ts`, `services/mcp/vscodeSdkMcp.ts` — (React hook; Rust: config watcher)
- `utils/mcp/` — datetime parser, elicitation validation

## Key Types

- Connections: `McpConnectionManager`, `McpConnectionState`, `McpClientError`, `ConnectedMcpServer`
- Config: `McpConfigLoader`, `McpServerConfig`, `ScopedMcpServerConfig`, `ConfigScope`, `McpTransport`, `McpConfigChanged`, `watch_mcp_configs`
- Discovery: `DiscoveryCache`, `DiscoveredTool`, `DiscoveredResource`, `DynamicResourceQuery`, `ServerCapabilities`, `McpCapabilities`, `McpResource`, `McpToolDefinition`, `ToolAnnotations`, `discover_all`, `discover_tools_from_server`, `discover_resources`, `discover_resources_matching`, `refresh_server_capabilities`, `convert_mcp_tool_to_tool_def`
- Auth: `OAuthConfig`, `OAuthTokens`, `OAuthTokenStore`
- Channels: `ChannelPermission`, `ChannelPermissionRelay`, `DenyAllRelay`, `StaticPermissionRelay`
- Elicitation: `ElicitationRequest`, `ElicitationResult`, `ElicitationField`, `ElicitationFieldType`, `ElicitationMode`, `ElicitationType`, `ElicitResult`
- Naming: `mcp_tool_id`, `parse_mcp_tool_id`
- Tool call: `tool_call` module
- Re-exports from `coco-rmcp-client`: `RmcpClient`, `ElicitationResponse`, `McpAuthStatus`, `SendElicitation`

## Note

`coco-mcp` only owns coco-specific business logic (scopes, discovery caching, file watching, naming). All rmcp protocol details (state machine, transport, OAuth persistor) live in `coco-rmcp-client`.
