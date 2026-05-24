//! Gemini-specific tool schema sanitization.
//!
//! Google's Gemini API imposes stricter JSON schema constraints than OpenAI/Anthropic.
//! This module transforms tool schemas to be Gemini-compatible:
//!
//! 1. Integer/number enums → string enums (Gemini only supports string enums)
//! 2. Array items without type → default `"type": "string"`
//! 3. `properties`/`required` on non-object types → removed (Gemini rejects)
//! 4. `required` entries not in `properties` → filtered out
//!
//! Ported from OpenCode's `sanitizeGemini()` in `transform.ts`.

use crate::LanguageModelTool;
use cocode_protocol::ProviderApi;
use serde_json::Value;

/// Sanitize tool schemas for provider compatibility.
///
/// Currently only Gemini requires sanitization. Other providers pass through unchanged.
pub fn sanitize_tool_schemas(tools: &mut [LanguageModelTool], provider: ProviderApi) {
    if provider != ProviderApi::Gemini {
        return;
    }

    for tool in tools.iter_mut() {
        if let LanguageModelTool::Function(ft) = tool {
            sanitize_schema(&mut ft.input_schema);
        }
    }
}

/// Recursively sanitize a JSON schema value for Gemini compatibility.
fn sanitize_schema(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };

    // Recurse into nested properties
    if let Some(props) = obj.get_mut("properties")
        && let Some(props_obj) = props.as_object_mut()
    {
        for v in props_obj.values_mut() {
            sanitize_schema(v);
        }
    }

    // Recurse into array items
    if let Some(items) = obj.get_mut("items") {
        sanitize_schema(items);
    }

    // Recurse into combiners
    for key in &["anyOf", "oneOf", "allOf"] {
        if let Some(arr) = obj.get_mut(*key)
            && let Some(variants) = arr.as_array_mut()
        {
            for v in variants.iter_mut() {
                sanitize_schema(v);
            }
        }
    }

    // 1. Convert non-string enum values to strings
    if let Some(enum_values) = obj.get_mut("enum")
        && let Some(arr) = enum_values.as_array_mut()
    {
        let has_non_string = arr.iter().any(|v| !v.is_string());
        if has_non_string {
            *arr = arr
                .iter()
                .map(|v| match v {
                    Value::String(_) => v.clone(),
                    _ => Value::String(v.to_string()),
                })
                .collect();
            // Force type to string when enum values were converted
            obj.insert("type".to_string(), Value::String("string".to_string()));
        }
    }

    let has_combiner =
        obj.contains_key("anyOf") || obj.contains_key("oneOf") || obj.contains_key("allOf");

    // 2. Filter required to only include fields in properties
    if let (Some(Value::Array(required)), Some(Value::Object(props))) =
        (obj.get("required").cloned(), obj.get("properties").cloned())
    {
        let filtered: Vec<Value> = required
            .into_iter()
            .filter(|r| r.as_str().is_some_and(|s| props.contains_key(s)))
            .collect();
        obj.insert("required".to_string(), Value::Array(filtered));
    }

    // 3. Array items: ensure items has a type
    if obj.get("type").and_then(|v| v.as_str()) == Some("array") && !has_combiner {
        match obj.get("items") {
            None => {
                obj.insert("items".to_string(), serde_json::json!({"type": "string"}));
            }
            Some(items) if items.as_object().is_some_and(serde_json::Map::is_empty) => {
                obj.insert("items".to_string(), serde_json::json!({"type": "string"}));
            }
            _ => {}
        }
    }

    // 4. Remove properties/required from non-object types (unless combiners present)
    let is_object = obj.get("type").and_then(|v| v.as_str()) == Some("object");
    if !is_object && !has_combiner {
        obj.remove("properties");
        obj.remove("required");
    }
}

#[cfg(test)]
#[path = "schema_sanitize.test.rs"]
mod tests;
