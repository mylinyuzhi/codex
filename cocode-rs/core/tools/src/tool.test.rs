use super::*;

struct DummyTool;

#[async_trait]
impl Tool for DummyTool {
    fn name(&self) -> &str {
        "dummy"
    }

    fn description(&self) -> &str {
        "A dummy tool for testing"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {"type": "string"}
            },
            "required": ["message"]
        })
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &mut ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let message = input["message"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "message must be a string",
            }
            .build()
        })?;
        Ok(ToolOutput::text(format!("Received: {message}")))
    }
}

#[tokio::test]
async fn test_tool_trait() {
    let tool = DummyTool;
    assert_eq!(tool.name(), "dummy");
    assert!(tool.is_concurrent_safe());
    // New trait methods with defaults
    assert!(tool.is_concurrency_safe_for(&serde_json::json!({})));
    assert!(tool.is_read_only());
    assert_eq!(tool.max_result_size_chars(), 30_000);
    assert!(tool.feature_gate().is_none());
}

#[tokio::test]
async fn test_validation() {
    let tool = DummyTool;

    // Valid input
    let valid = serde_json::json!({"message": "hello"});
    assert!(matches!(
        tool.validate(&valid).await,
        ValidationResult::Valid
    ));

    // Missing required field
    let invalid = serde_json::json!({});
    assert!(matches!(
        tool.validate(&invalid).await,
        ValidationResult::Invalid { .. }
    ));
}

#[test]
fn test_tool_output_ext() {
    let text_output = ToolOutput::text("hello");
    assert!(!text_output.is_error);

    let error_output = ToolOutput::error("something failed");
    assert!(error_output.is_error);

    let structured = ToolOutput::structured(serde_json::json!({"key": "value"}));
    assert!(!structured.is_error);
}

#[test]
fn test_to_definition() {
    let tool = DummyTool;
    let def = tool.to_definition();
    assert_eq!(def.name, "dummy");
    assert!(def.description.is_some());
}
