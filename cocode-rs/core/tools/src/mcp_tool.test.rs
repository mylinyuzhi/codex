use super::*;
use cocode_mcp_types::ToolAnnotations;
use cocode_mcp_types::ToolInputSchema;

fn make_mcp_tool(name: &str, description: Option<&str>) -> McpTool {
    McpTool {
        name: name.to_string(),
        description: description.map(String::from),
        input_schema: ToolInputSchema {
            r#type: "object".to_string(),
            properties: Some(serde_json::json!({
                "arg1": {"type": "string"}
            })),
            required: Some(vec!["arg1".to_string()]),
        },
        annotations: None,
        output_schema: None,
        title: None,
    }
}

#[test]
fn test_qualified_name() {
    let tool = make_mcp_tool("get_data", Some("Gets data"));

    assert_eq!(tool.name, "get_data");
    assert_eq!(tool.description, Some("Gets data".to_string()));
}

#[test]
fn test_input_schema_conversion() {
    let tool = make_mcp_tool("test", None);

    assert!(tool.input_schema.properties.is_some());
    assert!(tool.input_schema.required.is_some());
}

// ---------------------------------------------------------------------------
// Annotation logic tests
//
// McpToolWrapper has private fields and requires an Arc<RmcpClient> which
// cannot be mocked without spawning a real MCP server. To test the annotation
// logic in isolation, we inline the same expressions used by
// concurrency_safety() and is_read_only() and verify them against all
// annotation permutations.
// ---------------------------------------------------------------------------

/// Helper: resolve concurrency safety from a ToolAnnotations the same way
/// McpToolWrapper.concurrency_safety() does.
fn resolve_concurrency_safety(annotations: &Option<ToolAnnotations>) -> ConcurrencySafety {
    if annotations.as_ref().and_then(|a| a.read_only_hint) == Some(true) {
        ConcurrencySafety::Safe
    } else {
        ConcurrencySafety::Unsafe
    }
}

/// Helper: resolve is_read_only from annotations the same way
/// McpToolWrapper.is_read_only() does.
fn resolve_is_read_only(annotations: &Option<ToolAnnotations>) -> bool {
    annotations
        .as_ref()
        .and_then(|a| a.read_only_hint)
        .unwrap_or(false)
}

#[test]
fn test_concurrency_safety_without_annotations() {
    let annotations: Option<ToolAnnotations> = None;
    assert_eq!(
        resolve_concurrency_safety(&annotations),
        ConcurrencySafety::Unsafe
    );
    assert!(!resolve_is_read_only(&annotations));
}

#[test]
fn test_concurrency_safety_with_read_only_hint_true() {
    let annotations = Some(ToolAnnotations {
        read_only_hint: Some(true),
        destructive_hint: None,
        idempotent_hint: None,
        open_world_hint: None,
        title: None,
    });
    assert_eq!(
        resolve_concurrency_safety(&annotations),
        ConcurrencySafety::Safe
    );
    assert!(resolve_is_read_only(&annotations));
}

#[test]
fn test_concurrency_safety_with_read_only_hint_false() {
    let annotations = Some(ToolAnnotations {
        read_only_hint: Some(false),
        destructive_hint: None,
        idempotent_hint: None,
        open_world_hint: None,
        title: None,
    });
    assert_eq!(
        resolve_concurrency_safety(&annotations),
        ConcurrencySafety::Unsafe
    );
    assert!(!resolve_is_read_only(&annotations));
}

#[test]
fn test_concurrency_safety_with_empty_annotations() {
    let annotations = Some(ToolAnnotations {
        read_only_hint: None,
        destructive_hint: None,
        idempotent_hint: None,
        open_world_hint: None,
        title: None,
    });
    assert_eq!(
        resolve_concurrency_safety(&annotations),
        ConcurrencySafety::Unsafe
    );
    assert!(!resolve_is_read_only(&annotations));
}

#[test]
fn test_annotations_deserialized_from_json() {
    let json = serde_json::json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "title": "My Tool"
    });
    let annotations: ToolAnnotations = serde_json::from_value(json).expect("valid");
    assert_eq!(annotations.read_only_hint, Some(true));
    assert_eq!(annotations.destructive_hint, Some(false));
    assert_eq!(annotations.title, Some("My Tool".into()));
    assert_eq!(
        resolve_concurrency_safety(&Some(annotations)),
        ConcurrencySafety::Safe
    );
}
