//! Tool error types for generate_text.
//!
//! This module provides error types for tool execution failures.

use std::fmt;

/// Error that occurred during tool execution.
#[derive(Debug, Clone)]
pub struct ToolError {
    /// The tool call ID that caused the error.
    pub tool_call_id: String,
    /// The tool name that caused the error.
    pub tool_name: String,
    /// The error message.
    pub message: String,
    /// Whether the error is retryable.
    pub is_retryable: bool,
}

impl ToolError {
    /// Create a new tool error.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            message: message.into(),
            is_retryable: false,
        }
    }

    /// Create a tool error from an error message.
    pub fn from_message(message: impl Into<String>) -> Self {
        Self {
            tool_call_id: String::new(),
            tool_name: String::new(),
            message: message.into(),
            is_retryable: false,
        }
    }

    /// Set the tool call ID.
    pub fn with_tool_call_id(mut self, id: impl Into<String>) -> Self {
        self.tool_call_id = id.into();
        self
    }

    /// Set the tool name.
    pub fn with_tool_name(mut self, name: impl Into<String>) -> Self {
        self.tool_name = name.into();
        self
    }

    /// Mark the error as retryable.
    pub fn retryable(mut self) -> Self {
        self.is_retryable = true;
        self
    }

    /// Check if the error is retryable.
    pub fn is_retryable(&self) -> bool {
        self.is_retryable
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.tool_name.is_empty() {
            write!(f, "Tool error: {}", self.message)
        } else if self.tool_call_id.is_empty() {
            write!(f, "Tool '{}' error: {}", self.tool_name, self.message)
        } else {
            write!(
                f,
                "Tool '{}' ({}) error: {}",
                self.tool_name, self.tool_call_id, self.message
            )
        }
    }
}

impl std::error::Error for ToolError {}

/// Result of a tool execution that may fail.
pub type ToolResult<T> = Result<T, ToolError>;

/// Create a tool error from a message.
pub fn tool_error(message: impl Into<String>) -> ToolError {
    ToolError::from_message(message)
}

/// Create a tool error with tool context.
pub fn tool_error_with_context(
    tool_call_id: impl Into<String>,
    tool_name: impl Into<String>,
    message: impl Into<String>,
) -> ToolError {
    ToolError::new(tool_call_id, tool_name, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_error_new() {
        let error = ToolError::new("call_123", "my_tool", "Something went wrong");
        assert_eq!(error.tool_call_id, "call_123");
        assert_eq!(error.tool_name, "my_tool");
        assert_eq!(error.message, "Something went wrong");
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_tool_error_from_message() {
        let error = ToolError::from_message("Generic error");
        assert!(error.tool_call_id.is_empty());
        assert!(error.tool_name.is_empty());
        assert_eq!(error.message, "Generic error");
    }

    #[test]
    fn test_tool_error_builder() {
        let error = ToolError::from_message("Error")
            .with_tool_call_id("call_1")
            .with_tool_name("test_tool")
            .retryable();

        assert_eq!(error.tool_call_id, "call_1");
        assert_eq!(error.tool_name, "test_tool");
        assert!(error.is_retryable());
    }

    #[test]
    fn test_tool_error_display() {
        let error1 = ToolError::from_message("Error");
        assert_eq!(error1.to_string(), "Tool error: Error");

        let error2 = ToolError::new("", "my_tool", "Failed");
        assert_eq!(error2.to_string(), "Tool 'my_tool' error: Failed");

        let error3 = ToolError::new("call_1", "my_tool", "Failed");
        assert_eq!(error3.to_string(), "Tool 'my_tool' (call_1) error: Failed");
    }

    #[test]
    fn test_tool_error_functions() {
        let err1 = tool_error("Simple error");
        assert_eq!(err1.message, "Simple error");

        let err2 = tool_error_with_context("id_1", "tool_1", "Context error");
        assert_eq!(err2.tool_call_id, "id_1");
        assert_eq!(err2.tool_name, "tool_1");
    }
}
