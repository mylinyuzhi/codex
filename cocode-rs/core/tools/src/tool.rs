//! Tool trait definition with 5-stage pipeline.
//!
//! This module defines the [`Tool`] trait that all tools must implement,
//! providing a standardized execution pipeline.

use crate::context::ToolContext;
use crate::error::ToolError;
use async_trait::async_trait;
use cocode_protocol::{
    ConcurrencySafety, PermissionResult, ToolOutput, ToolResultContent, ValidationError,
    ValidationResult,
};
use hyper_sdk::ToolDefinition;
use serde_json::Value;

/// A tool that can be executed by the agent.
///
/// Tools implement a 5-stage pipeline:
/// 1. **Validate** - Check input validity
/// 2. **Check Permission** - Verify user has granted permission
/// 3. **Execute** - Perform the actual work
/// 4. **Post Process** - Transform output (optional)
/// 5. **Cleanup** - Release resources (optional)
///
/// # Concurrency Safety
///
/// Tools declare their concurrency safety via [`concurrency_safety`](Tool::concurrency_safety):
/// - `Safe` - Can run in parallel with other tools
/// - `Unsafe` - Must run sequentially (e.g., file writes, shell commands)
///
/// # Example
///
/// ```ignore
/// use cocode_tools::{Tool, ToolContext, ToolOutput, ToolError};
/// use async_trait::async_trait;
///
/// struct ReadTool;
///
/// #[async_trait]
/// impl Tool for ReadTool {
///     fn name(&self) -> &str { "Read" }
///     fn description(&self) -> &str { "Read file contents" }
///     fn input_schema(&self) -> serde_json::Value {
///         serde_json::json!({
///             "type": "object",
///             "properties": {
///                 "file_path": {"type": "string"}
///             },
///             "required": ["file_path"]
///         })
///     }
///
///     async fn execute(
///         &self,
///         input: serde_json::Value,
///         ctx: &mut ToolContext,
///     ) -> Result<ToolOutput, ToolError> {
///         let path = input["file_path"].as_str().unwrap();
///         let content = tokio::fs::read_to_string(path).await?;
///         Ok(ToolOutput::text(content))
///     }
/// }
/// ```
#[async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool name.
    fn name(&self) -> &str;

    /// Get the tool description.
    fn description(&self) -> &str;

    /// Get the JSON schema for tool input.
    fn input_schema(&self) -> Value;

    /// Get the concurrency safety of this tool.
    ///
    /// Default is `Safe` - tools can run in parallel.
    /// Override to return `Unsafe` for tools that modify state.
    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    /// Validate the input before execution.
    ///
    /// Default implementation checks against JSON schema.
    async fn validate(&self, input: &Value) -> ValidationResult {
        // Basic validation - check required fields exist
        let schema = self.input_schema();

        if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
            for field in required {
                if let Some(field_name) = field.as_str() {
                    if input.get(field_name).is_none() {
                        return ValidationResult::Invalid {
                            errors: vec![ValidationError::with_path(
                                format!("Missing required field: {field_name}"),
                                field_name,
                            )],
                        };
                    }
                }
            }
        }

        ValidationResult::Valid
    }

    /// Check if the tool has permission to execute.
    ///
    /// Default implementation always allows. Override for tools that
    /// need user permission (e.g., file writes, shell commands).
    async fn check_permission(&self, _input: &Value, _ctx: &ToolContext) -> PermissionResult {
        PermissionResult::Allowed
    }

    /// Execute the tool with the given input.
    ///
    /// This is the main execution method that performs the tool's work.
    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput, ToolError>;

    /// Post-process the output after execution.
    ///
    /// Default implementation returns output unchanged.
    async fn post_process(&self, output: ToolOutput, _ctx: &ToolContext) -> ToolOutput {
        output
    }

    /// Cleanup after execution (success or failure).
    ///
    /// Default implementation does nothing.
    async fn cleanup(&self, _ctx: &ToolContext) {}

    /// Convert to a tool definition for the API.
    fn to_definition(&self) -> ToolDefinition {
        ToolDefinition::full(self.name(), self.description(), self.input_schema())
    }

    /// Check if this tool is safe to run concurrently.
    fn is_concurrent_safe(&self) -> bool {
        matches!(self.concurrency_safety(), ConcurrencySafety::Safe)
    }
}

/// Extension methods for ToolOutput.
pub trait ToolOutputExt {
    /// Create a text output.
    fn text(content: impl Into<String>) -> Self;

    /// Create a structured output.
    fn structured(value: Value) -> Self;

    /// Create an error output.
    fn error(message: impl Into<String>) -> Self;

    /// Create an empty output.
    fn empty() -> Self;
}

impl ToolOutputExt for ToolOutput {
    fn text(content: impl Into<String>) -> Self {
        Self {
            content: ToolResultContent::Text(content.into()),
            is_error: false,
            modifiers: Vec::new(),
        }
    }

    fn structured(value: Value) -> Self {
        Self {
            content: ToolResultContent::Structured(value),
            is_error: false,
            modifiers: Vec::new(),
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            content: ToolResultContent::Text(message.into()),
            is_error: true,
            modifiers: Vec::new(),
        }
    }

    fn empty() -> Self {
        Self {
            content: ToolResultContent::Text(String::new()),
            is_error: false,
            modifiers: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
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
            let message = input["message"]
                .as_str()
                .ok_or_else(|| ToolError::invalid_input("message must be a string"))?;
            Ok(ToolOutput::text(format!("Received: {message}")))
        }
    }

    #[tokio::test]
    async fn test_tool_trait() {
        let tool = DummyTool;
        assert_eq!(tool.name(), "dummy");
        assert!(tool.is_concurrent_safe());
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
}
