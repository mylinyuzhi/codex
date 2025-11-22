//! Dynamic `complete_task` tool generation for subagents.

use crate::client_common::tools::ResponsesApiTool;
use crate::client_common::tools::ToolSpec;
use crate::subagent::definition::OutputConfig;
use crate::tools::spec::JsonSchema;
use std::collections::BTreeMap;

/// Create the `complete_task` tool for a subagent.
///
/// If `output_config` is provided, the tool will have a typed parameter
/// matching the output schema. Otherwise, it accepts a simple `output` string.
pub fn create_complete_task_tool(output_config: Option<&OutputConfig>) -> ToolSpec {
    let (parameters, description) = if let Some(config) = output_config {
        // Typed output based on OutputConfig
        let mut properties = BTreeMap::new();

        // Convert the JsonValue schema to our internal JsonSchema
        let output_schema = json_value_to_schema(&config.schema);
        properties.insert(config.output_name.clone(), output_schema);

        let schema = JsonSchema::Object {
            properties,
            required: Some(vec![config.output_name.clone()]),
            additional_properties: None,
        };

        let desc = format!(
            "Complete the task with your final answer. {}",
            config.description
        );
        (schema, desc)
    } else {
        // Default: simple string output
        let mut properties = BTreeMap::new();
        properties.insert(
            "output".to_string(),
            JsonSchema::String {
                description: Some("Your final answer or result for the task".to_string()),
            },
        );

        let schema = JsonSchema::Object {
            properties,
            required: Some(vec!["output".to_string()]),
            additional_properties: None,
        };

        (
            schema,
            "Complete the task with your final answer.".to_string(),
        )
    };

    ToolSpec::Function(ResponsesApiTool {
        name: "complete_task".to_string(),
        description,
        strict: false,
        parameters,
    })
}

/// Convert a serde_json::Value to our internal JsonSchema.
fn json_value_to_schema(value: &serde_json::Value) -> JsonSchema {
    match value.get("type").and_then(|t| t.as_str()) {
        Some("string") => JsonSchema::String {
            description: value
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from),
        },
        Some("number") => JsonSchema::Number {
            description: value
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from),
        },
        Some("boolean") => JsonSchema::Boolean {
            description: value
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from),
        },
        Some("array") => {
            let items = value
                .get("items")
                .map(json_value_to_schema)
                .unwrap_or(JsonSchema::String { description: None });
            JsonSchema::Array {
                items: Box::new(items),
                description: value
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(String::from),
            }
        }
        Some("object") => {
            let mut properties = BTreeMap::new();
            if let Some(props) = value.get("properties").and_then(|p| p.as_object()) {
                for (key, val) in props {
                    properties.insert(key.clone(), json_value_to_schema(val));
                }
            }
            let required = value.get("required").and_then(|r| r.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });
            JsonSchema::Object {
                properties,
                required,
                additional_properties: None,
            }
        }
        _ => JsonSchema::String { description: None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_create_complete_task_default() {
        let tool = create_complete_task_tool(None);
        match &tool {
            ToolSpec::Function(func) => {
                assert_eq!(func.name, "complete_task");
                assert!(func.description.contains("final answer"));
                match &func.parameters {
                    JsonSchema::Object { properties, .. } => {
                        assert!(properties.contains_key("output"));
                    }
                    _ => panic!("Expected Object schema"),
                }
            }
            _ => panic!("Expected Function tool"),
        }
    }

    #[test]
    fn test_create_complete_task_with_output_config() {
        let config = OutputConfig {
            output_name: "result".to_string(),
            description: "The search result".to_string(),
            schema: json!({
                "type": "object",
                "properties": {
                    "found": {"type": "boolean"},
                    "data": {"type": "string"}
                },
                "required": ["found"]
            }),
        };

        let tool = create_complete_task_tool(Some(&config));
        match &tool {
            ToolSpec::Function(func) => {
                assert_eq!(func.name, "complete_task");
                assert!(func.description.contains("search result"));
                match &func.parameters {
                    JsonSchema::Object {
                        properties,
                        required,
                        ..
                    } => {
                        assert!(properties.contains_key("result"));
                        assert_eq!(required.as_ref().unwrap(), &vec!["result".to_string()]);
                    }
                    _ => panic!("Expected Object schema"),
                }
            }
            _ => panic!("Expected Function tool"),
        }
    }
}
