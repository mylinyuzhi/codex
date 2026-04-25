use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::McpToolInfo;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

pub struct McpAuthTool;

#[async_trait::async_trait]
impl Tool for McpAuthTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::McpAuth)
    }
    fn name(&self) -> &str {
        ToolName::McpAuth.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Authenticate with an MCP server to enable tool and resource access.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "server_name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Name of the MCP server to authenticate with"
            }),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let server_name = input
            .get("server_name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if server_name.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "server_name is required".into(),
                error_code: None,
            });
        }

        match ctx.mcp.authenticate(server_name).await {
            Ok(msg) => Ok(ToolResult {
                data: serde_json::json!(msg),
                new_messages: vec![],
                app_state_patch: None,
            }),
            Err(e) => Ok(ToolResult {
                data: serde_json::json!(format!("Authentication failed for {server_name}: {e}")),
                new_messages: vec![],
                app_state_patch: None,
            }),
        }
    }
}

pub struct ListMcpResourcesTool;

#[async_trait::async_trait]
impl Tool for ListMcpResourcesTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ListMcpResources)
    }
    fn name(&self) -> &str {
        ToolName::ListMcpResources.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "List resources available on MCP servers.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "server_name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Optional MCP server name to filter resources"
            }),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    /// TS `ListMcpResourcesTool.ts`: `isConcurrencySafe() { return true }`.
    /// Listing resources from one or more MCP servers is read-only and
    /// independent across servers — the executor can fan out concurrent
    /// listing calls.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let server_name = input.get("server_name").and_then(|v| v.as_str());

        match ctx.mcp.list_resources(server_name).await {
            Ok(resources) => {
                if resources.is_empty() {
                    return Ok(ToolResult {
                        data: serde_json::json!("No MCP resources available"),
                        new_messages: vec![],
                        app_state_patch: None,
                    });
                }
                let items: Vec<Value> = resources
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "uri": r.uri,
                            "name": r.name,
                            "description": r.description,
                            "mime_type": r.mime_type,
                        })
                    })
                    .collect();
                Ok(ToolResult {
                    data: serde_json::json!(items),
                    new_messages: vec![],
                    app_state_patch: None,
                })
            }
            Err(e) => Ok(ToolResult {
                data: serde_json::json!(format!("Failed to list resources: {e}")),
                new_messages: vec![],
                app_state_patch: None,
            }),
        }
    }
}

pub struct ReadMcpResourceTool;

#[async_trait::async_trait]
impl Tool for ReadMcpResourceTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ReadMcpResource)
    }
    fn name(&self) -> &str {
        ToolName::ReadMcpResource.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Read a specific resource from an MCP server.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "server_name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Name of the MCP server"
            }),
        );
        p.insert(
            "resource_uri".into(),
            serde_json::json!({
                "type": "string",
                "description": "URI of the resource to read"
            }),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    /// TS `ReadMcpResourceTool.ts`: `isConcurrencySafe() { return true }`.
    /// Resource reads are side-effect-free; multiple reads to the same or
    /// different resources can run in parallel.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let server_name = input
            .get("server_name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let resource_uri = input
            .get("resource_uri")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if server_name.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "server_name is required".into(),
                error_code: None,
            });
        }
        if resource_uri.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "resource_uri is required".into(),
                error_code: None,
            });
        }

        match ctx.mcp.read_resource(server_name, resource_uri).await {
            Ok(content) => {
                let data = if let Some(text) = &content.text {
                    serde_json::json!({
                        "uri": content.uri,
                        "text": text,
                        "mime_type": content.mime_type,
                    })
                } else {
                    serde_json::json!({
                        "uri": content.uri,
                        "mime_type": content.mime_type,
                        "has_blob": content.blob.is_some(),
                    })
                };
                Ok(ToolResult {
                    data,
                    new_messages: vec![],
                    app_state_patch: None,
                })
            }
            Err(e) => Ok(ToolResult {
                data: serde_json::json!(format!(
                    "Failed to read resource {resource_uri} from {server_name}: {e}"
                )),
                new_messages: vec![],
                app_state_patch: None,
            }),
        }
    }
}

/// Dynamic MCP tool wrapper — exposes MCP server tools to the LLM.
///
/// TS: `MCPTool` in `tools/MCPTool/` — generates tool definitions dynamically
/// from MCP server tool schemas. Input is passed through to the MCP server.
///
/// Each MCPTool instance wraps one specific MCP server tool. The registry
/// creates one MCPTool per discovered MCP server tool at startup and when
/// MCP servers connect/disconnect.
pub struct McpTool {
    info: McpToolInfo,
    tool_description: String,
    schema: ToolInputSchema,
    annotations: coco_tool_runtime::McpToolAnnotations,
}

impl McpTool {
    pub fn new(
        server_name: String,
        tool_name: String,
        description: String,
        schema: Value,
        annotations: coco_tool_runtime::McpToolAnnotations,
    ) -> Self {
        let properties = schema
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        Self {
            info: McpToolInfo {
                server_name,
                tool_name,
            },
            tool_description: description,
            schema: ToolInputSchema { properties },
            annotations,
        }
    }
}

#[async_trait::async_trait]
impl Tool for McpTool {
    fn id(&self) -> ToolId {
        ToolId::Mcp {
            server: self.info.server_name.clone(),
            tool: self.info.tool_name.clone(),
        }
    }

    fn name(&self) -> &str {
        &self.info.tool_name
    }

    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        self.tool_description.clone()
    }

    fn input_schema(&self) -> ToolInputSchema {
        self.schema.clone()
    }

    fn mcp_info(&self) -> Option<&McpToolInfo> {
        Some(&self.info)
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        // TS: tool.annotations?.readOnlyHint ?? false
        // Only concurrent-safe if the server declares read-only.
        self.annotations.read_only_hint
    }

    fn is_read_only(&self, _: &Value) -> bool {
        // TS: tool.annotations?.readOnlyHint ?? false
        self.annotations.read_only_hint
    }

    fn is_destructive(&self, _: &Value) -> bool {
        // TS: tool.annotations?.destructiveHint ?? false
        self.annotations.destructive_hint
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let arguments =
            if input.is_null() || input.as_object().is_some_and(serde_json::Map::is_empty) {
                None
            } else {
                Some(input)
            };

        match ctx
            .mcp
            .call_tool(&self.info.server_name, &self.info.tool_name, arguments)
            .await
        {
            Ok(result) => {
                let content: Vec<Value> = result
                    .content
                    .iter()
                    .map(|block| match block {
                        coco_tool_runtime::mcp_handle::McpContentBlock::Text(text) => {
                            serde_json::json!({"type": "text", "text": text})
                        }
                        coco_tool_runtime::mcp_handle::McpContentBlock::Image { data, mime_type } => {
                            serde_json::json!({"type": "image", "data": data, "mime_type": mime_type})
                        }
                    })
                    .collect();

                let data = if result.is_error {
                    serde_json::json!({"error": true, "content": content})
                } else {
                    serde_json::json!(content)
                };

                Ok(ToolResult {
                    data,
                    new_messages: vec![],
                    app_state_patch: None,
                })
            }
            Err(e) => Err(ToolError::ExecutionFailed {
                message: format!(
                    "MCP tool call failed: {}.{}: {e}",
                    self.info.server_name, self.info.tool_name
                ),
                source: None,
            }),
        }
    }
}
