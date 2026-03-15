use super::*;
use serde_json::json;

#[test]
fn converts_simple_object_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer" }
        },
        "required": ["name"]
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result["type"], "object");
    assert_eq!(result["properties"]["name"]["type"], "string");
    assert_eq!(result["required"], json!(["name"]));
}

#[test]
fn converts_type_array_with_null() {
    let schema = json!({
        "type": ["string", "null"]
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result["type"], "string");
    assert_eq!(result["nullable"], true);
}

#[test]
fn converts_const_to_enum() {
    let schema = json!({
        "const": "fixed_value"
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result["enum"], json!(["fixed_value"]));
}

#[test]
fn converts_nested_properties() {
    let schema = json!({
        "type": "object",
        "properties": {
            "address": {
                "type": "object",
                "properties": {
                    "street": { "type": "string" }
                }
            }
        }
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(
        result["properties"]["address"]["properties"]["street"]["type"],
        "string"
    );
}

#[test]
fn handles_empty_schema() {
    let schema = json!({});
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result, json!({}));
}

#[test]
fn preserves_enum_values() {
    let schema = json!({
        "type": "string",
        "enum": ["a", "b", "c"]
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result["enum"], json!(["a", "b", "c"]));
}
