//! Core types for Google Generative AI (Gemini) API.
//!
//! This module contains all the data structures used for request/response
//! communication with the Gemini API.

use base64::Engine;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de::Error as DeError;
use std::collections::HashMap;
use tracing::warn;

// ============================================================================
// Base64 Serde Helpers (for bytes fields like thought_signature)
// ============================================================================

fn serialize_bytes_base64<S>(data: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match data {
        Some(bytes) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            serializer.serialize_some(&encoded)
        }
        None => serializer.serialize_none(),
    }
}

fn deserialize_bytes_base64<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        Some(s) => base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map(Some)
            .map_err(|e| DeError::custom(format!("base64 decode error: {e}"))),
        None => Ok(None),
    }
}

// ============================================================================
// Enums
// ============================================================================

/// The reason why the model stopped generating tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FinishReason {
    #[default]
    FinishReasonUnspecified,
    Stop,
    MaxTokens,
    Safety,
    Recitation,
    Language,
    Other,
    Blocklist,
    ProhibitedContent,
    Spii,
    MalformedFunctionCall,
    ImageSafety,
    UnexpectedToolCall,
}

/// Harm category for safety ratings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmCategory {
    HarmCategoryUnspecified,
    HarmCategoryHarassment,
    HarmCategoryHateSpeech,
    HarmCategorySexuallyExplicit,
    HarmCategoryDangerousContent,
    HarmCategoryCivicIntegrity,
}

/// Harm probability levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmProbability {
    HarmProbabilityUnspecified,
    Negligible,
    Low,
    Medium,
    High,
}

/// Harm block threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarmBlockThreshold {
    HarmBlockThresholdUnspecified,
    BlockLowAndAbove,
    BlockMediumAndAbove,
    BlockOnlyHigh,
    BlockNone,
    Off,
}

/// The reason why the prompt was blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BlockedReason {
    BlockedReasonUnspecified,
    Safety,
    Other,
    Blocklist,
    ProhibitedContent,
    ImageSafety,
}

/// Function calling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FunctionCallingMode {
    #[default]
    ModeUnspecified,
    Auto,
    Any,
    None,
    /// Validated function calls with constrained decoding.
    Validated,
}

/// JSON Schema type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SchemaType {
    TypeUnspecified,
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
    Null,
}

/// The level of thinking tokens that the model should generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ThinkingLevel {
    #[default]
    ThinkingLevelUnspecified,
    Low,
    Medium,
    High,
}

/// Programming language for code execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Language {
    #[default]
    LanguageUnspecified,
    Python,
}

/// Outcome of the code execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Outcome {
    #[default]
    OutcomeUnspecified,
    OutcomeOk,
    OutcomeFailed,
    OutcomeDeadlineExceeded,
}

/// Function calling behavior (blocking vs non-blocking).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Behavior {
    #[default]
    BehaviorUnspecified,
    Blocking,
    NonBlocking,
}

/// Specifies how the function response should be scheduled in the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FunctionResponseScheduling {
    #[default]
    SchedulingUnspecified,
    /// Only add the result to the conversation context, do not trigger generation.
    Silent,
    /// Add the result and prompt generation without interrupting ongoing generation.
    WhenIdle,
    /// Add the result, interrupt ongoing generation and prompt to generate output.
    Interrupt,
}

/// Media modality for token counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MediaModality {
    #[default]
    ModalityUnspecified,
    Text,
    Image,
    Audio,
    Video,
    Document,
}

/// Media resolution for parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PartMediaResolution {
    #[default]
    MediaResolutionUnspecified,
    MediaResolutionLow,
    MediaResolutionMedium,
    MediaResolutionHigh,
}

// ============================================================================
// Content Parts
// ============================================================================

/// Content blob (inline binary data).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Blob {
    /// Raw bytes, base64 encoded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,

    /// The IANA standard MIME type of the source data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl Blob {
    pub fn new(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            data: Some(data.into()),
            mime_type: Some(mime_type.into()),
        }
    }

    /// Create a Blob from raw bytes (will be base64 encoded).
    pub fn from_bytes(data: &[u8], mime_type: impl Into<String>) -> Self {
        use base64::Engine;
        Self {
            data: Some(base64::engine::general_purpose::STANDARD.encode(data)),
            mime_type: Some(mime_type.into()),
        }
    }
}

/// URI based data reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    /// URI of the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_uri: Option<String>,

    /// The IANA standard MIME type of the source data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl FileData {
    pub fn new(file_uri: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            file_uri: Some(file_uri.into()),
            mime_type: Some(mime_type.into()),
        }
    }
}

/// Partial argument value of the function call (for streaming).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PartialArg {
    /// Represents a null value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub null_value: Option<String>,

    /// Number value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_value: Option<f64>,

    /// String value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_value: Option<String>,

    /// Boolean value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bool_value: Option<bool>,

    /// JSON path for the partial argument.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_path: Option<String>,

    /// Whether this is not the last part of the same json_path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub will_continue: Option<bool>,
}

/// A function call predicted by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCall {
    /// The unique id of the function call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// The name of the function to call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The function parameters and values in JSON object format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,

    /// Partial argument values (for streaming function call arguments).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_args: Option<Vec<PartialArg>>,

    /// Whether this is not the last part of the FunctionCall.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub will_continue: Option<bool>,
}

impl FunctionCall {
    pub fn new(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self {
            id: None,
            name: Some(name.into()),
            args: Some(args),
            partial_args: None,
            will_continue: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Raw media bytes for function response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponseBlob {
    /// The IANA standard MIME type of the source data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Inline media bytes (base64 encoded in JSON).
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_bytes_base64",
        deserialize_with = "deserialize_bytes_base64"
    )]
    pub data: Option<Vec<u8>>,

    /// Display name of the blob.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// URI based data for function response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponseFileData {
    /// URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_uri: Option<String>,

    /// The IANA standard MIME type of the source data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Display name of the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// A datatype containing media that is part of a FunctionResponse message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponsePart {
    /// Inline media bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<FunctionResponseBlob>,

    /// URI based data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_data: Option<FunctionResponseFileData>,
}

/// The result of a function call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponse {
    /// The id of the function call this response is for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// The name of the function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The function response in JSON object format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<serde_json::Value>,

    /// Whether more responses are coming for this function call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub will_continue: Option<bool>,

    /// Scheduling for the response in the conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduling: Option<FunctionResponseScheduling>,

    /// Multi-part function response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<FunctionResponsePart>>,
}

impl FunctionResponse {
    pub fn new(name: impl Into<String>, response: serde_json::Value) -> Self {
        Self {
            id: None,
            name: Some(name.into()),
            response: Some(response),
            will_continue: None,
            scheduling: None,
            parts: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Code authored and executed by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableCode {
    /// Programming language of the code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,

    /// The code to be executed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Result of executing the code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodeExecutionResult {
    /// Outcome of the code execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,

    /// Output from the code execution (stdout/stderr).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// Video metadata for video parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    /// Start offset (duration string like "1.5s").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<String>,

    /// End offset (duration string like "10.5s").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_offset: Option<String>,
}

/// A datatype containing media content.
///
/// Exactly one field should be set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    /// Text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Inline bytes data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<Blob>,

    /// URI based data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_data: Option<FileData>,

    /// A predicted function call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCall>,

    /// The result of a function call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response: Option<FunctionResponse>,

    /// Indicates if the part is thought/reasoning from the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<bool>,

    /// Opaque signature for reusing thought in subsequent requests.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_bytes_base64",
        deserialize_with = "deserialize_bytes_base64"
    )]
    pub thought_signature: Option<Vec<u8>>,

    /// Code authored and executed by the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_code: Option<ExecutableCode>,

    /// Result of code execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_execution_result: Option<CodeExecutionResult>,

    /// Video metadata for video parts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_metadata: Option<VideoMetadata>,

    /// Media resolution for the input media.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_resolution: Option<PartMediaResolution>,
}

impl Part {
    /// Create a text part.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Default::default()
        }
    }

    /// Create an inline data part from bytes.
    pub fn from_bytes(data: &[u8], mime_type: impl Into<String>) -> Self {
        Self {
            inline_data: Some(Blob::from_bytes(data, mime_type)),
            ..Default::default()
        }
    }

    /// Create a file data part from URI.
    pub fn from_uri(file_uri: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            file_data: Some(FileData::new(file_uri, mime_type)),
            ..Default::default()
        }
    }

    /// Create a function call part.
    pub fn function_call(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self {
            function_call: Some(FunctionCall::new(name, args)),
            ..Default::default()
        }
    }

    /// Create a function response part.
    pub fn function_response(name: impl Into<String>, response: serde_json::Value) -> Self {
        Self {
            function_response: Some(FunctionResponse::new(name, response)),
            ..Default::default()
        }
    }

    /// Create a thought part with a signature (for passing thoughts to subsequent requests).
    pub fn with_thought_signature(signature: impl Into<Vec<u8>>) -> Self {
        Self {
            thought: Some(true),
            thought_signature: Some(signature.into()),
            ..Default::default()
        }
    }

    /// Create a thought part with a base64-encoded signature string.
    pub fn with_thought_signature_base64(signature: &str) -> Result<Self, base64::DecodeError> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(signature)?;
        Ok(Self {
            thought: Some(true),
            thought_signature: Some(bytes),
            ..Default::default()
        })
    }

    /// Check if this part is a thought/reasoning part.
    pub fn is_thought(&self) -> bool {
        self.thought == Some(true)
    }
}

impl From<&str> for Part {
    fn from(text: &str) -> Self {
        Part::text(text)
    }
}

impl From<String> for Part {
    fn from(text: String) -> Self {
        Part::text(text)
    }
}

/// Contains the multi-part content of a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    /// List of parts that constitute a single message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<Part>>,

    /// The producer of the content. Must be either 'user' or 'model'.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl Content {
    /// Create a user content with text.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            parts: Some(vec![Part::text(text)]),
            role: Some("user".to_string()),
        }
    }

    /// Create a model content with text.
    pub fn model(text: impl Into<String>) -> Self {
        Self {
            parts: Some(vec![Part::text(text)]),
            role: Some("model".to_string()),
        }
    }

    /// Create content with multiple parts.
    pub fn with_parts(role: impl Into<String>, parts: Vec<Part>) -> Self {
        Self {
            parts: Some(parts),
            role: Some(role.into()),
        }
    }

    /// Create user content with image (bytes).
    pub fn user_with_image(
        text: impl Into<String>,
        image_data: &[u8],
        mime_type: impl Into<String>,
    ) -> Self {
        Self {
            parts: Some(vec![
                Part::text(text),
                Part::from_bytes(image_data, mime_type),
            ]),
            role: Some("user".to_string()),
        }
    }

    /// Create user content with image URI.
    pub fn user_with_image_uri(
        text: impl Into<String>,
        file_uri: impl Into<String>,
        mime_type: impl Into<String>,
    ) -> Self {
        Self {
            parts: Some(vec![Part::text(text), Part::from_uri(file_uri, mime_type)]),
            role: Some("user".to_string()),
        }
    }
}

// ============================================================================
// Tools
// ============================================================================

/// JSON Schema definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Schema {
    /// The type of the data.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub schema_type: Option<SchemaType>,

    /// Description of the schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Enum values for string types.
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,

    /// Properties for object types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, Schema>>,

    /// Required property names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,

    /// Items schema for array types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<Schema>>,

    /// Default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,

    /// Format hint (e.g., "int32", "int64", "float", "email").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    /// Minimum string length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_length: Option<i32>,

    /// Maximum string length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_length: Option<i32>,

    /// Minimum number value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,

    /// Maximum number value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,

    /// Minimum array items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_items: Option<i32>,

    /// Maximum array items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_items: Option<i32>,

    /// Regex pattern for string validation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,

    /// Whether the value can be null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nullable: Option<bool>,

    /// Union types (any of these schemas).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub any_of: Option<Vec<Schema>>,

    /// Schema definitions for $ref.
    #[serde(rename = "$defs", skip_serializing_if = "Option::is_none")]
    pub defs: Option<HashMap<String, Schema>>,

    /// Reference to a schema definition.
    #[serde(rename = "$ref", skip_serializing_if = "Option::is_none")]
    pub schema_ref: Option<String>,

    /// Whether additional properties are allowed (bool or schema).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_properties: Option<serde_json::Value>,

    /// Preferred order of properties in the output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property_ordering: Option<Vec<String>>,
}

impl Schema {
    pub fn string() -> Self {
        Self {
            schema_type: Some(SchemaType::String),
            ..Default::default()
        }
    }

    pub fn number() -> Self {
        Self {
            schema_type: Some(SchemaType::Number),
            ..Default::default()
        }
    }

    pub fn integer() -> Self {
        Self {
            schema_type: Some(SchemaType::Integer),
            ..Default::default()
        }
    }

    pub fn boolean() -> Self {
        Self {
            schema_type: Some(SchemaType::Boolean),
            ..Default::default()
        }
    }

    pub fn array(items: Schema) -> Self {
        Self {
            schema_type: Some(SchemaType::Array),
            items: Some(Box::new(items)),
            ..Default::default()
        }
    }

    pub fn object(properties: HashMap<String, Schema>) -> Self {
        Self {
            schema_type: Some(SchemaType::Object),
            properties: Some(properties),
            ..Default::default()
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_required(mut self, required: Vec<String>) -> Self {
        self.required = Some(required);
        self
    }
}

/// Defines a function that the model can generate JSON inputs for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDeclaration {
    /// The name of the function to call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Description and purpose of the function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The parameters to this function in OpenAPI Schema format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Schema>,

    /// Alternative: parameters in JSON Schema format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters_json_schema: Option<serde_json::Value>,

    /// Return type schema in OpenAPI Schema format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Schema>,

    /// Alternative: return type in JSON Schema format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_json_schema: Option<serde_json::Value>,

    /// Function behavior: BLOCKING (default) or NON_BLOCKING.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior: Option<Behavior>,
}

impl FunctionDeclaration {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Default::default()
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_parameters(mut self, parameters: Schema) -> Self {
        self.parameters = Some(parameters);
        self
    }
}

/// GoogleSearch tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoogleSearch {}

/// Code execution tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodeExecution {}

/// RAG filter for retrieval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RagFilter {
    /// Metadata filter string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_filter: Option<String>,
}

/// RAG retrieval configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RagRetrievalConfig {
    /// Number of top results to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,

    /// Filter for retrieval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<RagFilter>,
}

/// Vertex RAG store configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VertexRagStore {
    /// RAG corpora resource names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_corpora: Option<Vec<String>>,

    /// RAG retrieval configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_retrieval_config: Option<RagRetrievalConfig>,
}

/// Vertex AI Search configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VertexAISearch {
    /// Datastore resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datastore: Option<String>,
}

/// Retrieval tool configuration (Vertex AI only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Retrieval {
    /// Vertex AI Search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertex_ai_search: Option<VertexAISearch>,

    /// Vertex RAG store.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertex_rag_store: Option<VertexRagStore>,
}

/// Tool details that the model may use to generate a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// List of function declarations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_declarations: Option<Vec<FunctionDeclaration>>,

    /// Google Search tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_search: Option<GoogleSearch>,

    /// Code execution tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_execution: Option<CodeExecution>,

    /// Retrieval tool (Vertex AI only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieval: Option<Retrieval>,
}

impl Tool {
    /// Create a tool with function declarations.
    pub fn functions(declarations: Vec<FunctionDeclaration>) -> Self {
        Self {
            function_declarations: Some(declarations),
            ..Default::default()
        }
    }

    /// Create a Google Search tool.
    pub fn google_search() -> Self {
        Self {
            google_search: Some(GoogleSearch {}),
            ..Default::default()
        }
    }

    /// Create a code execution tool.
    pub fn code_execution() -> Self {
        Self {
            code_execution: Some(CodeExecution {}),
            ..Default::default()
        }
    }
}

/// Function calling configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCallingConfig {
    /// Function calling mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<FunctionCallingMode>,

    /// Function names to call (only when mode is ANY or VALIDATED).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,

    /// When true, function call arguments are streamed out in partial_args.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_function_call_arguments: Option<bool>,
}

/// Tool configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfig {
    /// Function calling config.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_calling_config: Option<FunctionCallingConfig>,
}

// ============================================================================
// Safety
// ============================================================================

/// Safety setting for a harm category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetySetting {
    /// The harm category.
    pub category: HarmCategory,

    /// The harm block threshold.
    pub threshold: HarmBlockThreshold,
}

/// Safety rating for generated content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyRating {
    /// The harm category.
    pub category: HarmCategory,

    /// The harm probability.
    pub probability: HarmProbability,

    /// Whether the content was blocked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
}

// ============================================================================
// Generation Config
// ============================================================================

/// Thinking configuration for models that support it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingConfig {
    /// Whether to include thoughts in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,

    /// Budget of thinking tokens. 0=DISABLED, -1=AUTO.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i32>,

    /// The level of thoughts tokens that the model should generate (LOW/HIGH).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

impl ThinkingConfig {
    /// Create a config that includes thoughts in the response.
    pub fn with_thoughts() -> Self {
        Self {
            include_thoughts: Some(true),
            thinking_budget: None,
            thinking_level: None,
        }
    }

    /// Create a config with a thinking budget.
    pub fn with_budget(budget: i32) -> Self {
        Self {
            include_thoughts: Some(true),
            thinking_budget: Some(budget),
            thinking_level: None,
        }
    }

    /// Create a config with a thinking level (LOW/HIGH).
    pub fn with_level(level: ThinkingLevel) -> Self {
        Self {
            include_thoughts: Some(true),
            thinking_budget: None,
            thinking_level: Some(level),
        }
    }
}

/// Generation configuration parameters (wire format inside generationConfig).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    /// Temperature for randomness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Top-p for nucleus sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Top-k for sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,

    /// Maximum output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,

    /// Number of candidates to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<i32>,

    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Whether to return log probabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_logprobs: Option<bool>,

    /// Number of top log probabilities to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<i32>,

    /// Response MIME type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,

    /// Response schema for structured output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<Schema>,

    /// Presence penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,

    /// Frequency penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,

    /// Seed for reproducibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i32>,

    /// Response modalities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<String>>,

    /// Thinking configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<ThinkingConfig>,
}

// ============================================================================
// Request / Response
// ============================================================================

/// Configuration for generate content request (user-facing API).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentConfig {
    /// System instruction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,

    /// Temperature for randomness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Top-p for nucleus sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Top-k for sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,

    /// Maximum output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,

    /// Number of candidates to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<i32>,

    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Whether to return log probabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_logprobs: Option<bool>,

    /// Number of top log probabilities to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<i32>,

    /// Response MIME type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,

    /// Response schema for structured output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<Schema>,

    /// Presence penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,

    /// Frequency penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,

    /// Seed for reproducibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i32>,

    /// Response modalities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<String>>,

    /// Safety settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,

    /// Tools available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// Tool configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfig>,

    /// Thinking configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<ThinkingConfig>,

    /// Cached content resource name for context caching.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_content: Option<String>,

    /// Alternative: response schema in raw JSON Schema format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_json_schema: Option<serde_json::Value>,

    /// Request extensions (headers, params, body) - not serialized.
    #[serde(skip)]
    pub extensions: Option<RequestExtensions>,

    /// Extra parameters passed through to the API request body.
    #[serde(flatten, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

impl GenerateContentConfig {
    /// Check if any generation parameters are set.
    pub fn has_generation_params(&self) -> bool {
        self.temperature.is_some()
            || self.top_p.is_some()
            || self.top_k.is_some()
            || self.max_output_tokens.is_some()
            || self.candidate_count.is_some()
            || self.stop_sequences.is_some()
            || self.response_logprobs.is_some()
            || self.logprobs.is_some()
            || self.response_mime_type.is_some()
            || self.response_schema.is_some()
            || self.presence_penalty.is_some()
            || self.frequency_penalty.is_some()
            || self.seed.is_some()
            || self.response_modalities.is_some()
            || self.thinking_config.is_some()
    }
}

/// Request body for generateContent API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentRequest {
    /// The content of the conversation.
    pub contents: Vec<Content>,

    /// System instruction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,

    /// Generation configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,

    /// Safety settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,

    /// Tools available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// Tool configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfig>,
}

/// Prompt feedback in response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PromptFeedback {
    /// The reason why the prompt was blocked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<BlockedReason>,

    /// Safety ratings for the prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<SafetyRating>>,
}

// ============================================================================
// Citation & Grounding Types
// ============================================================================

/// A citation source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CitationSource {
    /// URI of the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Title of the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// License information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// Start index in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_index: Option<i32>,

    /// End index in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_index: Option<i32>,
}

/// Citation metadata for a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CitationMetadata {
    /// List of citations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<Vec<CitationSource>>,
}

/// Web source for grounding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroundingChunkWeb {
    /// URI of the web source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Title of the web source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Retrieved context for grounding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroundingChunkRetrievedContext {
    /// URI of the retrieved context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Title of the retrieved context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// A grounding chunk (web or retrieved context).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroundingChunk {
    /// Web source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web: Option<GroundingChunkWeb>,

    /// Retrieved context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieved_context: Option<GroundingChunkRetrievedContext>,
}

/// A segment of text in the response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// Start index of the segment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_index: Option<i32>,

    /// End index of the segment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_index: Option<i32>,

    /// Text of the segment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Part index in the content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_index: Option<i32>,
}

/// Grounding support for a segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroundingSupport {
    /// The segment being grounded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment: Option<Segment>,

    /// Indices of grounding chunks that support this segment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_chunk_indices: Option<Vec<i32>>,

    /// Confidence scores for each grounding chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_scores: Option<Vec<f64>>,
}

/// Search entry point for grounding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchEntryPoint {
    /// Rendered HTML content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_content: Option<String>,

    /// SDK blob for the search entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk_blob: Option<String>,
}

/// Retrieval metadata for grounding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalMetadata {
    /// Dynamic retrieval score from Google Search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_search_dynamic_retrieval_score: Option<f64>,
}

/// Grounding metadata for a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroundingMetadata {
    /// Grounding chunks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_chunks: Option<Vec<GroundingChunk>>,

    /// Grounding supports linking segments to chunks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_supports: Option<Vec<GroundingSupport>>,

    /// Web search queries used for grounding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search_queries: Option<Vec<String>>,

    /// Search entry point.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_entry_point: Option<SearchEntryPoint>,

    /// Retrieval metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieval_metadata: Option<RetrievalMetadata>,
}

// ============================================================================
// Logprobs Types
// ============================================================================

/// A candidate token with its log probability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LogprobsCandidate {
    /// The token string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Token ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_id: Option<i32>,

    /// Log probability of the token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_probability: Option<f64>,
}

/// Top candidate tokens at a position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TopCandidates {
    /// List of top candidate tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<LogprobsCandidate>>,
}

/// Log probabilities result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LogprobsResult {
    /// Top candidates at each position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_candidates: Option<Vec<TopCandidates>>,

    /// Chosen candidates at each position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chosen_candidates: Option<Vec<LogprobsCandidate>>,
}

// ============================================================================
// Token Counting Types
// ============================================================================

/// Token count for a specific modality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModalityTokenCount {
    /// The modality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modality: Option<MediaModality>,

    /// Token count for this modality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<i32>,
}

/// Usage metadata in response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    /// Number of tokens in the prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_token_count: Option<i32>,

    /// Number of tokens in the candidates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_token_count: Option<i32>,

    /// Total token count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_token_count: Option<i32>,

    /// Cached content token count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_content_token_count: Option<i32>,

    /// Thoughts token count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thoughts_token_count: Option<i32>,

    /// Token count from tool use prompts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_prompt_token_count: Option<i32>,

    /// Token breakdown by modality for the prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<Vec<ModalityTokenCount>>,

    /// Token breakdown by modality for cached content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_tokens_details: Option<Vec<ModalityTokenCount>>,

    /// Token breakdown by modality for candidates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_tokens_details: Option<Vec<ModalityTokenCount>>,
}

/// A response candidate generated from the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// The generated content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,

    /// The reason why the model stopped generating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,

    /// Human-readable description of the finish reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_message: Option<String>,

    /// Safety ratings for the candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<SafetyRating>>,

    /// Index of the candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i32>,

    /// Token count for this candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<i32>,

    /// Citation metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citation_metadata: Option<CitationMetadata>,

    /// Average log probability across all tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_logprobs: Option<f64>,

    /// Grounding metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_metadata: Option<GroundingMetadata>,

    /// Log probabilities result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs_result: Option<LogprobsResult>,
}

impl Candidate {
    /// Format a debug summary of this candidate for logging.
    ///
    /// Returns a string with: index, finish_reason, part types, and text preview.
    pub fn debug_summary(&self) -> String {
        let index = self.index.map_or("?".to_string(), |i| i.to_string());
        let finish = self
            .finish_reason
            .map_or("none".to_string(), |r| format!("{r:?}"));

        let parts_info = self
            .content
            .as_ref()
            .and_then(|c| c.parts.as_ref())
            .map(|parts| {
                let summaries: Vec<String> = parts.iter().map(Self::part_summary).collect();
                summaries.join(", ")
            })
            .unwrap_or_else(|| "no parts".to_string());

        format!("[candidate {index}] finish={finish}, parts=[{parts_info}]")
    }

    /// Format a single part for logging.
    fn part_summary(part: &Part) -> String {
        if let Some(text) = &part.text {
            let preview = if text.len() > 100 {
                format!("{}...", &text[..100])
            } else {
                text.clone()
            };
            let thought_marker = if part.thought == Some(true) {
                "(thought) "
            } else {
                ""
            };
            format!("{thought_marker}text({} chars): {:?}", text.len(), preview)
        } else if let Some(fc) = &part.function_call {
            format!(
                "function_call({:?}, id={:?})",
                fc.name.as_deref().unwrap_or("?"),
                fc.id.as_deref().unwrap_or("?")
            )
        } else if let Some(fr) = &part.function_response {
            format!(
                "function_response({:?}, id={:?})",
                fr.name.as_deref().unwrap_or("?"),
                fr.id.as_deref().unwrap_or("?")
            )
        } else if part.inline_data.is_some() {
            "inline_data".to_string()
        } else if part.file_data.is_some() {
            "file_data".to_string()
        } else if part.executable_code.is_some() {
            "executable_code".to_string()
        } else if part.code_execution_result.is_some() {
            "code_execution_result".to_string()
        } else {
            "unknown".to_string()
        }
    }
}

/// Response from generateContent API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentResponse {
    /// Response candidates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<Candidate>>,

    /// Prompt feedback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_feedback: Option<PromptFeedback>,

    /// Usage metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,

    /// Model version used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,

    /// Unique response identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,

    /// Timestamp when the response was created (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,

    /// HTTP response metadata (not serialized, populated by client).
    /// Used to retain the full HTTP response for debugging/inspection.
    #[serde(skip)]
    pub sdk_http_response: Option<SdkHttpResponse>,
}

impl GenerateContentResponse {
    /// Get the text from the first candidate.
    pub fn text(&self) -> Option<String> {
        self.candidates
            .as_ref()?
            .first()?
            .content
            .as_ref()?
            .parts
            .as_ref()?
            .iter()
            .filter_map(|p| {
                // Skip thought parts
                if p.thought == Some(true) {
                    return None;
                }
                p.text.clone()
            })
            .reduce(|acc, s| acc + &s)
    }

    /// Get function calls from the first candidate.
    pub fn function_calls(&self) -> Option<Vec<&FunctionCall>> {
        let parts = self
            .candidates
            .as_ref()?
            .first()?
            .content
            .as_ref()?
            .parts
            .as_ref()?;

        let calls: Vec<_> = parts
            .iter()
            .filter_map(|p| p.function_call.as_ref())
            .collect();

        if calls.is_empty() { None } else { Some(calls) }
    }

    /// Get the finish reason from the first candidate.
    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.candidates.as_ref()?.first()?.finish_reason
    }

    /// Get the parts from the first candidate.
    ///
    /// Warns if there are multiple candidates, as only the first is returned
    /// and content from other candidates would be lost.
    pub fn parts(&self) -> Option<&Vec<Part>> {
        let candidates = self.candidates.as_ref()?;
        if candidates.len() > 1 {
            let all_summaries: Vec<String> = candidates.iter().map(|c| c.debug_summary()).collect();
            warn!(
                candidate_count = candidates.len(),
                candidates_detail = %all_summaries.join("\n  "),
                "Response has multiple candidates, only returning parts from the first. \
                 Content from {} additional candidate(s) will be ignored.",
                candidates.len() - 1
            );
        }
        candidates.first()?.content.as_ref()?.parts.as_ref()
    }

    /// Get thought/reasoning text from the first candidate (for debugging).
    pub fn thought_text(&self) -> Option<String> {
        self.candidates
            .as_ref()?
            .first()?
            .content
            .as_ref()?
            .parts
            .as_ref()?
            .iter()
            .filter_map(|p| {
                if p.thought == Some(true) {
                    p.text.clone()
                } else {
                    None
                }
            })
            .reduce(|acc, s| acc + &s)
    }

    /// Get thought signatures from the response for use in subsequent requests.
    /// These can be passed back to the model to continue a thought chain.
    /// Returns raw bytes (base64 decoded) for each thought signature.
    pub fn thought_signatures(&self) -> Vec<Vec<u8>> {
        self.candidates
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|c| c.content.as_ref())
            .and_then(|c| c.parts.as_ref())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.thought_signature.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if the response contains thought/reasoning content.
    pub fn has_thoughts(&self) -> bool {
        self.candidates
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|c| c.content.as_ref())
            .and_then(|c| c.parts.as_ref())
            .map(|parts| parts.iter().any(|p| p.thought == Some(true)))
            .unwrap_or(false)
    }
}

// ============================================================================
// SDK HTTP Response (runtime metadata)
// ============================================================================

/// HTTP response metadata for debugging and inspection.
///
/// This struct captures the raw HTTP response information that is not part of
/// the API response body. It's populated by the client after receiving a response.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SdkHttpResponse {
    /// HTTP status code.
    pub status_code: Option<i32>,

    /// Response headers.
    pub headers: Option<HashMap<String, String>>,

    /// Raw response body (for debugging).
    pub body: Option<String>,
}

impl SdkHttpResponse {
    /// Create a new SdkHttpResponse with all fields.
    pub fn new(status_code: i32, headers: HashMap<String, String>, body: String) -> Self {
        Self {
            status_code: Some(status_code),
            headers: Some(headers),
            body: Some(body),
        }
    }

    /// Create from status code and body only.
    pub fn from_status_and_body(status_code: i32, body: String) -> Self {
        Self {
            status_code: Some(status_code),
            headers: None,
            body: Some(body),
        }
    }
}

// ============================================================================
// Error Response
// ============================================================================

/// Error details from the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorDetail {
    #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

/// Error from the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: i32,
    pub message: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<ApiErrorDetail>>,
}

/// Error response wrapper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: ApiError,
}

// ============================================================================
// Request Extensions
// ============================================================================

/// Extension configuration for API requests.
///
/// Allows adding extra headers, query parameters, and body fields to requests.
/// Supports two-level configuration: client-level (default) and request-level.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RequestExtensions {
    /// Additional HTTP headers.
    pub headers: Option<HashMap<String, String>>,
    /// Additional URL query parameters.
    pub params: Option<HashMap<String, String>>,
    /// Additional body fields (shallow-merged into request JSON root).
    pub body: Option<serde_json::Value>,
}

impl RequestExtensions {
    /// Create a new empty RequestExtensions.
    pub fn new() -> Self {
        Default::default()
    }

    /// Add an HTTP header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value.into());
        self
    }

    /// Add a URL query parameter.
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value.into());
        self
    }

    /// Add a body field.
    pub fn with_body_field(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        let body = self.body.get_or_insert_with(|| serde_json::json!({}));
        if let Some(obj) = body.as_object_mut() {
            obj.insert(key.into(), value);
        }
        self
    }

    /// Set the entire body (replaces any existing body).
    pub fn with_body(mut self, body: serde_json::Value) -> Self {
        self.body = Some(body);
        self
    }

    /// Shallow merge: other's fields override self's fields.
    pub fn merge(&self, other: &RequestExtensions) -> RequestExtensions {
        RequestExtensions {
            headers: merge_hashmaps(&self.headers, &other.headers),
            params: merge_hashmaps(&self.params, &other.params),
            body: merge_json_objects(&self.body, &other.body),
        }
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.headers.as_ref().is_none_or(|h| h.is_empty())
            && self.params.as_ref().is_none_or(|p| p.is_empty())
            && self.body.is_none()
    }
}

fn merge_hashmaps(
    base: &Option<HashMap<String, String>>,
    other: &Option<HashMap<String, String>>,
) -> Option<HashMap<String, String>> {
    match (base, other) {
        (None, None) => None,
        (Some(b), None) => Some(b.clone()),
        (None, Some(o)) => Some(o.clone()),
        (Some(b), Some(o)) => {
            let mut merged = b.clone();
            merged.extend(o.iter().map(|(k, v)| (k.clone(), v.clone())));
            Some(merged)
        }
    }
}

fn merge_json_objects(
    base: &Option<serde_json::Value>,
    other: &Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    match (base, other) {
        (None, None) => None,
        (Some(b), None) => Some(b.clone()),
        (None, Some(o)) => Some(o.clone()),
        (Some(b), Some(o)) => {
            let mut merged = b.clone();
            if let (Some(base_obj), Some(other_obj)) = (merged.as_object_mut(), o.as_object()) {
                for (k, v) in other_obj {
                    base_obj.insert(k.clone(), v.clone());
                }
            }
            Some(merged)
        }
    }
}

#[cfg(test)]
#[path = "types.test.rs"]
mod tests;
