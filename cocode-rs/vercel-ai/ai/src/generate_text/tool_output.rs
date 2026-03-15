//! Tool output types for generate_text.
//!
//! This module provides types for representing tool execution outputs.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// Output from a tool execution.
///
/// This type represents the result of executing a tool, which can be
/// various types of content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolOutput {
    /// Text output.
    Text(String),
    /// JSON output.
    Json(Value),
    /// Multiple content parts.
    Multi(Vec<ToolOutputContent>),
}

impl ToolOutput {
    /// Create a text output.
    pub fn text(content: impl Into<String>) -> Self {
        ToolOutput::Text(content.into())
    }

    /// Create a JSON output.
    pub fn json(value: Value) -> Self {
        ToolOutput::Json(value)
    }

    /// Create a multi-part output.
    pub fn multi(parts: Vec<ToolOutputContent>) -> Self {
        ToolOutput::Multi(parts)
    }

    /// Check if the output is text.
    pub fn is_text(&self) -> bool {
        matches!(self, ToolOutput::Text(_))
    }

    /// Check if the output is JSON.
    pub fn is_json(&self) -> bool {
        matches!(self, ToolOutput::Json(_))
    }

    /// Get the text content if this is a text output.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ToolOutput::Text(t) => Some(t),
            _ => None,
        }
    }

    /// Get the JSON value if this is a JSON output.
    pub fn as_json(&self) -> Option<&Value> {
        match self {
            ToolOutput::Json(v) => Some(v),
            _ => None,
        }
    }

    /// Convert to a JSON value.
    pub fn to_json(&self) -> Value {
        match self {
            ToolOutput::Text(t) => Value::String(t.clone()),
            ToolOutput::Json(v) => v.clone(),
            ToolOutput::Multi(parts) => {
                Value::Array(parts.iter().map(ToolOutputContent::to_json).collect())
            }
        }
    }

    /// Convert to a string representation.
    pub fn to_string_output(&self) -> String {
        match self {
            ToolOutput::Text(t) => t.clone(),
            ToolOutput::Json(v) => v.to_string(),
            ToolOutput::Multi(parts) => parts
                .iter()
                .map(ToolOutputContent::to_string_output)
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

impl From<String> for ToolOutput {
    fn from(s: String) -> Self {
        ToolOutput::Text(s)
    }
}

impl From<&str> for ToolOutput {
    fn from(s: &str) -> Self {
        ToolOutput::Text(s.to_string())
    }
}

impl From<Value> for ToolOutput {
    fn from(v: Value) -> Self {
        ToolOutput::Json(v)
    }
}

/// Content within a multi-part tool output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToolOutputContent {
    /// Text content.
    Text {
        /// The text content.
        text: String,
    },
    /// Image content (base64 or URL).
    Image {
        /// The image data or URL.
        image: String,
        /// Optional media type (MIME type).
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
    /// JSON content.
    Json {
        /// The JSON value.
        value: Value,
    },
}

impl ToolOutputContent {
    /// Create text content.
    pub fn text(content: impl Into<String>) -> Self {
        ToolOutputContent::Text {
            text: content.into(),
        }
    }

    /// Create image content.
    pub fn image(data: impl Into<String>, mime_type: Option<String>) -> Self {
        ToolOutputContent::Image {
            image: data.into(),
            mime_type,
        }
    }

    /// Create JSON content.
    pub fn json(value: Value) -> Self {
        ToolOutputContent::Json { value }
    }

    /// Check if this is text content.
    pub fn is_text(&self) -> bool {
        matches!(self, ToolOutputContent::Text { .. })
    }

    /// Check if this is image content.
    pub fn is_image(&self) -> bool {
        matches!(self, ToolOutputContent::Image { .. })
    }

    /// Convert to a JSON value.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// Convert to a string representation.
    pub fn to_string_output(&self) -> String {
        match self {
            ToolOutputContent::Text { text } => text.clone(),
            ToolOutputContent::Image { image, .. } => image.clone(),
            ToolOutputContent::Json { value } => value.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tool_output_text() {
        let output = ToolOutput::text("Hello");
        assert!(output.is_text());
        assert_eq!(output.as_text(), Some("Hello"));
        assert!(!output.is_json());
    }

    #[test]
    fn test_tool_output_json() {
        let output = ToolOutput::json(json!({ "key": "value" }));
        assert!(output.is_json());
        assert!(!output.is_text());
        assert_eq!(output.as_json(), Some(&json!({ "key": "value" })));
    }

    #[test]
    fn test_tool_output_multi() {
        let output = ToolOutput::multi(vec![
            ToolOutputContent::text("Part 1"),
            ToolOutputContent::text("Part 2"),
        ]);
        assert!(!output.is_text());
        assert!(!output.is_json());
    }

    #[test]
    fn test_tool_output_to_json() {
        let text_output = ToolOutput::text("Hello");
        assert_eq!(text_output.to_json(), json!("Hello"));

        let json_output = ToolOutput::json(json!({ "a": 1 }));
        assert_eq!(json_output.to_json(), json!({ "a": 1 }));
    }

    #[test]
    fn test_tool_output_to_string() {
        let text_output = ToolOutput::text("Hello");
        assert_eq!(text_output.to_string_output(), "Hello");

        let json_output = ToolOutput::json(json!({ "a": 1 }));
        assert_eq!(json_output.to_string_output(), r#"{"a":1}"#);
    }

    #[test]
    fn test_tool_output_from() {
        let output1: ToolOutput = "text".into();
        assert!(output1.is_text());

        let output2: ToolOutput = String::from("text").into();
        assert!(output2.is_text());

        let output3: ToolOutput = json!({ "key": "value" }).into();
        assert!(output3.is_json());
    }

    #[test]
    fn test_tool_output_content_text() {
        let content = ToolOutputContent::text("Hello");
        assert!(content.is_text());
        assert!(!content.is_image());
        assert_eq!(content.to_string_output(), "Hello");
    }

    #[test]
    fn test_tool_output_content_image() {
        let content = ToolOutputContent::image("base64data", Some("image/png".to_string()));
        assert!(content.is_image());
        assert!(!content.is_text());
    }

    #[test]
    fn test_tool_output_content_json() {
        let content = ToolOutputContent::json(json!({ "a": 1 }));
        assert!(!content.is_text());
        assert!(!content.is_image());
    }
}
