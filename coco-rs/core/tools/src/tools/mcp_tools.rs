use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::McpToolInfo;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::tool_result_storage;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// Typed input for [`McpAuthTool`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct McpAuthInput {
    /// Name of the MCP server to authenticate with
    #[serde(default)]
    pub server_name: String,
}

pub struct McpAuthTool;

#[async_trait::async_trait]
impl Tool for McpAuthTool {
    type Input = McpAuthInput;
    coco_tool_runtime::impl_runtime_schema!(McpAuthInput);
    /// Output is the bare status string from the MCP authenticator —
    /// rendered unwrapped so the model sees readable prose, not a
    /// JSON-quoted string.
    type Output = String;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::McpAuth)
    }
    fn name(&self) -> &str {
        ToolName::McpAuth.as_str()
    }
    fn max_result_size_bound(&self) -> coco_tool_runtime::ResultSizeBound {
        coco_tool_runtime::ResultSizeBound::Chars(10_000)
    }
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::Mcp)
    }
    fn description(&self, _input: &McpAuthInput, _options: &DescriptionOptions) -> String {
        "Authenticate with an MCP server to enable tool and resource access.".into()
    }

    async fn check_permissions(
        &self,
        input: &McpAuthInput,
        _ctx: &ToolUseContext,
    ) -> coco_types::ToolCheckResult {
        coco_types::ToolCheckResult::Allow {
            updated_input: serde_json::to_value(input).ok(),
            feedback: None,
        }
    }

    fn to_auto_classifier_input(&self, input: &McpAuthInput) -> Option<String> {
        Some(input.server_name.clone())
    }

    fn render_for_model(&self, out: &String) -> Vec<ToolResultContentPart> {
        vec![ToolResultContentPart::Text {
            text: out.clone(),
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: McpAuthInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<String>, ToolError> {
        if input.server_name.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "server_name is required".into(),
                error_code: None,
            });
        }

        let message = match ctx.mcp.authenticate(&input.server_name).await {
            Ok(msg) => msg,
            Err(e) => format!(
                "Authentication failed for {server}: {e}",
                server = input.server_name
            ),
        };

        Ok(ToolResult {
            data: message,
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

/// Typed input for [`ListMcpResourcesTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ListMcpResourcesInput {
    /// Optional MCP server name to filter resources
    #[serde(default)]
    pub server_name: Option<String>,
}

pub struct ListMcpResourcesTool;

#[async_trait::async_trait]
impl Tool for ListMcpResourcesTool {
    type Input = ListMcpResourcesInput;
    coco_tool_runtime::impl_runtime_schema!(ListMcpResourcesInput);
    /// Output is `Value` because the wire shape is a union (bare
    /// status string for empty/error, JSON array for results). TS
    /// `ListMcpResourcesTool.ts:108-122` treats both shapes the same
    /// way via `jsonStringify(content)` on the model-visible side.
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ListMcpResources)
    }
    fn name(&self) -> &str {
        ToolName::ListMcpResources.as_str()
    }
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::Mcp)
    }
    fn description(&self, _input: &ListMcpResourcesInput, _options: &DescriptionOptions) -> String {
        "List resources available on MCP servers.".into()
    }
    fn is_read_only(&self, _input: &ListMcpResourcesInput) -> bool {
        true
    }
    fn is_always_read_only(&self) -> bool {
        true
    }
    /// TS `ListMcpResourcesTool.ts`: `isConcurrencySafe() { return true }`.
    /// Listing resources from one or more MCP servers is read-only and
    /// independent across servers — the executor can fan out concurrent
    /// listing calls.
    fn is_concurrency_safe(&self, _input: &ListMcpResourcesInput) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("list resources available on connected MCP servers")
    }

    /// TS `ListMcpResourcesTool.ts:108-122`: empty branch emits a
    /// specific message; non-empty branch emits `jsonStringify(content)`.
    /// coco-rs execute() emits a bare string for the empty/error
    /// branches and a JSON array for non-empty; this render unwraps
    /// the bare string and JSON-stringifies the array — byte-identical
    /// to TS in both cases.
    fn render_for_model(&self, out: &Value) -> Vec<ToolResultContentPart> {
        coco_tool_runtime::render_text_or_json(out)
    }

    async fn execute(
        &self,
        input: ListMcpResourcesInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let server_name = input.server_name.as_deref();

        match ctx.mcp.list_resources(server_name).await {
            Ok(resources) => {
                if resources.is_empty() {
                    // TS `ListMcpResourcesTool.ts:113-115` empty-case message.
                    return Ok(ToolResult {
                        data: serde_json::json!(
                            "No resources found. MCP servers may still provide tools even if they have no resources."
                        ),
                        new_messages: vec![],
                        app_state_patch: None,
                        permission_updates: Vec::new(),
                        display_data: None,
                    });
                }
                let items: Vec<Value> = resources
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "server_name": r.server_name,
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
                    permission_updates: Vec::new(),
                    display_data: None,
                })
            }
            Err(e) => Ok(ToolResult {
                data: serde_json::json!(format!("Failed to list resources: {e}")),
                new_messages: vec![],
                app_state_patch: None,
                permission_updates: Vec::new(),
                display_data: None,
            }),
        }
    }
}

/// Typed input for [`ReadMcpResourceTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ReadMcpResourceInput {
    /// Name of the MCP server
    #[serde(default)]
    pub server_name: String,
    /// URI of the resource to read
    #[serde(default)]
    pub resource_uri: String,
}

pub struct ReadMcpResourceTool;

#[async_trait::async_trait]
impl Tool for ReadMcpResourceTool {
    type Input = ReadMcpResourceInput;
    coco_tool_runtime::impl_runtime_schema!(ReadMcpResourceInput);
    /// Output is `Value` because the wire shape varies: single content
    /// envelope, multi-content `{contents: [...]}`, or a bare error
    /// string. The renderer treats them uniformly.
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ReadMcpResource)
    }
    fn name(&self) -> &str {
        ToolName::ReadMcpResource.as_str()
    }
    fn is_enabled(&self, ctx: &ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::Mcp)
    }
    fn description(&self, _input: &ReadMcpResourceInput, _options: &DescriptionOptions) -> String {
        "Read a specific resource from an MCP server.".into()
    }
    fn is_read_only(&self, _input: &ReadMcpResourceInput) -> bool {
        true
    }
    fn is_always_read_only(&self) -> bool {
        true
    }
    /// TS `ReadMcpResourceTool.ts`: `isConcurrencySafe() { return true }`.
    /// Resource reads are side-effect-free; multiple reads to the same or
    /// different resources can run in parallel.
    fn is_concurrency_safe(&self, _input: &ReadMcpResourceInput) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("read a specific resource from an MCP server by URI")
    }

    /// TS `ReadMcpResourceTool.ts:151-157` emits `jsonStringify(content)`
    /// for both success and error paths — equivalent to the trait's
    /// default impl. The override exists only to unwrap the error-path
    /// bare string (which would otherwise be JSON-quoted) so the wire
    /// matches TS's plain string error format.
    fn render_for_model(&self, out: &Value) -> Vec<ToolResultContentPart> {
        if let Some(text) = out.get("persisted_output").and_then(Value::as_str) {
            return vec![ToolResultContentPart::Text {
                text: text.to_string(),
                provider_options: None,
            }];
        }
        coco_tool_runtime::render_text_or_json(out)
    }

    async fn execute(
        &self,
        input: ReadMcpResourceInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        if input.server_name.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "server_name is required".into(),
                error_code: None,
            });
        }
        if input.resource_uri.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "resource_uri is required".into(),
                error_code: None,
            });
        }

        match ctx
            .mcp
            .read_resource(&input.server_name, &input.resource_uri)
            .await
        {
            Ok(contents) => {
                let total = contents.len();
                let mut rendered = Vec::with_capacity(total);
                for (idx, content) in contents.iter().enumerate() {
                    rendered
                        .push(read_mcp_resource_content_for_model(ctx, content, idx, total).await);
                }
                let data = if rendered.len() == 1 {
                    rendered.remove(0)
                } else {
                    serde_json::json!({ "contents": rendered })
                };
                Ok(ToolResult {
                    data,
                    new_messages: vec![],
                    app_state_patch: None,
                    permission_updates: Vec::new(),
                    display_data: None,
                })
            }
            Err(e) => Ok(ToolResult {
                data: serde_json::json!(format!(
                    "Failed to read resource {uri} from {server}: {e}",
                    uri = input.resource_uri,
                    server = input.server_name
                )),
                new_messages: vec![],
                app_state_patch: None,
                permission_updates: Vec::new(),
                display_data: None,
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
    /// Self-validating schema (v4.2) built from the wire schema at
    /// construction — owns both the document (model-facing, via
    /// [`coco_tool_runtime::ToolInputSchema::as_value`]) and the compiled
    /// validator (runtime). Replaces the old `raw_schema` map; `from_value`
    /// folds in `"type":"object"` for servers that omit it.
    schema: coco_tool_runtime::ToolInputSchema,
    annotations: coco_tool_runtime::McpToolAnnotations,
}

impl McpTool {
    pub fn new(
        server_name: String,
        tool_name: String,
        description: String,
        schema: Value,
        annotations: coco_tool_runtime::McpToolAnnotations,
    ) -> Result<Self, coco_tool_runtime::SchemaError> {
        // Non-object / absent payload → canonical empty-params envelope
        // (TS parity). `from_value` folds in `"type":"object"` when the
        // server omits it and compiles the validator (= meta-validation);
        // an uncompilable wire schema surfaces as `Err` and the tool is
        // skipped at registration.
        let raw = match schema {
            Value::Object(_) => schema,
            _ => serde_json::json!({ "properties": {} }),
        };
        let schema = coco_tool_runtime::ToolInputSchema::from_value(raw)?;
        Ok(Self {
            info: McpToolInfo {
                server_name,
                tool_name,
            },
            tool_description: description,
            schema,
            annotations,
        })
    }
}

#[async_trait::async_trait]
impl Tool for McpTool {
    /// `McpTool` is the **dynamic** wrapper — its input schema is
    /// supplied by the connected MCP server at runtime via
    /// `self.info.input_schema`. No compile-time Rust type can describe
    /// it, so `Value` is the correct assoc type here (and only here).
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    fn runtime_validation_schema(&self) -> &coco_tool_runtime::ToolInputSchema {
        &self.schema
    }

    fn id(&self) -> ToolId {
        ToolId::Mcp {
            server: self.info.server_name.clone(),
            tool: self.info.tool_name.clone(),
        }
    }

    fn name(&self) -> &str {
        &self.info.tool_name
    }

    fn is_enabled(&self, ctx: &coco_tool_runtime::ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::Mcp)
    }

    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        self.tool_description.clone()
    }

    fn mcp_info(&self) -> Option<&McpToolInfo> {
        Some(&self.info)
    }

    /// TS `Tool.ts:441 isMcp: true` defers every MCP tool by default.
    /// The model must call `ToolSearch` to bring an MCP tool's full
    /// schema into the request — unless the server advertised
    /// `_meta["anthropic/alwaysLoad"] == true`, which routes through
    /// [`Self::always_load`] and short-circuits the deferred-pool
    /// filter in `ToolRegistry::loaded_tools`.
    fn should_defer(&self) -> bool {
        true
    }

    /// TS `prompt.ts:64-66 isDeferredTool`: `if (tool.alwaysLoad ===
    /// true) return false`. Read from
    /// `McpToolAnnotations.always_load`, sourced from the server's
    /// `_meta["anthropic/alwaysLoad"]` flag on the tool. When true,
    /// `ToolRegistry::loaded_tools` ignores the `should_defer()`
    /// signal and surfaces the tool's full schema on turn 1.
    fn always_load(&self) -> bool {
        self.annotations.always_load
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

    /// Decode the MCP server-provided content envelope back into typed
    /// `ToolResultContentPart`s. The `execute` path serializes
    /// `result.content` into a JSON array of `{type, ...}` blocks
    /// (success: bare array; error: `{error, content: [...]}`).
    /// `render_for_model` reverses that step so multimodal-capable
    /// providers see the original Text + FileData (image) parts the
    /// server emitted, instead of an opaque JSON-stringified envelope.
    /// TS parity: MCPTool wraps server content unchanged in
    /// `ToolResultBlockParam.content`.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let arr = data
            .as_array()
            .or_else(|| data.get("content").and_then(Value::as_array));
        let Some(blocks) = arr else {
            return vec![ToolResultContentPart::Text {
                text: serde_json::to_string(data).unwrap_or_default(),
                provider_options: None,
            }];
        };
        let parts: Vec<ToolResultContentPart> = blocks
            .iter()
            .filter_map(|block| {
                let kind = block.get("type")?.as_str()?;
                match kind {
                    "text" => Some(ToolResultContentPart::Text {
                        text: block.get("text")?.as_str()?.to_string(),
                        provider_options: None,
                    }),
                    "image" => Some(ToolResultContentPart::FileData {
                        data: block.get("data")?.as_str()?.to_string(),
                        media_type: block
                            .get("mime_type")
                            .and_then(Value::as_str)
                            .unwrap_or("image/png")
                            .to_string(),
                        filename: None,
                        provider_options: None,
                    }),
                    "resource" => block.get("text").and_then(Value::as_str).map(|text| {
                        ToolResultContentPart::Text {
                            text: text.to_string(),
                            provider_options: None,
                        }
                    }),
                    _ => None,
                }
            })
            .collect();
        if parts.is_empty() {
            return vec![ToolResultContentPart::Text {
                text: serde_json::to_string(data).unwrap_or_default(),
                provider_options: None,
            }];
        }
        parts
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
                let mut content: Vec<Value> = Vec::with_capacity(result.content.len());
                for (idx, block) in result.content.iter().enumerate() {
                    let value = match block {
                        coco_tool_runtime::mcp_handle::McpContentBlock::Text(text) => {
                            serde_json::json!({"type": "text", "text": text})
                        }
                        coco_tool_runtime::mcp_handle::McpContentBlock::Image {
                            data,
                            mime_type,
                        } => {
                            serde_json::json!({"type": "image", "data": data, "mime_type": mime_type})
                        }
                        coco_tool_runtime::mcp_handle::McpContentBlock::Audio {
                            data,
                            mime_type,
                        } => {
                            mcp_binary_block_for_model(
                                ctx,
                                &self.info.tool_name,
                                idx,
                                result.content.len(),
                                data,
                                Some(mime_type),
                            )
                            .await
                        }
                        coco_tool_runtime::mcp_handle::McpContentBlock::Resource {
                            uri,
                            text: Some(text),
                            mime_type,
                            ..
                        } => serde_json::json!({
                            "type": "resource",
                            "uri": uri,
                            "text": text,
                            "mime_type": mime_type,
                        }),
                        coco_tool_runtime::mcp_handle::McpContentBlock::Resource {
                            blob: Some(blob),
                            mime_type,
                            ..
                        } => {
                            mcp_binary_block_for_model(
                                ctx,
                                &self.info.tool_name,
                                idx,
                                result.content.len(),
                                blob,
                                mime_type.as_deref(),
                            )
                            .await
                        }
                        coco_tool_runtime::mcp_handle::McpContentBlock::Resource {
                            uri, ..
                        } => {
                            serde_json::json!({"type": "text", "text": format!("[Resource: {uri}]")})
                        }
                    };
                    content.push(value);
                }

                let data = if result.is_error {
                    serde_json::json!({"error": true, "content": content})
                } else {
                    serde_json::json!(content)
                };

                Ok(ToolResult {
                    data,
                    new_messages: vec![],
                    app_state_patch: None,
                    permission_updates: Vec::new(),
                    display_data: None,
                })
            }
            Err(e) => Err(ToolError::ExecutionFailed {
                message: format!(
                    "MCP tool call failed: {}.{}: {e}",
                    self.info.server_name, self.info.tool_name
                ),
                display_data: None,
                source: None,
            }),
        }
    }
}

async fn mcp_binary_block_for_model(
    ctx: &ToolUseContext,
    tool_name: &str,
    idx: usize,
    block_count: usize,
    blob: &str,
    mime_type: Option<&str>,
) -> Value {
    let Some(output_id) = mcp_binary_output_id(ctx, idx, block_count) else {
        return serde_json::json!({
            "type": "text",
            "text": format!("[Binary MCP content from {tool_name}: persistence unavailable]"),
        });
    };
    match persist_mcp_blob_reference(ctx, &output_id, blob, mime_type).await {
        Ok(reference) => serde_json::json!({"type": "text", "text": reference}),
        Err(message) => serde_json::json!({
            "type": "text",
            "text": format!("[Binary MCP content from {tool_name}: {message}]"),
        }),
    }
}

async fn read_mcp_resource_content_for_model(
    ctx: &ToolUseContext,
    content: &coco_tool_runtime::mcp_handle::McpResourceContent,
    idx: usize,
    total: usize,
) -> Value {
    if let Some(text) = &content.text {
        return serde_json::json!({
            "uri": content.uri,
            "text": text,
            "mime_type": content.mime_type,
        });
    }
    if let Some(blob) = &content.blob {
        if let Some(output_id) = mcp_binary_output_id(ctx, idx, total) {
            return match persist_mcp_blob_reference(
                ctx,
                &output_id,
                blob,
                content.mime_type.as_deref(),
            )
            .await
            {
                Ok(replacement) => serde_json::json!({
                    "uri": content.uri,
                    "mime_type": content.mime_type,
                    "persisted_output": replacement,
                }),
                Err(message) => serde_json::json!({
                    "uri": content.uri,
                    "mime_type": content.mime_type,
                    "has_blob": true,
                    "persistence_error": message,
                }),
            };
        }
        return serde_json::json!({
            "uri": content.uri,
            "mime_type": content.mime_type,
            "has_blob": true,
        });
    }
    serde_json::json!({
        "uri": content.uri,
        "mime_type": content.mime_type,
        "has_blob": false,
    })
}

fn mcp_binary_output_id(ctx: &ToolUseContext, idx: usize, total: usize) -> Option<String> {
    ctx.tool_use_id.as_deref().map(|tool_use_id| {
        if total == 1 {
            tool_use_id.to_string()
        } else {
            format!("{tool_use_id}-{}", idx + 1)
        }
    })
}

async fn persist_mcp_blob_reference(
    ctx: &ToolUseContext,
    output_id: &str,
    blob: &str,
    mime_type: Option<&str>,
) -> Result<String, String> {
    use base64::Engine as _;

    let session_dir = ctx
        .tool_result_session_dir
        .as_ref()
        .ok_or_else(|| "persistence unavailable".to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(blob)
        .map_err(|e| format!("invalid base64 data: {e}"))?;
    let persisted =
        tool_result_storage::persist_mcp_binary_to_disk(session_dir, output_id, &bytes, mime_type)
            .await
            .map_err(|e| format!("failed to persist binary output: {e}"))?;
    Ok(tool_result_storage::render_mcp_binary_reference(&persisted))
}

#[cfg(test)]
#[path = "mcp_tools.test.rs"]
mod tests;
