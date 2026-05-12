//! MCP handle trait — abstraction for MCP operations from tools.
//!
//! Same pattern as [`SideQuery`](crate::side_query): trait defined here,
//! implementation in the MCP service layer, injected via `ToolUseContext`.

use serde_json::Value;
use std::sync::Arc;

/// A resource from an MCP server.
#[derive(Debug, Clone)]
pub struct McpResourceInfo {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

/// Content of a read MCP resource.
#[derive(Debug, Clone)]
pub struct McpResourceContent {
    pub uri: String,
    pub text: Option<String>,
    pub blob: Option<String>,
    pub mime_type: Option<String>,
}

/// Result from an MCP tool call.
#[derive(Debug, Clone)]
pub struct McpToolCallResult {
    pub content: Vec<McpContentBlock>,
    pub is_error: bool,
}

/// A content block in an MCP result.
#[derive(Debug, Clone)]
pub enum McpContentBlock {
    Text(String),
    Image { data: String, mime_type: String },
}

/// MCP tool annotations — safety hints from the MCP server.
///
/// TS: `tool.annotations` (readOnlyHint, destructiveHint, openWorldHint).
/// These are server-declared hints, not guarantees. Used for concurrency
/// batching and permission decisions.
#[derive(Debug, Clone, Default)]
pub struct McpToolAnnotations {
    /// Tool only reads data, no side effects. Default: false.
    pub read_only_hint: bool,
    /// Tool can destroy or irreversibly modify data. Default: false.
    pub destructive_hint: bool,
    /// Tool accesses external resources (network, APIs). Default: false.
    pub open_world_hint: bool,
    /// Server-side opt-out from `ToolSearch` deferral. When the MCP
    /// server advertises `_meta["anthropic/alwaysLoad"] == true` on a
    /// tool, this flag short-circuits the deferred-pool filter in
    /// [`crate::ToolRegistry::loaded_tools`], so the tool's full
    /// schema appears in turn-1 tool definitions.
    ///
    /// TS parity: `prompt.ts:isDeferredTool` checks
    /// `tool.alwaysLoad === true` first (line 64) before any other
    /// rule. Default: false (every MCP tool is deferred unless the
    /// server opts out).
    pub always_load: bool,
}

impl McpToolAnnotations {
    /// Read the MCP server-declared `_meta` block off the tool's input
    /// schema and lift the `anthropic/alwaysLoad` flag onto the typed
    /// annotation struct. Other annotation fields stay at
    /// [`Default::default()`] — they are wired by the discovery layer
    /// from the rmcp `annotations` object, not from `_meta`.
    ///
    /// TS source: `prompt.ts:62-108 isDeferredTool` consumes the
    /// `tool.alwaysLoad` boolean that the MCP discovery code lifts
    /// from `_meta["anthropic/alwaysLoad"]`.
    pub fn from_input_schema_meta(input_schema: &Value) -> Self {
        let always_load = input_schema
            .get("_meta")
            .and_then(|m| m.get("anthropic/alwaysLoad"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Self {
            always_load,
            ..Self::default()
        }
    }
}

/// Schema info for an MCP server tool.
#[derive(Debug, Clone)]
pub struct McpToolSchema {
    pub server_name: String,
    pub tool_name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    /// Safety annotations from the MCP server.
    pub annotations: McpToolAnnotations,
}

/// Trait for MCP operations from tools.
///
/// Wraps `McpConnectionManager` — tools call this instead of
/// depending on `coco-mcp` directly.
#[async_trait::async_trait]
pub trait McpHandle: Send + Sync {
    /// List resources from an MCP server.
    async fn list_resources(
        &self,
        server_name: Option<&str>,
    ) -> Result<Vec<McpResourceInfo>, coco_error::BoxedError>;

    /// Read a resource from an MCP server.
    async fn read_resource(
        &self,
        server_name: &str,
        resource_uri: &str,
    ) -> Result<McpResourceContent, coco_error::BoxedError>;

    /// Call an MCP tool.
    async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> Result<McpToolCallResult, coco_error::BoxedError>;

    /// Initiate authentication for an MCP server.
    async fn authenticate(&self, server_name: &str) -> Result<String, coco_error::BoxedError>;

    /// Get names of all connected servers.
    async fn connected_servers(&self) -> Vec<String>;

    /// Names of servers still in the connecting / handshake phase. TS:
    /// `getPendingServerNames()` (`mcpClient.ts`). Surfaced by
    /// `ToolSearchTool` in its empty-match branch so the model knows
    /// to retry once handshakes complete. Default impl returns empty —
    /// handles without server-state tracking just say "no pending".
    async fn pending_server_names(&self) -> Vec<String> {
        Vec::new()
    }

    /// List all tools from all connected MCP servers.
    ///
    /// Used by MCPTool to discover and expose MCP server tools to the LLM.
    /// TS: `MCPTool` dynamically generates tool definitions from this.
    async fn list_tools(&self) -> Vec<McpToolSchema> {
        vec![]
    }

    /// Register and connect a dynamically-defined MCP server (TS
    /// `connectToServer(name, {…inlineConfig, scope: 'dynamic'})`).
    /// Used by per-agent MCP initialization (`runAgent.ts:135-191`)
    /// to stand up agent-private servers from inline frontmatter
    /// configs. Returns the server name on success so the caller can
    /// pair it with the matching `remove_dynamic_server` at agent
    /// teardown.
    ///
    /// `config` is the JSON body of `McpServerConfig` (transport,
    /// command, env, …). The default impl returns an error so
    /// handles without a real MCP layer surface a clean failure.
    async fn add_dynamic_server(
        &self,
        _name: &str,
        _config: Value,
    ) -> Result<(), coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "MCP add_dynamic_server not supported in this context",
            coco_error::StatusCode::Internal,
        )))
    }

    /// Disconnect + deregister a dynamically-added server. Mirror of
    /// `add_dynamic_server`. Called at SubagentStop for every server
    /// that was spun up via inline config so agent-private servers
    /// don't outlive their agent.
    async fn remove_dynamic_server(&self, _name: &str) -> Result<(), coco_error::BoxedError> {
        Ok(())
    }
}

pub type McpHandleRef = Arc<dyn McpHandle>;

/// No-op implementation for contexts without MCP (tests, subagents).
#[derive(Debug, Clone)]
pub struct NoOpMcpHandle;

#[async_trait::async_trait]
impl McpHandle for NoOpMcpHandle {
    async fn list_resources(
        &self,
        _: Option<&str>,
    ) -> Result<Vec<McpResourceInfo>, coco_error::BoxedError> {
        Ok(vec![])
    }
    async fn read_resource(
        &self,
        _: &str,
        _: &str,
    ) -> Result<McpResourceContent, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "MCP not available in this context",
            coco_error::StatusCode::Internal,
        )))
    }
    async fn call_tool(
        &self,
        _: &str,
        _: &str,
        _: Option<Value>,
    ) -> Result<McpToolCallResult, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "MCP not available in this context",
            coco_error::StatusCode::Internal,
        )))
    }
    async fn authenticate(&self, _: &str) -> Result<String, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "MCP not available in this context",
            coco_error::StatusCode::Internal,
        )))
    }
    async fn connected_servers(&self) -> Vec<String> {
        vec![]
    }
}
