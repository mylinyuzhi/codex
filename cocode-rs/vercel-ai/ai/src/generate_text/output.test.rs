//! Tests for output.rs

use super::*;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct Person {
    name: String,
    age: u32,
}

#[test]
fn test_output_new() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });

    let output = Output::new(schema.clone());
    assert_eq!(output.schema, schema);
    assert!(output.name.is_none());
    assert!(output.description.is_none());
}

#[test]
fn test_output_with_name() {
    let schema = serde_json::json!({"type": "string"});
    let output = Output::new(schema).with_name("myOutput");

    assert_eq!(output.name, Some("myOutput".to_string()));
}

#[test]
fn test_output_from_type() {
    let output = Output::from_type::<Person>();
    assert!(output.schema.is_object());
}

#[test]
fn test_output_to_response_format() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });
    let output = Output::new(schema).with_name("person");

    let format = output.to_response_format();
    // ResponseFormat should be JSON
    assert!(matches!(format, ResponseFormat::Json { .. }));
}

#[test]
fn test_output_mode_text() {
    let mode = OutputMode::text();
    assert!(mode.to_response_format().is_none());
}

#[test]
fn test_output_mode_object() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        }
    });
    let mode = OutputMode::object(schema);
    assert!(mode.to_response_format().is_some());
}

#[test]
fn test_output_mode_object_from_type() {
    let mode = OutputMode::object_from_type::<Person>();
    assert!(mode.to_response_format().is_some());
}

#[test]
fn test_output_mode_array() {
    let element = serde_json::json!({"type": "string"});
    let mode = OutputMode::array(element);
    let format = mode.to_response_format();
    assert!(format.is_some());
}

#[test]
fn test_output_mode_parse_complete_text() {
    let mode = OutputMode::text();
    let result = mode.parse_complete_output("hello").unwrap();
    assert_eq!(result, Some(serde_json::Value::String("hello".to_string())));
}

#[test]
fn test_output_mode_parse_complete_object() {
    let mode = OutputMode::object(serde_json::json!({"type": "object"}));
    let result = mode.parse_complete_output(r#"{"name":"Alice"}"#).unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap()["name"], "Alice");
}

#[test]
fn test_output_mode_parse_partial_incomplete() {
    let mode = OutputMode::object(serde_json::json!({"type": "object"}));
    let result = mode.parse_partial_output(r#"{"name":"Ali"#).unwrap();
    // Partial JSON should return None since it's not parseable
    assert!(result.is_none());
}
