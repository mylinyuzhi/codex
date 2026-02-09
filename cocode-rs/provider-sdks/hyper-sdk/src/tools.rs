//! Tool/function definitions and calls.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// Definition of a tool that can be called by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Name of the tool.
    pub name: String,
    /// Description of what the tool does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the tool's parameters.
    pub parameters: Value,
    /// Custom tool format (OpenAI-only). When set, sent as `type: "custom"` tool.
    /// Non-OpenAI providers ignore this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_format: Option<Value>,
}

impl ToolDefinition {
    /// Create a new tool definition.
    pub fn new(name: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            description: None,
            parameters,
            custom_format: None,
        }
    }

    /// Set the tool description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Create a tool definition with all fields.
    pub fn full(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            parameters,
            custom_format: None,
        }
    }

    /// Create a custom tool definition (OpenAI-only).
    ///
    /// The `custom_format` value is sent as the `format` field of an OpenAI
    /// `type: "custom"` tool. Non-OpenAI providers silently skip custom tools.
    pub fn custom(
        name: impl Into<String>,
        description: impl Into<String>,
        custom_format: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            parameters: Value::Null,
            custom_format: Some(custom_format),
        }
    }
}

/// How the model should choose which tool to call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    /// Model decides whether to call tools.
    Auto,
    /// Model must call a tool.
    Required,
    /// Model must not call any tools.
    None,
    /// Model must call a specific tool.
    Tool {
        /// Name of the tool to call.
        name: String,
    },
}

impl Default for ToolChoice {
    fn default() -> Self {
        ToolChoice::Auto
    }
}

/// A tool call made by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique ID for this tool call.
    pub id: String,
    /// Name of the tool being called.
    pub name: String,
    /// Arguments as JSON.
    pub arguments: Value,
}

impl ToolCall {
    /// Create a new tool call.
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }

    /// Get a reference to the arguments as a JSON value.
    pub fn arguments(&self) -> &Value {
        &self.arguments
    }

    /// Parse the arguments as a specific type.
    pub fn parse_arguments<T: for<'de> Deserialize<'de>>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.arguments.clone())
    }
}

/// Content for a tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    /// Plain text result.
    Text(String),
    /// Structured JSON result.
    Json(Value),
    /// Multiple content blocks (for complex results).
    Blocks(Vec<ToolResultBlock>),
}

impl ToolResultContent {
    /// Create a text result.
    pub fn text(text: impl Into<String>) -> Self {
        ToolResultContent::Text(text.into())
    }

    /// Create a JSON result.
    pub fn json(value: Value) -> Self {
        ToolResultContent::Json(value)
    }

    /// Get as text if this is a text result.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ToolResultContent::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Convert to a text string, handling all content types.
    ///
    /// - `Text`: returns the string directly
    /// - `Json`: serializes to JSON string
    /// - `Blocks`: concatenates all text blocks, ignoring images
    pub fn to_text(&self) -> String {
        match self {
            ToolResultContent::Text(s) => s.clone(),
            ToolResultContent::Json(v) => v.to_string(),
            ToolResultContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ToolResultBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
        }
    }
}

/// A block within a tool result (for complex results).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultBlock {
    /// Text content.
    Text {
        /// The text content.
        text: String,
    },
    /// Image content.
    Image {
        /// Base64-encoded image data.
        data: String,
        /// MIME type.
        media_type: String,
    },
}

#[cfg(test)]
#[path = "tools.test.rs"]
mod tests;
