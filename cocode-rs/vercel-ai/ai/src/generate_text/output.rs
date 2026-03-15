//! Output parameter support for generate_text.
//!
//! This module provides types for structured output in generate_text,
//! allowing users to request JSON output that conforms to a schema.

use vercel_ai_provider::JSONSchema;
use vercel_ai_provider::ResponseFormat;

/// Output configuration for structured output in generate_text.
///
/// When specified, the model will generate output that conforms to
/// the provided JSON schema.
#[derive(Debug, Clone)]
pub struct Output {
    /// The JSON schema for the output.
    pub schema: JSONSchema,
    /// Optional name for the schema.
    pub name: Option<String>,
    /// Optional description for the schema.
    pub description: Option<String>,
}

impl Output {
    /// Create a new output configuration with a JSON schema.
    pub fn new(schema: JSONSchema) -> Self {
        Self {
            schema,
            name: None,
            description: None,
        }
    }

    /// Create an output configuration from a schema with a name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Create an output configuration from a schema with a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Create from a serializable type using schemars.
    pub fn from_type<T: schemars::JsonSchema>() -> Self {
        let schema = schemars::schema_for!(T);
        let schema_value = serde_json::to_value(&schema).unwrap_or_default();
        Self::new(schema_value)
    }

    /// Convert to a ResponseFormat for the provider.
    pub fn to_response_format(&self) -> ResponseFormat {
        let name = self.name.clone().unwrap_or_else(|| "output".to_string());
        let mut format = ResponseFormat::json_with_schema(self.schema.clone()).with_name(name);

        if let Some(desc) = &self.description {
            format = format.with_description(desc.clone());
        }

        format
    }
}

/// Output mode enum matching the TS SDK's output strategy pattern.
///
/// Supports different output modes: text (default), object with schema,
/// and array with element schema.
#[derive(Debug, Clone)]
pub enum OutputMode {
    /// Plain text output (default).
    Text,
    /// Structured object output conforming to a JSON schema.
    Object {
        /// The JSON schema for the output object.
        schema: JSONSchema,
        /// Optional name for the schema.
        name: Option<String>,
        /// Optional description for the schema.
        description: Option<String>,
    },
    /// Array output where each element conforms to a JSON schema.
    Array {
        /// The JSON schema for each array element.
        element_schema: JSONSchema,
    },
}

impl OutputMode {
    /// Create a text output mode.
    pub fn text() -> Self {
        Self::Text
    }

    /// Create an object output mode with a schema.
    pub fn object(schema: JSONSchema) -> Self {
        Self::Object {
            schema,
            name: None,
            description: None,
        }
    }

    /// Create an object output mode from a type.
    pub fn object_from_type<T: schemars::JsonSchema>() -> Self {
        let schema = schemars::schema_for!(T);
        let schema_value = serde_json::to_value(&schema).unwrap_or_default();
        Self::Object {
            schema: schema_value,
            name: None,
            description: None,
        }
    }

    /// Create an array output mode with an element schema.
    pub fn array(element_schema: JSONSchema) -> Self {
        Self::Array { element_schema }
    }

    /// Parse a complete output string according to this mode.
    pub fn parse_complete_output(
        &self,
        text: &str,
    ) -> Result<Option<serde_json::Value>, serde_json::Error> {
        match self {
            Self::Text => Ok(Some(serde_json::Value::String(text.to_string()))),
            Self::Object { .. } | Self::Array { .. } => {
                let value: serde_json::Value = serde_json::from_str(text)?;
                Ok(Some(value))
            }
        }
    }

    /// Parse a partial output string (for streaming).
    pub fn parse_partial_output(
        &self,
        text: &str,
    ) -> Result<Option<serde_json::Value>, serde_json::Error> {
        match self {
            Self::Text => Ok(Some(serde_json::Value::String(text.to_string()))),
            Self::Object { .. } | Self::Array { .. } => {
                // Best-effort parse for partial JSON
                match serde_json::from_str::<serde_json::Value>(text) {
                    Ok(value) => Ok(Some(value)),
                    Err(_) => Ok(None), // Partial JSON is not yet parseable
                }
            }
        }
    }

    /// Convert to a ResponseFormat for the provider.
    pub fn to_response_format(&self) -> Option<ResponseFormat> {
        match self {
            Self::Text => None,
            Self::Object {
                schema,
                name,
                description,
            } => {
                let format_name = name.clone().unwrap_or_else(|| "output".to_string());
                let mut format =
                    ResponseFormat::json_with_schema(schema.clone()).with_name(format_name);
                if let Some(desc) = description {
                    format = format.with_description(desc.clone());
                }
                Some(format)
            }
            Self::Array { element_schema } => {
                // Wrap element schema in an array schema
                let array_schema = serde_json::json!({
                    "type": "array",
                    "items": element_schema
                });
                Some(ResponseFormat::json_with_schema(array_schema).with_name("output"))
            }
        }
    }
}

/// Trait for output strategies.
///
/// This trait allows different output formats to be used with generate_text.
pub trait OutputStrategy {
    /// Get the JSON schema for this output, if any.
    fn json_schema(&self) -> Option<&JSONSchema>;

    /// Get the response format for this output.
    fn response_format(&self) -> Option<ResponseFormat>;

    /// Get the name of the output.
    fn name(&self) -> Option<&str>;
}

impl OutputStrategy for Output {
    fn json_schema(&self) -> Option<&JSONSchema> {
        Some(&self.schema)
    }

    fn response_format(&self) -> Option<ResponseFormat> {
        Some(self.to_response_format())
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl OutputStrategy for Option<Output> {
    fn json_schema(&self) -> Option<&JSONSchema> {
        self.as_ref()?.json_schema()
    }

    fn response_format(&self) -> Option<ResponseFormat> {
        self.as_ref()?.response_format()
    }

    fn name(&self) -> Option<&str> {
        self.as_ref()?.name()
    }
}

/// Output that allows any JSON value.
///
/// This type is reserved for future use when implementing output strategies
/// that don't require schema validation.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct NoOutput;

impl OutputStrategy for NoOutput {
    fn json_schema(&self) -> Option<&JSONSchema> {
        None
    }

    fn response_format(&self) -> Option<ResponseFormat> {
        None
    }

    fn name(&self) -> Option<&str> {
        None
    }
}

/// Parsed output result.
///
/// This type is reserved for future use when implementing structured output
/// parsing in generate_text.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ParsedOutput<T> {
    /// The parsed value.
    pub value: T,
    /// The raw JSON string.
    pub raw: String,
}

#[allow(dead_code)]
impl<T> ParsedOutput<T> {
    /// Create a new parsed output.
    pub fn new(value: T, raw: String) -> Self {
        Self { value, raw }
    }
}

#[cfg(test)]
#[path = "output.test.rs"]
mod tests;
