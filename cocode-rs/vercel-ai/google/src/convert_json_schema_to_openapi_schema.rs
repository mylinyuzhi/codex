//! Convert JSON Schema 7 to OpenAPI 3.0 schema format.
//!
//! Google's API expects OpenAPI 3.0 schema format, not JSON Schema 7.

use serde_json::Map;
use serde_json::Value;

/// Convert a JSON Schema to OpenAPI 3.0 schema format.
///
/// This performs a recursive conversion, handling:
/// - Type arrays -> anyOf
/// - anyOf/oneOf/allOf pass-through with recursive conversion
/// - `const` -> `enum` with single value
/// - Removal of `$schema`, `additionalProperties`, `$id`, `$anchor`, `$comment`
/// - `minLength` preservation
pub fn convert_json_schema_to_openapi_schema(schema: &Value) -> Option<Value> {
    convert_inner(schema, true)
}

fn convert_inner(schema: &Value, is_root: bool) -> Option<Value> {
    let obj = schema.as_object()?;

    // Handle boolean schemas
    if obj.is_empty() {
        return Some(Value::Object(Map::new()));
    }

    let mut result = Map::new();

    // Process type field
    if let Some(type_val) = obj.get("type") {
        match type_val {
            Value::Array(types) => {
                // Type array: filter out "null" and create anyOf
                let non_null_types: Vec<&Value> = types
                    .iter()
                    .filter(|t| t.as_str() != Some("null"))
                    .collect();

                if non_null_types.len() == 1 {
                    result.insert("type".to_string(), non_null_types[0].clone());
                    if types.iter().any(|t| t.as_str() == Some("null")) {
                        result.insert("nullable".to_string(), Value::Bool(true));
                    }
                } else if !non_null_types.is_empty() {
                    let any_of: Vec<Value> = non_null_types
                        .iter()
                        .map(|t| {
                            let mut m = Map::new();
                            m.insert("type".to_string(), (*t).clone());
                            Value::Object(m)
                        })
                        .collect();
                    result.insert("anyOf".to_string(), Value::Array(any_of));
                    if types.iter().any(|t| t.as_str() == Some("null")) {
                        result.insert("nullable".to_string(), Value::Bool(true));
                    }
                }
            }
            Value::String(_) => {
                result.insert("type".to_string(), type_val.clone());
            }
            _ => {}
        }
    }

    // Convert properties recursively
    if let Some(Value::Object(props)) = obj.get("properties") {
        let mut converted_props = Map::new();
        for (key, value) in props {
            if let Some(converted) = convert_inner(value, false) {
                converted_props.insert(key.clone(), converted);
            }
        }
        if !converted_props.is_empty() {
            result.insert("properties".to_string(), Value::Object(converted_props));
        }
    }

    // Convert items recursively
    if let Some(items) = obj.get("items")
        && let Some(converted) = convert_inner(items, false)
    {
        result.insert("items".to_string(), converted);
    }

    // Handle const -> enum
    if let Some(const_val) = obj.get("const") {
        result.insert("enum".to_string(), Value::Array(vec![const_val.clone()]));
    }

    // Handle anyOf/oneOf/allOf
    for keyword in &["anyOf", "oneOf", "allOf"] {
        if let Some(Value::Array(schemas)) = obj.get(*keyword) {
            let converted: Vec<Value> = schemas
                .iter()
                .filter_map(|s| convert_inner(s, false))
                .collect();
            if !converted.is_empty() {
                result.insert(keyword.to_string(), Value::Array(converted));
            }
        }
    }

    // Pass through selected fields
    for key in &[
        "description",
        "enum",
        "required",
        "minLength",
        "maxLength",
        "minimum",
        "maximum",
        "pattern",
        "format",
        "default",
        "title",
        "minItems",
        "maxItems",
        "nullable",
    ] {
        if let Some(val) = obj.get(*key) {
            // Don't duplicate enum if we already added one from const
            if *key == "enum" && obj.contains_key("const") {
                continue;
            }
            result.insert(key.to_string(), val.clone());
        }
    }

    // Handle additionalProperties only for non-root
    // (Google API doesn't support additionalProperties at root)

    if is_root {
        // Remove $schema, $id, $anchor, $comment from root
        result.remove("$schema");
        result.remove("$id");
        result.remove("$anchor");
        result.remove("$comment");
    }

    Some(Value::Object(result))
}

#[cfg(test)]
#[path = "convert_json_schema_to_openapi_schema.test.rs"]
mod tests;
