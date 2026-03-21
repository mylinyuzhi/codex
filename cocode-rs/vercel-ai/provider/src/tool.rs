//! Tool invocation and tool call/result types.
//!
//! Tool definition types have been consolidated into
//! [`crate::language_model::v4::function_tool::LanguageModelV4FunctionTool`].
//! The type alias `ToolDefinitionV4` in the crate root points there.

use serde::Deserialize;
use serde::Serialize;

use crate::json_value::JSONValue;

/// A tool invocation (combination of tool call and optional result).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInvocation {
    /// The tool call.
    pub tool_call_id: String,
    /// The tool name.
    pub tool_name: String,
    /// The tool arguments.
    pub input: JSONValue,
    /// The tool result (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<JSONValue>,
    /// Whether the result is an error.
    #[serde(default)]
    pub is_error: bool,
}

impl ToolInvocation {
    /// Create a new tool invocation without a result.
    pub fn pending(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: JSONValue,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            output: None,
            is_error: false,
        }
    }

    /// Add a result to the invocation.
    pub fn with_output(mut self, output: JSONValue) -> Self {
        self.output = Some(output);
        self
    }

    /// Add an error result to the invocation.
    pub fn with_error(mut self, error: JSONValue) -> Self {
        self.output = Some(error);
        self.is_error = true;
        self
    }
}

/// A tool call in a response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    /// The tool call ID.
    pub tool_call_id: String,
    /// The tool name.
    pub tool_name: String,
    /// The tool arguments.
    pub input: JSONValue,
    /// Whether the tool call will be executed by the provider.
    /// If this flag is not set or is false, the tool call will be executed by the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
    /// Whether the tool is dynamic, i.e. defined at runtime.
    /// For example, MCP (Model Context Protocol) tools that are executed by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,
    /// Additional provider-specific metadata for the tool call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<crate::shared::ProviderMetadata>,
}

impl ToolCall {
    /// Create a new tool call.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: JSONValue,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            provider_executed: None,
            dynamic: None,
            provider_metadata: None,
        }
    }

    /// Set whether this is a provider-executed tool call.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }

    /// Set whether this is a dynamic tool call.
    pub fn with_dynamic(mut self, dynamic: bool) -> Self {
        self.dynamic = Some(dynamic);
        self
    }

    /// Set provider metadata.
    pub fn with_provider_metadata(mut self, metadata: crate::shared::ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

/// A tool result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    /// The tool call ID.
    pub tool_call_id: String,
    /// The tool name.
    pub tool_name: String,
    /// The result content.
    pub output: JSONValue,
    /// Whether this is an error result.
    #[serde(default)]
    pub is_error: bool,
}

impl ToolResult {
    /// Create a new tool result.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        output: JSONValue,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            output,
            is_error: false,
        }
    }

    /// Create an error result.
    pub fn error(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        error: JSONValue,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            output: error,
            is_error: true,
        }
    }
}

#[cfg(test)]
#[path = "tool.test.rs"]
mod tests;
