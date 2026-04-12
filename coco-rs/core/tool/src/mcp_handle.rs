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
    ) -> anyhow::Result<Vec<McpResourceInfo>>;

    /// Read a resource from an MCP server.
    async fn read_resource(
        &self,
        server_name: &str,
        resource_uri: &str,
    ) -> anyhow::Result<McpResourceContent>;

    /// Call an MCP tool.
    async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> anyhow::Result<McpToolCallResult>;

    /// Initiate authentication for an MCP server.
    async fn authenticate(&self, server_name: &str) -> anyhow::Result<String>;

    /// Get names of all connected servers.
    async fn connected_servers(&self) -> Vec<String>;

    /// List all tools from all connected MCP servers.
    ///
    /// Used by MCPTool to discover and expose MCP server tools to the LLM.
    /// TS: `MCPTool` dynamically generates tool definitions from this.
    async fn list_tools(&self) -> Vec<McpToolSchema> {
        vec![]
    }
}

pub type McpHandleRef = Arc<dyn McpHandle>;

/// No-op implementation for contexts without MCP (tests, subagents).
#[derive(Debug, Clone)]
pub struct NoOpMcpHandle;

#[async_trait::async_trait]
impl McpHandle for NoOpMcpHandle {
    async fn list_resources(&self, _: Option<&str>) -> anyhow::Result<Vec<McpResourceInfo>> {
        Ok(vec![])
    }
    async fn read_resource(&self, _: &str, _: &str) -> anyhow::Result<McpResourceContent> {
        anyhow::bail!("MCP not available in this context")
    }
    async fn call_tool(
        &self,
        _: &str,
        _: &str,
        _: Option<Value>,
    ) -> anyhow::Result<McpToolCallResult> {
        anyhow::bail!("MCP not available in this context")
    }
    async fn authenticate(&self, _: &str) -> anyhow::Result<String> {
        anyhow::bail!("MCP not available in this context")
    }
    async fn connected_servers(&self) -> Vec<String> {
        vec![]
    }
}
