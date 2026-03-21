//! Language model V4 tool call type.
//!
//! Tool calls that the model has generated.

use crate::json_value::JSONValue;
use crate::shared::ProviderMetadata;
use serde::Deserialize;
use serde::Serialize;

/// Tool calls that the model has generated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelV4ToolCall {
    /// The identifier of the tool call. It must be unique across all tool calls.
    pub tool_call_id: String,
    /// The name of the tool that should be called.
    pub tool_name: String,
    /// Stringified JSON object with the tool call arguments.
    /// Must match the parameters schema of the tool.
    pub input: String,
    /// Whether the tool call will be executed by the provider.
    /// If this flag is not set or is false, the tool call will be executed by the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
    /// Whether the tool is dynamic, i.e. defined at runtime.
    /// For example, MCP (Model Context Protocol) tools that are executed by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,
    /// Provider-specific metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelV4ToolCall {
    /// Create a new tool call.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input: input.into(),
            provider_executed: None,
            dynamic: None,
            provider_metadata: None,
        }
    }

    /// Create from JSON value (serializes to string).
    pub fn from_json(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: JSONValue,
    ) -> Self {
        Self::new(tool_call_id, tool_name, input.to_string())
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
    pub fn with_metadata(mut self, metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(metadata);
        self
    }
}

#[cfg(test)]
#[path = "tool_call.test.rs"]
mod tests;
