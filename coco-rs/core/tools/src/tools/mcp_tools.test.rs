use super::*;
use coco_tool_runtime::McpToolAnnotations;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::mcp_handle::McpContentBlock;
use coco_tool_runtime::mcp_handle::McpResourceContent;
use coco_tool_runtime::mcp_handle::McpResourceInfo;
use coco_tool_runtime::mcp_handle::McpToolCallResult;
use serde_json::json;
use std::sync::Arc;

fn make_mcp_tool() -> McpTool {
    McpTool::new(
        "test-server".into(),
        "test-tool".into(),
        "test description".into(),
        json!({"properties": {}}),
        McpToolAnnotations::default(),
    )
}

// ---------------------------------------------------------------------------
// alwaysLoad — TS `prompt.ts:64 isDeferredTool` opt-out
// ---------------------------------------------------------------------------

#[test]
fn always_load_defaults_to_false_so_mcp_tools_are_deferred() {
    let tool = make_mcp_tool();
    assert!(tool.should_defer(), "MCP tools defer by default");
    assert!(
        !tool.always_load(),
        "Default annotations do not opt out of deferral"
    );
}

#[test]
fn always_load_propagates_from_meta_opt_out() {
    // TS parity: `_meta["anthropic/alwaysLoad"] == true` on the
    // server-side tool schema → `tool.alwaysLoad === true` →
    // `isDeferredTool()` returns `false` first thing.
    let schema = json!({
        "_meta": {"anthropic/alwaysLoad": true},
        "properties": {}
    });
    let annotations = McpToolAnnotations::from_input_schema_meta(&schema);
    assert!(annotations.always_load);

    let tool = McpTool::new(
        "test-server".into(),
        "always-on".into(),
        "always-loaded tool".into(),
        schema,
        annotations,
    );
    assert!(tool.always_load(), "always_load flows through annotations");
    // `should_defer()` stays `true` — the registry filter is what
    // decides whether to surface the schema: `should_defer() &&
    // !always_load()`.
    assert!(tool.should_defer());
}

#[test]
fn always_load_meta_extractor_ignores_non_bool_values() {
    // Defensive: arbitrary `_meta["anthropic/alwaysLoad"]` payloads
    // from misbehaving servers (string "true", number 1) must NOT
    // accidentally opt the tool out.
    let schemas = [
        json!({"_meta": {"anthropic/alwaysLoad": "true"}}),
        json!({"_meta": {"anthropic/alwaysLoad": 1}}),
        json!({"_meta": {}}),
        json!({}),
    ];
    for schema in schemas {
        let a = McpToolAnnotations::from_input_schema_meta(&schema);
        assert!(
            !a.always_load,
            "non-bool _meta should not opt out: {schema}"
        );
    }
}

// ---------------------------------------------------------------------------
// ListMcpResources / ReadMcpResource render — TS `jsonStringify` parity
// ---------------------------------------------------------------------------

#[test]
fn list_mcp_resources_render_empty_string_path() {
    // execute() emits a bare string for the empty/error branches; the
    // render unwraps it so the wire output is the bare phrase, not a
    // JSON-quoted "...".
    let data = json!(
        "No resources found. MCP servers may still provide tools even if they have no resources."
    );
    let parts = ListMcpResourcesTool.render_for_model(&data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    assert!(text.starts_with("No resources found"));
    assert!(!text.starts_with('"'));
}

#[test]
fn list_mcp_resources_render_array_uses_json_stringify() {
    // TS non-empty branch: `jsonStringify(content)`.
    let data = json!([
        {"uri": "u1", "name": "n1", "description": "d1", "mime_type": "text/plain"},
    ]);
    let parts = ListMcpResourcesTool.render_for_model(&data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    assert!(text.starts_with('['), "expected JSON array, got: {text}");
    assert!(text.contains("\"uri\":\"u1\""));
}

#[test]
fn read_mcp_resource_render_uses_json_stringify() {
    let data = json!({"uri": "u1", "text": "hello", "mime_type": "text/plain"});
    let parts = ReadMcpResourceTool.render_for_model(&data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    assert!(text.starts_with('{'), "expected JSON object, got: {text}");
    assert!(text.contains("\"text\":\"hello\""));
}

#[tokio::test]
async fn mcp_auth_tool_forwards_to_generic_handle() {
    let mut ctx = ToolUseContext::test_default();
    ctx.mcp = Arc::new(AuthHandle {
        message: "Authentication started".into(),
    });
    assert!(!McpAuthTool.is_read_only(&json!({"server_name": "srv"})));
    assert_eq!(
        McpAuthTool.to_auto_classifier_input(&json!({"server_name": "srv"})),
        "srv"
    );
    let permission = McpAuthTool
        .check_permissions(&json!({"server_name": "srv"}), &ctx)
        .await;
    let coco_types::ToolCheckResult::Allow { updated_input, .. } = permission else {
        panic!("McpAuthTool should explicitly allow its auth-start input");
    };
    assert_eq!(updated_input, Some(json!({"server_name": "srv"})));

    let result = McpAuthTool
        .execute(json!({"server_name": "srv"}), &ctx)
        .await
        .unwrap();

    assert_eq!(result.data, json!("Authentication started"));
}

// ---------------------------------------------------------------------------
// render_for_model — pass-through MCP server-provided multimodal content
// ---------------------------------------------------------------------------

#[test]
fn render_decodes_text_block_array_into_text_part() {
    // Success path: data is a bare array of `{type, text/data, ...}` blocks.
    let tool = make_mcp_tool();
    let data = json!([
        {"type": "text", "text": "first chunk"},
        {"type": "text", "text": "second chunk"},
    ]);
    let parts = tool.render_for_model(&data);
    assert_eq!(parts.len(), 2);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part 0");
    };
    assert_eq!(text, "first chunk");
    let ToolResultContentPart::Text { text, .. } = &parts[1] else {
        panic!("expected Text part 1");
    };
    assert_eq!(text, "second chunk");
}

#[test]
fn render_decodes_image_block_into_filedata_part() {
    let tool = make_mcp_tool();
    let data = json!([
        {"type": "image", "data": "iVBOR...", "mime_type": "image/png"},
    ]);
    let parts = tool.render_for_model(&data);
    assert_eq!(parts.len(), 1);
    match &parts[0] {
        ToolResultContentPart::FileData {
            data,
            media_type,
            filename,
            ..
        } => {
            assert_eq!(data, "iVBOR...");
            assert_eq!(media_type, "image/png");
            assert!(filename.is_none());
        }
        other => panic!("expected FileData, got {other:?}"),
    }
}

#[test]
fn render_handles_mixed_text_and_image_in_order() {
    // Text + image + text — order must be preserved so the model sees
    // captions adjacent to the screenshot they describe.
    let tool = make_mcp_tool();
    let data = json!([
        {"type": "text", "text": "Screenshot of the page:"},
        {"type": "image", "data": "iVBOR...", "mime_type": "image/png"},
        {"type": "text", "text": "Notice the error banner."},
    ]);
    let parts = tool.render_for_model(&data);
    assert_eq!(parts.len(), 3);
    assert!(matches!(&parts[0], ToolResultContentPart::Text { .. }));
    assert!(matches!(&parts[1], ToolResultContentPart::FileData { .. }));
    assert!(matches!(&parts[2], ToolResultContentPart::Text { .. }));
}

#[test]
fn render_decodes_error_envelope_content() {
    // Error path: data is `{error: true, content: [...]}`.
    let tool = make_mcp_tool();
    let data = json!({
        "error": true,
        "content": [
            {"type": "text", "text": "Tool execution failed: timeout"},
        ],
    });
    let parts = tool.render_for_model(&data);
    assert_eq!(parts.len(), 1);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    assert_eq!(text, "Tool execution failed: timeout");
}

#[test]
fn render_unknown_block_falls_back_to_json_string() {
    // Defensive: a future block type the decoder doesn't recognize.
    // Empty parts list triggers the JSON fallback so the model still
    // sees something rather than getting a silent empty result.
    let tool = make_mcp_tool();
    let data = json!([
        {"type": "audio", "data": "...", "mime_type": "audio/wav"},
    ]);
    let parts = tool.render_for_model(&data);
    assert_eq!(parts.len(), 1);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text fallback");
    };
    // JSON fallback contains the raw payload so nothing is dropped.
    assert!(text.contains("audio"), "got: {text}");
}

#[tokio::test]
async fn read_mcp_resource_persists_blob_to_session_tool_results() {
    use base64::Engine as _;

    let tmp = tempfile::TempDir::new().unwrap();
    let bytes = b"\x89PNG\r\n\x1a\nblob";
    let mut ctx = ToolUseContext::test_default();
    ctx.config_home = Some(tmp.path().to_path_buf());
    ctx.session_id_for_history = Some("session-1".into());
    ctx.tool_result_session_dir = Some(tmp.path().join("sessions/session-1"));
    ctx.tool_use_id = Some("tool-1".into());
    ctx.mcp = Arc::new(BlobResourceHandle {
        blob: base64::engine::general_purpose::STANDARD.encode(bytes),
        mime_type: "image/png".into(),
    });

    let result = ReadMcpResourceTool
        .execute(
            json!({
                "server_name": "srv",
                "resource_uri": "mcp://file",
            }),
            &ctx,
        )
        .await
        .unwrap();
    let rendered = ReadMcpResourceTool.render_for_model(&result.data);
    let ToolResultContentPart::Text { text, .. } = &rendered[0] else {
        panic!("expected Text part");
    };
    assert!(text.starts_with("<persisted-output>"), "got: {text}");
    assert!(text.contains("MCP output is binary"), "got: {text}");
    let path = tmp
        .path()
        .join("sessions/session-1/tool-results/tool-1.png");
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[tokio::test]
async fn read_mcp_resource_preserves_multiple_contents() {
    use base64::Engine as _;

    let tmp = tempfile::TempDir::new().unwrap();
    let bytes = b"audio";
    let mut ctx = ToolUseContext::test_default();
    ctx.tool_result_session_dir = Some(tmp.path().join("sessions/session-1"));
    ctx.tool_use_id = Some("tool-multi".into());
    ctx.mcp = Arc::new(MixedResourceHandle {
        blob: base64::engine::general_purpose::STANDARD.encode(bytes),
    });

    let result = ReadMcpResourceTool
        .execute(
            json!({
                "server_name": "srv",
                "resource_uri": "mcp://bundle",
            }),
            &ctx,
        )
        .await
        .unwrap();

    let contents = result.data["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 2);
    assert_eq!(contents[0]["text"], "first");
    assert!(
        contents[1]["persisted_output"]
            .as_str()
            .unwrap()
            .contains("audio/wav")
    );
    let path = tmp
        .path()
        .join("sessions/session-1/tool-results/tool-multi-2.wav");
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[tokio::test]
async fn dynamic_mcp_tool_persists_embedded_resource_blob() {
    use base64::Engine as _;

    let tmp = tempfile::TempDir::new().unwrap();
    let bytes = b"%PDF-1.7";
    let mut ctx = ToolUseContext::test_default();
    ctx.config_home = Some(tmp.path().to_path_buf());
    ctx.session_id_for_history = Some("session-1".into());
    ctx.tool_result_session_dir = Some(tmp.path().join("sessions/session-1"));
    ctx.tool_use_id = Some("tool-2".into());
    ctx.mcp = Arc::new(BlobToolHandle {
        blob: base64::engine::general_purpose::STANDARD.encode(bytes),
        mime_type: "application/pdf".into(),
    });

    let tool = make_mcp_tool();
    let result = tool.execute(json!({}), &ctx).await.unwrap();
    let parts = tool.render_for_model(&result.data);
    let ToolResultContentPart::Text { text, .. } = &parts[0] else {
        panic!("expected Text part");
    };
    assert!(text.starts_with("<persisted-output>"), "got: {text}");
    assert!(text.contains("application/pdf"), "got: {text}");
    let path = tmp
        .path()
        .join("sessions/session-1/tool-results/tool-2.pdf");
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

struct BlobResourceHandle {
    blob: String,
    mime_type: String,
}

struct AuthHandle {
    message: String,
}

#[async_trait::async_trait]
impl coco_tool_runtime::McpHandle for AuthHandle {
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
    ) -> Result<Vec<McpResourceContent>, coco_error::BoxedError> {
        unreachable!("not used by McpAuthTool test")
    }

    async fn call_tool(
        &self,
        _: &str,
        _: &str,
        _: Option<serde_json::Value>,
    ) -> Result<McpToolCallResult, coco_error::BoxedError> {
        unreachable!("not used by McpAuthTool test")
    }

    async fn authenticate(&self, _: &str) -> Result<String, coco_error::BoxedError> {
        Ok(self.message.clone())
    }

    async fn connected_servers(&self) -> Vec<String> {
        vec![]
    }
}

#[async_trait::async_trait]
impl coco_tool_runtime::McpHandle for BlobResourceHandle {
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
    ) -> Result<Vec<McpResourceContent>, coco_error::BoxedError> {
        Ok(vec![McpResourceContent {
            uri: "mcp://file".into(),
            text: None,
            blob: Some(self.blob.clone()),
            mime_type: Some(self.mime_type.clone()),
        }])
    }

    async fn call_tool(
        &self,
        _: &str,
        _: &str,
        _: Option<serde_json::Value>,
    ) -> Result<McpToolCallResult, coco_error::BoxedError> {
        unreachable!("not used by ReadMcpResourceTool test")
    }

    async fn authenticate(&self, _: &str) -> Result<String, coco_error::BoxedError> {
        unreachable!("not used by ReadMcpResourceTool test")
    }

    async fn connected_servers(&self) -> Vec<String> {
        vec![]
    }
}

struct BlobToolHandle {
    blob: String,
    mime_type: String,
}

struct MixedResourceHandle {
    blob: String,
}

#[async_trait::async_trait]
impl coco_tool_runtime::McpHandle for MixedResourceHandle {
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
    ) -> Result<Vec<McpResourceContent>, coco_error::BoxedError> {
        Ok(vec![
            McpResourceContent {
                uri: "mcp://bundle/text".into(),
                text: Some("first".into()),
                blob: None,
                mime_type: Some("text/plain".into()),
            },
            McpResourceContent {
                uri: "mcp://bundle/audio".into(),
                text: None,
                blob: Some(self.blob.clone()),
                mime_type: Some("audio/wav".into()),
            },
        ])
    }

    async fn call_tool(
        &self,
        _: &str,
        _: &str,
        _: Option<serde_json::Value>,
    ) -> Result<McpToolCallResult, coco_error::BoxedError> {
        unreachable!("not used by multi resource test")
    }

    async fn authenticate(&self, _: &str) -> Result<String, coco_error::BoxedError> {
        unreachable!("not used by multi resource test")
    }

    async fn connected_servers(&self) -> Vec<String> {
        vec![]
    }
}

#[async_trait::async_trait]
impl coco_tool_runtime::McpHandle for BlobToolHandle {
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
    ) -> Result<Vec<McpResourceContent>, coco_error::BoxedError> {
        unreachable!("not used by dynamic MCP tool test")
    }

    async fn call_tool(
        &self,
        _: &str,
        _: &str,
        _: Option<serde_json::Value>,
    ) -> Result<McpToolCallResult, coco_error::BoxedError> {
        Ok(McpToolCallResult {
            content: vec![McpContentBlock::Resource {
                uri: "mcp://report".into(),
                text: None,
                blob: Some(self.blob.clone()),
                mime_type: Some(self.mime_type.clone()),
            }],
            is_error: false,
        })
    }

    async fn authenticate(&self, _: &str) -> Result<String, coco_error::BoxedError> {
        unreachable!("not used by dynamic MCP tool test")
    }

    async fn connected_servers(&self) -> Vec<String> {
        vec![]
    }
}
