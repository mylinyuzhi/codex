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
    assert_eq!(result["anyOf"], json!([{ "type": "string" }]));
    assert_eq!(result["nullable"], true);
}

#[test]
fn converts_type_array_multiple_non_null() {
    let schema = json!({
        "type": ["string", "integer", "null"]
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(
        result["anyOf"],
        json!([{ "type": "string" }, { "type": "integer" }])
    );
    assert_eq!(result["nullable"], true);
}

#[test]
fn converts_type_array_only_null() {
    let schema = json!({
        "type": ["null"]
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result["type"], "null");
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
fn handles_truly_empty_schema() {
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

// P2: Empty object schema handling
#[test]
fn empty_object_schema_returns_none_at_root() {
    let schema = json!({
        "type": "object",
        "properties": {}
    });
    assert!(convert_json_schema_to_openapi_schema(&schema).is_none());
}

#[test]
fn empty_object_schema_no_properties_returns_none_at_root() {
    let schema = json!({
        "type": "object"
    });
    assert!(convert_json_schema_to_openapi_schema(&schema).is_none());
}

#[test]
fn empty_object_schema_returns_type_object_when_nested() {
    let schema = json!({
        "type": "object",
        "properties": {
            "data": {
                "type": "object",
                "properties": {}
            }
        }
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result["properties"]["data"], json!({ "type": "object" }));
}

#[test]
fn empty_object_schema_with_description_nested() {
    let schema = json!({
        "type": "object",
        "properties": {
            "data": {
                "type": "object",
                "properties": {},
                "description": "A data object"
            }
        }
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(
        result["properties"]["data"],
        json!({ "type": "object", "description": "A data object" })
    );
}

// P2: Boolean schema handling
#[test]
fn boolean_schema_true() {
    let schema = json!(true);
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result, json!({ "type": "boolean", "properties": {} }));
}

#[test]
fn boolean_schema_false() {
    let schema = json!(false);
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result, json!({ "type": "boolean", "properties": {} }));
}

// P2: anyOf null-type flattening
#[test]
fn anyof_with_null_single_schema_flattened() {
    let schema = json!({
        "anyOf": [
            { "type": "string" },
            { "type": "null" }
        ]
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result["type"], "string");
    assert_eq!(result["nullable"], true);
    assert!(result.get("anyOf").is_none());
}

#[test]
fn anyof_with_null_multiple_schemas_kept() {
    let schema = json!({
        "anyOf": [
            { "type": "string" },
            { "type": "integer" },
            { "type": "null" }
        ]
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(
        result["anyOf"],
        json!([{ "type": "string" }, { "type": "integer" }])
    );
    assert_eq!(result["nullable"], true);
}

#[test]
fn anyof_without_null_passes_through() {
    let schema = json!({
        "anyOf": [
            { "type": "string" },
            { "type": "integer" }
        ]
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(
        result["anyOf"],
        json!([{ "type": "string" }, { "type": "integer" }])
    );
    assert!(result.get("nullable").is_none());
}

#[test]
fn anyof_with_null_complex_schema_flattened() {
    let schema = json!({
        "anyOf": [
            {
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                }
            },
            { "type": "null" }
        ]
    });
    let result = convert_json_schema_to_openapi_schema(&schema).unwrap();
    assert_eq!(result["type"], "object");
    assert_eq!(result["properties"]["name"]["type"], "string");
    assert_eq!(result["nullable"], true);
    assert!(result.get("anyOf").is_none());
}
