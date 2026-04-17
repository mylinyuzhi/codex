use std::collections::HashMap;
use std::collections::HashSet;

use super::*;

fn make_source(name: &str, desc: &str, origin: ToolSchemaOrigin) -> ToolSchemaSource {
    let mut properties = HashMap::new();
    properties.insert(
        "path".to_string(),
        serde_json::json!({
            "type": "string",
            "description": "File path to read"
        }),
    );
    ToolSchemaSource {
        name: name.to_string(),
        description: desc.to_string(),
        input_schema: ToolInputSchema { properties },
        origin,
    }
}

#[test]
fn test_generate_tool_schemas_produces_valid_definitions() {
    let sources = vec![
        make_source("Read", "Read a file from disk", ToolSchemaOrigin::Builtin),
        make_source(
            "Write",
            "Write content to a file",
            ToolSchemaOrigin::Builtin,
        ),
    ];

    let result = generate_tool_schemas(&sources);
    assert_eq!(result.definitions.len(), 2);

    let read_def = &result.definitions[0];
    assert_eq!(read_def.name, "Read");
    assert_eq!(
        read_def.description.as_deref(),
        Some("Read a file from disk")
    );

    // The schema should be a valid JSON object with type: "object"
    let schema_obj = read_def
        .input_schema
        .as_object()
        .expect("schema should be an object");
    assert_eq!(
        schema_obj.get("type").and_then(serde_json::Value::as_str),
        Some("object"),
        "schema type should be 'object'"
    );
    assert!(
        schema_obj.contains_key("properties"),
        "schema should have properties"
    );

    assert!(
        result.estimated_tokens > 0,
        "should estimate positive token count"
    );
}

#[test]
fn test_generate_tool_schemas_empty_input() {
    let result = generate_tool_schemas(&[]);
    assert!(result.definitions.is_empty());
    assert_eq!(result.estimated_tokens, 0);
}

#[test]
fn test_merge_tool_schemas_builtin_wins() {
    let builtin = vec![make_source(
        "Read",
        "Built-in Read tool",
        ToolSchemaOrigin::Builtin,
    )];
    let plugin = vec![make_source(
        "Read",
        "Plugin Read override (should be ignored)",
        ToolSchemaOrigin::Plugin {
            plugin_name: "custom".to_string(),
        },
    )];
    let mcp = vec![];

    let merged = merge_tool_schemas(&builtin, &mcp, &plugin);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].description, "Built-in Read tool");
    assert_eq!(merged[0].origin, ToolSchemaOrigin::Builtin);
}

#[test]
fn test_merge_tool_schemas_plugin_over_mcp() {
    let builtin = vec![];
    let plugin = vec![make_source(
        "CustomTool",
        "Plugin custom tool",
        ToolSchemaOrigin::Plugin {
            plugin_name: "my_plugin".to_string(),
        },
    )];
    let mcp = vec![make_source(
        "CustomTool",
        "MCP version (should be ignored)",
        ToolSchemaOrigin::Mcp {
            server: "server1".to_string(),
        },
    )];

    let merged = merge_tool_schemas(&builtin, &mcp, &plugin);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].description, "Plugin custom tool");
}

#[test]
fn test_merge_tool_schemas_no_collisions() {
    let builtin = vec![make_source("Read", "Read tool", ToolSchemaOrigin::Builtin)];
    let mcp = vec![make_source(
        "mcp__slack__send",
        "Send Slack message",
        ToolSchemaOrigin::Mcp {
            server: "slack".to_string(),
        },
    )];
    let plugin = vec![make_source(
        "Deploy",
        "Deploy to prod",
        ToolSchemaOrigin::Plugin {
            plugin_name: "ops".to_string(),
        },
    )];

    let merged = merge_tool_schemas(&builtin, &mcp, &plugin);
    assert_eq!(merged.len(), 3);
}

#[test]
fn test_filter_schemas_by_model_supported_set() {
    let schemas = vec![
        make_source("Read", "Read file", ToolSchemaOrigin::Builtin),
        make_source("Write", "Write file", ToolSchemaOrigin::Builtin),
        make_source("ComputerUse", "Use computer", ToolSchemaOrigin::Builtin),
    ];

    let supported: HashSet<String> = ["Read", "Write"]
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    let filtered = filter_schemas_by_model(&schemas, Some(&supported), None);
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().all(|s| s.name != "ComputerUse"));
}

#[test]
fn test_filter_schemas_by_model_max_tools() {
    let schemas = vec![
        make_source("A", "Tool A", ToolSchemaOrigin::Builtin),
        make_source("B", "Tool B", ToolSchemaOrigin::Builtin),
        make_source("C", "Tool C", ToolSchemaOrigin::Builtin),
    ];

    let filtered = filter_schemas_by_model(&schemas, None, Some(2));
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].name, "A");
    assert_eq!(filtered[1].name, "B");
}

#[test]
fn test_filter_schemas_by_model_no_filter() {
    let schemas = vec![
        make_source("A", "Tool A", ToolSchemaOrigin::Builtin),
        make_source("B", "Tool B", ToolSchemaOrigin::Builtin),
    ];

    let filtered = filter_schemas_by_model(&schemas, None, None);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn test_estimate_schema_tokens_positive() {
    let sources = vec![make_source(
        "Read",
        "Read a file from the filesystem",
        ToolSchemaOrigin::Builtin,
    )];

    let tokens = estimate_schema_tokens(&sources);
    assert!(tokens > 50, "should include overhead plus content tokens");
}

#[test]
fn test_estimate_schema_tokens_empty() {
    assert_eq!(estimate_schema_tokens(&[]), 0);
}
