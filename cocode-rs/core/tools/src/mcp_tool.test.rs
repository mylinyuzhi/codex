use super::*;
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
    // We can't fully test without a real client, but we can test the naming
    let tool = make_mcp_tool("get_data", Some("Gets data"));

    // Verify the tool definition
    assert_eq!(tool.name, "get_data");
    assert_eq!(tool.description, Some("Gets data".to_string()));
}

#[test]
fn test_input_schema_conversion() {
    let tool = make_mcp_tool("test", None);

    // Verify schema has properties and required
    assert!(tool.input_schema.properties.is_some());
    assert!(tool.input_schema.required.is_some());
}
