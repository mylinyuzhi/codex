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
    /// Optional hook registry for firing `Elicitation` /
    /// `ElicitationResult` hooks when an MCP server requests user
    /// input. `None` keeps the legacy no-op behaviour. TS:
    /// `services/mcp/elicitationHandler.ts`.
    hook_registry: Option<Arc<coco_hooks::HookRegistry>>,
    /// Builder that produces an `OrchestrationContext` per elicitation
    /// firing — same shape used elsewhere when session state is needed
    /// inside detached closures.
    elicitation_ctx_factory:
        Option<Arc<dyn Fn() -> coco_hooks::orchestration::OrchestrationContext + Send + Sync>>,
}

impl McpManagerAdapter {
    pub fn new(manager: Arc<Mutex<McpConnectionManager>>) -> Self {
        Self {
            manager,
            hook_registry: None,
            elicitation_ctx_factory: None,
        }
    }

    /// Install hook context so dynamic-MCP-server elicitations fire
    /// `Elicitation` / `ElicitationResult` hooks. Without this the
    /// adapter falls back to the legacy no-op send_elicitation closure.
    pub fn with_elicitation_hooks(
        mut self,
        registry: Arc<coco_hooks::HookRegistry>,
        ctx_factory: Arc<dyn Fn() -> coco_hooks::orchestration::OrchestrationContext + Send + Sync>,
    ) -> Self {
        self.hook_registry = Some(registry);
        self.elicitation_ctx_factory = Some(ctx_factory);
        self
    }
}

#[async_trait::async_trait]
impl McpHandle for McpManagerAdapter {
    async fn list_resources(
        &self,
        server_name: Option<&str>,
    ) -> Result<Vec<McpResourceInfo>, coco_error::BoxedError> {
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
    ) -> Result<McpResourceContent, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "McpHandle::read_resource not implemented through manager adapter; \
             tools should call ReadMcpResourceTool directly",
            coco_error::StatusCode::Internal,
        )))
    }

    async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> Result<McpToolCallResult, coco_error::BoxedError> {
        let manager = self.manager.lock().await;
        let result = manager
            .call_tool(server_name, tool_name, arguments)
            .await
            .map_err(|e| {
                Box::new(coco_error::PlainError::new(
                    format!("MCP tool call failed: {e}"),
                    coco_error::StatusCode::External,
                )) as coco_error::BoxedError
            })?;
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

    async fn authenticate(&self, _server_name: &str) -> Result<String, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "McpHandle::authenticate not implemented through manager adapter; \
             use McpAuthTool",
            coco_error::StatusCode::Internal,
        )))
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
    ) -> Result<(), coco_error::BoxedError> {
        // Deserialise the JSON body into the typed `McpServerConfig`
        // discriminated union. TS validates with Zod
        // (`McpServerConfigSchema`); coco-rs relies on serde's
        // `#[serde(tag = "transport")]` to do the same.
        let typed: coco_mcp::McpServerConfig = serde_json::from_value(config).map_err(|e| {
            Box::new(coco_error::PlainError::new(
                format!("invalid inline MCP server config for '{name}': {e}"),
                coco_error::StatusCode::InvalidJson,
            )) as coco_error::BoxedError
        })?;
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
        // Base SendElicitation: no UI dialog yet, so dialog-required
        // elicitations error out. The wrapper below makes hooks fire
        // first — if a hook returns a decision, we never reach this.
        let base_elicitation: coco_mcp::SendElicitation = Box::new(|_id, _req| {
            Box::pin(async move {
                Err(coco_mcp::RmcpClientError::generic(
                    "elicitation not supported for dynamically-added agent MCP servers",
                ))
            })
        });
        // TS parity: `services/mcp/elicitationHandler.ts:91-107` runs
        // hooks BEFORE showing the dialog. When hook context is
        // installed, wrap the base closure so `Elicitation` /
        // `ElicitationResult` hooks fire around every elicit/create
        // request (and the `elicitation_response` Notification fires
        // on completion).
        let send_elicitation = match (
            self.hook_registry.as_ref(),
            self.elicitation_ctx_factory.as_ref(),
        ) {
            (Some(registry), Some(factory)) => {
                crate::elicitation_hooks::wrap_send_elicitation_with_hooks(
                    name.to_string(),
                    registry.clone(),
                    factory.clone(),
                    base_elicitation,
                )
            }
            _ => base_elicitation,
        };
        manager.connect(name, send_elicitation).await.map_err(|e| {
            Box::new(coco_error::PlainError::new(
                format!("MCP connect '{name}' failed: {e}"),
                coco_error::StatusCode::ConnectionFailed,
            )) as coco_error::BoxedError
        })?;
        Ok(())
    }

    async fn remove_dynamic_server(&self, name: &str) -> Result<(), coco_error::BoxedError> {
        let manager = self.manager.lock().await;
        manager.disconnect(name).await;
        Ok(())
    }
}
