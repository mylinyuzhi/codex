use super::*;
use serde_json::json;

fn make_function_tool(name: &str, schema: Value) -> LanguageModelTool {
    LanguageModelTool::function(crate::LanguageModelFunctionTool::with_description(
        name, "test", schema,
    ))
}

#[test]
fn test_noop_for_non_gemini() {
    let mut tools = vec![make_function_tool(
        "test",
        json!({
            "type": "object",
            "properties": {
                "count": { "type": "integer", "enum": [1, 2, 3] }
            }
        }),
    )];

    sanitize_tool_schemas(&mut tools, ProviderApi::Openai);

    // Should be unchanged
    match &tools[0] {
        LanguageModelTool::Function(ft) => {
            let count = &ft.input_schema["properties"]["count"];
            assert_eq!(count["type"], "integer");
        }
        _ => panic!("Expected function tool"),
    }
}

#[test]
fn test_integer_enum_to_string() {
    let mut tools = vec![make_function_tool(
        "test",
        json!({
            "type": "object",
            "properties": {
                "level": { "type": "integer", "enum": [1, 2, 3] }
            }
        }),
    )];

    sanitize_tool_schemas(&mut tools, ProviderApi::Gemini);

    match &tools[0] {
        LanguageModelTool::Function(ft) => {
            let level = &ft.input_schema["properties"]["level"];
            assert_eq!(level["type"], "string");
            assert_eq!(level["enum"], json!(["1", "2", "3"]));
        }
        _ => panic!("Expected function tool"),
    }
}

#[test]
fn test_array_items_default() {
    let mut tools = vec![make_function_tool(
        "test",
        json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array" }
            }
        }),
    )];

    sanitize_tool_schemas(&mut tools, ProviderApi::Gemini);

    match &tools[0] {
        LanguageModelTool::Function(ft) => {
            let tags = &ft.input_schema["properties"]["tags"];
            assert_eq!(tags["items"], json!({"type": "string"}));
        }
        _ => panic!("Expected function tool"),
    }
}

#[test]
fn test_array_empty_items_gets_type() {
    let mut tools = vec![make_function_tool(
        "test",
        json!({
            "type": "object",
            "properties": {
                "items_field": { "type": "array", "items": {} }
            }
        }),
    )];

    sanitize_tool_schemas(&mut tools, ProviderApi::Gemini);

    match &tools[0] {
        LanguageModelTool::Function(ft) => {
            let items_field = &ft.input_schema["properties"]["items_field"];
            assert_eq!(items_field["items"], json!({"type": "string"}));
        }
        _ => panic!("Expected function tool"),
    }
}

#[test]
fn test_array_with_typed_items_unchanged() {
    let mut tools = vec![make_function_tool(
        "test",
        json!({
            "type": "object",
            "properties": {
                "numbers": { "type": "array", "items": { "type": "number" } }
            }
        }),
    )];

    sanitize_tool_schemas(&mut tools, ProviderApi::Gemini);

    match &tools[0] {
        LanguageModelTool::Function(ft) => {
            let numbers = &ft.input_schema["properties"]["numbers"];
            assert_eq!(numbers["items"]["type"], "number");
        }
        _ => panic!("Expected function tool"),
    }
}

#[test]
fn test_required_filtered_to_existing_properties() {
    let mut tools = vec![make_function_tool(
        "test",
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name", "phantom_field"]
        }),
    )];

    sanitize_tool_schemas(&mut tools, ProviderApi::Gemini);

    match &tools[0] {
        LanguageModelTool::Function(ft) => {
            let required = ft.input_schema["required"].as_array().unwrap();
            assert_eq!(required.len(), 1);
            assert_eq!(required[0], "name");
        }
        _ => panic!("Expected function tool"),
    }
}

#[test]
fn test_properties_removed_from_non_object() {
    let mut tools = vec![make_function_tool(
        "test",
        json!({
            "type": "object",
            "properties": {
                "bad": {
                    "type": "string",
                    "properties": { "nested": { "type": "string" } },
                    "required": ["nested"]
                }
            }
        }),
    )];

    sanitize_tool_schemas(&mut tools, ProviderApi::Gemini);

    match &tools[0] {
        LanguageModelTool::Function(ft) => {
            let bad = &ft.input_schema["properties"]["bad"];
            assert!(bad.get("properties").is_none());
            assert!(bad.get("required").is_none());
        }
        _ => panic!("Expected function tool"),
    }
}

#[test]
fn test_combiner_preserves_properties() {
    let mut tools = vec![make_function_tool(
        "test",
        json!({
            "type": "object",
            "properties": {
                "field": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "number" }
                    ],
                    "properties": { "a": { "type": "string" } },
                    "required": ["a"]
                }
            }
        }),
    )];

    sanitize_tool_schemas(&mut tools, ProviderApi::Gemini);

    match &tools[0] {
        LanguageModelTool::Function(ft) => {
            let field = &ft.input_schema["properties"]["field"];
            // With combiner present, properties/required should NOT be removed
            assert!(field.get("properties").is_some());
            assert!(field.get("required").is_some());
        }
        _ => panic!("Expected function tool"),
    }
}

#[test]
fn test_recursive_sanitization() {
    let mut tools = vec![make_function_tool(
        "test",
        json!({
            "type": "object",
            "properties": {
                "nested": {
                    "type": "object",
                    "properties": {
                        "deep": {
                            "type": "integer",
                            "enum": [10, 20]
                        }
                    }
                }
            }
        }),
    )];

    sanitize_tool_schemas(&mut tools, ProviderApi::Gemini);

    match &tools[0] {
        LanguageModelTool::Function(ft) => {
            let deep = &ft.input_schema["properties"]["nested"]["properties"]["deep"];
            assert_eq!(deep["type"], "string");
            assert_eq!(deep["enum"], json!(["10", "20"]));
        }
        _ => panic!("Expected function tool"),
    }
}
