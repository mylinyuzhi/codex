//! Tests for [`ToolSchemaValidator`] and [`effective_tool_schema`].
//!
//! Covers:
//! - properties-envelope wrap for tools without an explicit
//!   `input_json_schema` override.
//! - `input_json_schema` override wins when present.
//! - Validator rejects `required` missing, unknown field with
//!   `additionalProperties: false`, enum mismatch, nested type
//!   mismatch — exercising the plan's semantic-parity surface.
//! - Cache hit doesn't re-compile (asserted via counter proxy).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolResult;
use serde_json::{Value, json};

use super::*;
use crate::context::ToolUseContext;
use crate::error::ToolError;
use crate::traits::DescriptionOptions;

/// Test tool with either a full `input_json_schema` override or
/// just a properties map. Counts how many times `input_json_schema`
/// is called, which lets us assert cache behavior.
struct TestTool {
    name: String,
    properties: HashMap<String, Value>,
    json_schema: Option<Value>,
    json_schema_calls: Arc<AtomicI32>,
}

#[async_trait::async_trait]
impl crate::traits::Tool for TestTool {
    fn id(&self) -> ToolId {
        ToolId::Custom(self.name.clone())
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self, _: &Value, _: &DescriptionOptions) -> String {
        "test".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: self.properties.clone(),
        }
    }
    fn input_json_schema(&self) -> Option<Value> {
        self.json_schema_calls.fetch_add(1, Ordering::SeqCst);
        self.json_schema.clone()
    }
    async fn execute(
        &self,
        _input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        unreachable!("tests don't execute")
    }
}

fn tool_with_properties(name: &str, props: HashMap<String, Value>) -> TestTool {
    TestTool {
        name: name.into(),
        properties: props,
        json_schema: None,
        json_schema_calls: Arc::new(AtomicI32::new(0)),
    }
}

fn tool_with_full_schema(name: &str, schema: Value) -> TestTool {
    TestTool {
        name: name.into(),
        properties: HashMap::new(),
        json_schema: Some(schema),
        json_schema_calls: Arc::new(AtomicI32::new(0)),
    }
}

#[test]
fn test_effective_schema_wraps_properties_in_object_envelope() {
    let mut props = HashMap::new();
    props.insert("file_path".into(), json!({"type": "string"}));
    let tool = tool_with_properties("Read", props);
    let schema = effective_tool_schema(&tool);
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["file_path"].is_object());
}

#[test]
fn test_effective_schema_uses_input_json_schema_override_when_present() {
    let explicit = json!({
        "type": "object",
        "properties": {"x": {"type": "integer"}},
        "required": ["x"],
        "additionalProperties": false,
    });
    let tool = tool_with_full_schema("Override", explicit.clone());
    let schema = effective_tool_schema(&tool);
    assert_eq!(schema, explicit);
}

#[tokio::test]
async fn test_validator_accepts_valid_input() {
    let mut props = HashMap::new();
    props.insert("name".into(), json!({"type": "string"}));
    let tool = tool_with_properties("Greet", props);
    let validator = ToolSchemaValidator::new();
    validator
        .validate(&tool, &json!({"name": "alice"}))
        .await
        .expect("should accept valid input");
}

#[tokio::test]
async fn test_validator_rejects_required_missing() {
    let schema = json!({
        "type": "object",
        "properties": {"file_path": {"type": "string"}},
        "required": ["file_path"],
    });
    let tool = tool_with_full_schema("Read", schema);
    let validator = ToolSchemaValidator::new();
    let err = validator.validate(&tool, &json!({})).await.unwrap_err();
    match err {
        SchemaValidationError::Rejected { message } => {
            assert!(
                message.to_lowercase().contains("required") || message.contains("file_path"),
                "error must mention the missing required field; got: {message}"
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[tokio::test]
async fn test_validator_rejects_unknown_field_under_additional_properties_false() {
    let schema = json!({
        "type": "object",
        "properties": {"known": {"type": "string"}},
        "additionalProperties": false,
    });
    let tool = tool_with_full_schema("Strict", schema);
    let validator = ToolSchemaValidator::new();
    let err = validator
        .validate(&tool, &json!({"known": "ok", "extra": 1}))
        .await
        .unwrap_err();
    assert!(matches!(err, SchemaValidationError::Rejected { .. }));
}

#[tokio::test]
async fn test_validator_rejects_enum_mismatch() {
    let schema = json!({
        "type": "object",
        "properties": {
            "color": {"type": "string", "enum": ["red", "green", "blue"]}
        },
        "required": ["color"],
    });
    let tool = tool_with_full_schema("Color", schema);
    let validator = ToolSchemaValidator::new();
    let err = validator
        .validate(&tool, &json!({"color": "purple"}))
        .await
        .unwrap_err();
    assert!(matches!(err, SchemaValidationError::Rejected { .. }));
}

#[tokio::test]
async fn test_validator_rejects_nested_type_mismatch() {
    let schema = json!({
        "type": "object",
        "properties": {
            "nested": {
                "type": "object",
                "properties": {"count": {"type": "integer"}},
                "required": ["count"],
            }
        },
        "required": ["nested"],
    });
    let tool = tool_with_full_schema("Nested", schema);
    let validator = ToolSchemaValidator::new();
    let err = validator
        .validate(&tool, &json!({"nested": {"count": "not-an-int"}}))
        .await
        .unwrap_err();
    assert!(matches!(err, SchemaValidationError::Rejected { .. }));
}

#[tokio::test]
async fn test_validator_caches_compiled_schema_per_tool_id() {
    let tool = tool_with_full_schema(
        "Cached",
        json!({"type": "object", "properties": {"x": {"type": "string"}}}),
    );
    let calls = tool.json_schema_calls.clone();
    let validator = ToolSchemaValidator::new();

    // First call compiles + caches.
    validator
        .validate(&tool, &json!({"x": "hi"}))
        .await
        .unwrap();
    let after_first = calls.load(Ordering::SeqCst);
    // Second call hits cache — input_json_schema should NOT be
    // called again.
    validator
        .validate(&tool, &json!({"x": "world"}))
        .await
        .unwrap();
    let after_second = calls.load(Ordering::SeqCst);
    assert_eq!(
        after_first, after_second,
        "cache hit must not re-call input_json_schema; calls went {after_first} → {after_second}"
    );
}

#[tokio::test]
async fn test_validator_clear_invalidates_cache() {
    let tool = tool_with_full_schema(
        "Clearable",
        json!({"type": "object", "properties": {"x": {"type": "string"}}}),
    );
    let calls = tool.json_schema_calls.clone();
    let validator = ToolSchemaValidator::new();

    validator
        .validate(&tool, &json!({"x": "hi"}))
        .await
        .unwrap();
    let before_clear = calls.load(Ordering::SeqCst);
    validator.clear().await;
    validator
        .validate(&tool, &json!({"x": "hi"}))
        .await
        .unwrap();
    let after_reclear = calls.load(Ordering::SeqCst);
    assert!(
        after_reclear > before_clear,
        "cache clear must force a recompile"
    );
}
