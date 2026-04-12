//! Parse tool call arguments.
//!
//! This module provides utilities for parsing and validating tool call
//! arguments from language model responses.

use serde_json::Value;

use crate::error::InvalidToolInputError;

/// Parse a tool call input string into a JSON value.
pub fn parse_tool_call_input(input: &str) -> Result<Value, InvalidToolInputError> {
    if input.trim().is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }

    serde_json::from_str(input).map_err(|_e| {
        InvalidToolInputError::new("unknown", input)
            .with_message("Failed to parse tool input".to_string())
    })
}

/// Result of parsing a tool call.
#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    /// The tool call ID.
    pub tool_call_id: String,
    /// The tool name.
    pub tool_name: String,
    /// The parsed input arguments.
    pub input: Value,
    /// Whether the tool call is dynamic.
    pub is_dynamic: bool,
    /// Whether the tool call is provider-executed.
    pub is_provider_executed: bool,
}

impl ParsedToolCall {
    /// Create a new parsed tool call.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: Value,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            is_dynamic: false,
            is_provider_executed: false,
        }
    }

    /// Mark as dynamic.
    pub fn with_dynamic(mut self, is_dynamic: bool) -> Self {
        self.is_dynamic = is_dynamic;
        self
    }

    /// Mark as provider-executed.
    pub fn with_provider_executed(mut self, is_provider_executed: bool) -> Self {
        self.is_provider_executed = is_provider_executed;
        self
    }
}

#[cfg(test)]
#[path = "parse_tool_call.test.rs"]
mod tests;
