//! Bridge: `coco_mcp::McpConnectionManager` → `coco_tool_runtime::McpHandle`.
//!
//! Lives in `app/cli` because it sits at the seam where both crates
//! are deps. The adapter forwards the handle's read paths to the
//! manager's internal API; mutating paths (register / disconnect)
//! aren't exposed because tools should never reconfigure MCP at
//! runtime — only the supervisor / config watcher does that.
//!
//! TS parity — `MCPConnectionManager` is the single source of truth in
//! TS too; tool layer reads via `services/mcp/client.ts` directly.
//! Rust adds the trait indirection for testability.

use std::sync::Arc;

use coco_mcp::McpConnectionManager;
use coco_tool_runtime::McpHandle;
use coco_tool_runtime::McpToolSchema;
use coco_tool_runtime::mcp_handle::McpContentBlock;
use coco_tool_runtime::mcp_handle::McpResourceContent;
use coco_tool_runtime::mcp_handle::McpResourceInfo;
use coco_tool_runtime::mcp_handle::McpToolAnnotations;
use coco_tool_runtime::mcp_handle::McpToolCallResult;
use serde_json::Value;
use tokio::sync::Mutex;

/// Adapter that wraps a shared `Arc<Mutex<McpConnectionManager>>` and
/// implements `McpHandle`.
pub struct McpManagerAdapter {
    manager: Arc<Mutex<McpConnectionManager>>,
}

impl McpManagerAdapter {
    pub fn new(manager: Arc<Mutex<McpConnectionManager>>) -> Self {
        Self { manager }
    }
}

#[async_trait::async_trait]
impl McpHandle for McpManagerAdapter {
    async fn list_resources(
        &self,
        server_name: Option<&str>,
    ) -> anyhow::Result<Vec<McpResourceInfo>> {
        let manager = self.manager.lock().await;
        let connected = manager.connected_servers().await;
        let mut out = Vec::new();
        for server in connected {
            if let Some(filter) = server_name
                && server.name != filter
            {
                continue;
            }
            for r in &server.resources {
                out.push(McpResourceInfo {
                    uri: r.uri.clone(),
                    name: r.name.clone(),
                    description: r.description.clone(),
                    mime_type: r.mime_type.clone(),
                });
            }
        }
        Ok(out)
    }

    async fn read_resource(
        &self,
        _server_name: &str,
        _resource_uri: &str,
    ) -> anyhow::Result<McpResourceContent> {
        // `McpConnectionManager` doesn't expose a typed `read_resource`
        // surface yet — the tool layer's `ReadMcpResourceTool` reads
        // resources through the rmcp client directly. Keeping this as
        // a clean error preserves the no-op contract for callers that
        // touch this trait method through `McpHandle` (which only
        // `tools/agent/agent_tool::check_mcp_ready` does today).
        anyhow::bail!(
            "McpHandle::read_resource not implemented through manager adapter; \
             tools should call ReadMcpResourceTool directly"
        )
    }

    async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> anyhow::Result<McpToolCallResult> {
        let manager = self.manager.lock().await;
        let result = manager
            .call_tool(server_name, tool_name, arguments)
            .await
            .map_err(|e| anyhow::anyhow!("MCP tool call failed: {e}"))?;
        // Translate rmcp `CallToolResult` into the trait's content
        // blocks. Text blocks pass through; non-text blocks render as
        // an `Image` placeholder (the trait only carries text + image
        // shapes today).
        let content = result
            .content
            .into_iter()
            .filter_map(|block| {
                let raw = serde_json::to_value(&block).ok()?;
                let kind = raw.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match kind {
                    "text" => raw
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|t| McpContentBlock::Text(t.to_string())),
                    "image" => {
                        let data = raw.get("data").and_then(|v| v.as_str())?.to_string();
                        let mime_type = raw
                            .get("mimeType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("application/octet-stream")
                            .to_string();
                        Some(McpContentBlock::Image { data, mime_type })
                    }
                    _ => None,
                }
            })
            .collect();
        Ok(McpToolCallResult {
            content,
            is_error: result.is_error.unwrap_or(false),
        })
    }

    async fn authenticate(&self, _server_name: &str) -> anyhow::Result<String> {
        // Auth flow is owned by `McpAuthTool` which goes through the
        // RmcpClient OAuth port directly; no need to surface here.
        anyhow::bail!(
            "McpHandle::authenticate not implemented through manager adapter; \
             use McpAuthTool"
        )
    }

    async fn connected_servers(&self) -> Vec<String> {
        let manager = self.manager.lock().await;
        manager
            .connected_servers()
            .await
            .into_iter()
            .map(|s| s.name)
            .collect()
    }

    async fn list_tools(&self) -> Vec<McpToolSchema> {
        let manager = self.manager.lock().await;
        manager
            .all_tools()
            .await
            .into_iter()
            .map(|(server, def)| McpToolSchema {
                server_name: server,
                tool_name: def.name,
                description: def.description,
                input_schema: def.input_schema,
                annotations: McpToolAnnotations::default(),
            })
            .collect()
    }

    async fn add_dynamic_server(
        &self,
        name: &str,
        config: serde_json::Value,
    ) -> anyhow::Result<()> {
        // Deserialise the JSON body into the typed `McpServerConfig`
        // discriminated union. TS validates with Zod
        // (`McpServerConfigSchema`); coco-rs relies on serde's
        // `#[serde(tag = "transport")]` to do the same.
        let typed: coco_mcp::McpServerConfig = serde_json::from_value(config)
            .map_err(|e| anyhow::anyhow!("invalid inline MCP server config for '{name}': {e}"))?;
        let scoped = coco_mcp::ScopedMcpServerConfig {
            name: name.to_string(),
            config: typed,
            scope: coco_mcp::ConfigScope::Dynamic,
            plugin_source: None,
        };
        let mut manager = self.manager.lock().await;
        manager.register_server(scoped);
        // `connect` validates wiring + opens the transport. Errors
        // bubble up so the spawn path can fail closed (TS behaviour:
        // a failed inline server logs a warning + skips the agent's
        // tools — we surface the error so the agent's spawn can
        // decide).
        //
        // SendElicitation is a no-op closure here — dynamic agent-
        // private servers shouldn't block on a UI elicitation prompt
        // mid-spawn. If a server requires elicitation it'll fail at
        // connect time; the agent then runs without that tool.
        let no_elicitation: coco_mcp::SendElicitation = Box::new(|_id, _req| {
            Box::pin(async move {
                Err(anyhow::anyhow!(
                    "elicitation not supported for dynamically-added agent MCP servers"
                ))
            })
        });
        manager
            .connect(name, no_elicitation)
            .await
            .map_err(|e| anyhow::anyhow!("MCP connect '{name}' failed: {e}"))?;
        Ok(())
    }

    async fn remove_dynamic_server(&self, name: &str) -> anyhow::Result<()> {
        let manager = self.manager.lock().await;
        manager.disconnect(name).await;
        Ok(())
    }
}
