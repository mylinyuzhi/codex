use super::*;
use coco_tool_runtime::McpToolAnnotations;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolResultContentPart;
use serde_json::json;

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
