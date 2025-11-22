//! Core types for Google Generative AI (Gemini) API.
//!
//! This module contains all the data structures used for request/response
//! communication with the Gemini API.

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

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
}

impl FunctionCall {
    pub fn new(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self {
            id: None,
            name: Some(name.into()),
            args: Some(args),
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
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
}

impl FunctionResponse {
    pub fn new(name: impl Into<String>, response: serde_json::Value) -> Self {
        Self {
            id: None,
            name: Some(name.into()),
            response: Some(response),
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
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
    pub fn with_thought_signature(signature: impl Into<String>) -> Self {
        Self {
            thought: Some(true),
            thought_signature: Some(signature.into()),
            ..Default::default()
        }
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

    /// The parameters to this function in JSON Schema format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Schema>,
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

    /// Function names to call (only when mode is ANY).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_function_names: Option<Vec<String>>,
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

    /// Budget of thinking tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i32>,
}

impl ThinkingConfig {
    /// Create a config that includes thoughts in the response.
    pub fn with_thoughts() -> Self {
        Self {
            include_thoughts: Some(true),
            thinking_budget: None,
        }
    }

    /// Create a config with a thinking budget.
    pub fn with_budget(budget: i32) -> Self {
        Self {
            include_thoughts: Some(true),
            thinking_budget: Some(budget),
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

    /// Request extensions (headers, params, body) - not serialized.
    #[serde(skip)]
    pub extensions: Option<RequestExtensions>,
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

    /// Safety ratings for the candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<SafetyRating>>,

    /// Index of the candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i32>,

    /// Token count for this candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<i32>,
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
    pub fn parts(&self) -> Option<&Vec<Part>> {
        self.candidates
            .as_ref()?
            .first()?
            .content
            .as_ref()?
            .parts
            .as_ref()
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
    pub fn thought_signatures(&self) -> Vec<String> {
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
        self.headers.as_ref().map_or(true, |h| h.is_empty())
            && self.params.as_ref().map_or(true, |p| p.is_empty())
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
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization_structure() {
        // Test that the request is serialized with correct field names
        let request = GenerateContentRequest {
            contents: vec![Content::user("Hello")],
            system_instruction: Some(Content {
                parts: Some(vec![Part::text("You are helpful")]),
                role: Some("user".to_string()),
            }),
            generation_config: Some(GenerationConfig {
                temperature: Some(0.7),
                max_output_tokens: Some(1024),
                ..Default::default()
            }),
            safety_settings: None,
            tools: Some(vec![Tool::functions(vec![FunctionDeclaration::new(
                "test_func",
            )])]),
            tool_config: None,
        };

        let json = serde_json::to_value(&request).expect("serialization failed");

        // Verify top-level fields
        assert!(json.get("contents").is_some());
        assert!(json.get("systemInstruction").is_some());
        assert!(json.get("generationConfig").is_some());
        assert!(json.get("tools").is_some());

        // Verify generationConfig contains temperature (camelCase)
        let gen_config = json.get("generationConfig").unwrap();
        // Check temperature exists and is approximately 0.7 (f32 precision)
        let temp = gen_config.get("temperature").unwrap().as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.001);
        assert_eq!(
            gen_config.get("maxOutputTokens"),
            Some(&serde_json::json!(1024))
        );

        // Verify contents structure
        let contents = json.get("contents").unwrap().as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].get("role"), Some(&serde_json::json!("user")));
    }

    #[test]
    fn test_response_deserialization() {
        let json = r#"{
            "candidates": [{
                "content": {
                    "parts": [{"text": "Hello!"}],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 20,
                "totalTokenCount": 30
            }
        }"#;

        let response: GenerateContentResponse =
            serde_json::from_str(json).expect("deserialization failed");

        assert!(response.candidates.is_some());
        assert_eq!(response.text(), Some("Hello!".to_string()));
        assert_eq!(response.finish_reason(), Some(FinishReason::Stop));

        let usage = response.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, Some(10));
        assert_eq!(usage.candidates_token_count, Some(20));
        assert_eq!(usage.total_token_count, Some(30));
    }

    #[test]
    fn test_function_call_response_deserialization() {
        let json = r#"{
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "get_weather",
                            "args": {"location": "Tokyo"}
                        }
                    }],
                    "role": "model"
                },
                "finishReason": "STOP"
            }]
        }"#;

        let response: GenerateContentResponse =
            serde_json::from_str(json).expect("deserialization failed");

        let calls = response.function_calls().expect("no function calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, Some("get_weather".to_string()));
        assert_eq!(
            calls[0].args,
            Some(serde_json::json!({"location": "Tokyo"}))
        );
    }

    #[test]
    fn test_part_constructors() {
        // Text part
        let part = Part::text("hello");
        assert_eq!(part.text, Some("hello".to_string()));
        assert!(part.inline_data.is_none());

        // Image part from bytes
        let part = Part::from_bytes(&[1, 2, 3], "image/png");
        assert!(part.inline_data.is_some());
        let blob = part.inline_data.unwrap();
        assert_eq!(blob.mime_type, Some("image/png".to_string()));

        // Function call part
        let part = Part::function_call("test", serde_json::json!({"a": 1}));
        assert!(part.function_call.is_some());
        assert_eq!(part.function_call.unwrap().name, Some("test".to_string()));
    }

    #[test]
    fn test_content_constructors() {
        let user = Content::user("Hello");
        assert_eq!(user.role, Some("user".to_string()));

        let model = Content::model("Hi there");
        assert_eq!(model.role, Some("model".to_string()));
    }

    #[test]
    fn test_tool_constructors() {
        let tool = Tool::functions(vec![
            FunctionDeclaration::new("func1").with_description("A function"),
        ]);
        assert!(tool.function_declarations.is_some());
        assert!(tool.google_search.is_none());

        let search_tool = Tool::google_search();
        assert!(search_tool.google_search.is_some());
        assert!(search_tool.function_declarations.is_none());
    }

    #[test]
    fn test_generate_content_config_has_generation_params() {
        let empty = GenerateContentConfig::default();
        assert!(!empty.has_generation_params());

        let with_temp = GenerateContentConfig {
            temperature: Some(0.5),
            ..Default::default()
        };
        assert!(with_temp.has_generation_params());

        let with_system_only = GenerateContentConfig {
            system_instruction: Some(Content::user("system")),
            ..Default::default()
        };
        assert!(!with_system_only.has_generation_params());
    }

    #[test]
    fn test_request_extensions_builder() {
        let ext = RequestExtensions::new()
            .with_header("X-Custom", "value1")
            .with_param("key", "value2")
            .with_body_field("field", serde_json::json!("value3"));

        assert_eq!(
            ext.headers.as_ref().unwrap().get("X-Custom"),
            Some(&"value1".to_string())
        );
        assert_eq!(
            ext.params.as_ref().unwrap().get("key"),
            Some(&"value2".to_string())
        );
        assert_eq!(
            ext.body.as_ref().unwrap().get("field"),
            Some(&serde_json::json!("value3"))
        );
    }

    #[test]
    fn test_request_extensions_with_body() {
        let ext = RequestExtensions::new().with_body(serde_json::json!({"a": 1, "b": 2}));

        assert_eq!(ext.body, Some(serde_json::json!({"a": 1, "b": 2})));
    }

    #[test]
    fn test_request_extensions_merge_headers() {
        let base = RequestExtensions::new()
            .with_header("A", "1")
            .with_header("B", "2");

        let other = RequestExtensions::new()
            .with_header("B", "3") // Override
            .with_header("C", "4");

        let merged = base.merge(&other);

        let headers = merged.headers.unwrap();
        assert_eq!(headers.get("A"), Some(&"1".to_string()));
        assert_eq!(headers.get("B"), Some(&"3".to_string())); // Overridden
        assert_eq!(headers.get("C"), Some(&"4".to_string()));
    }

    #[test]
    fn test_request_extensions_merge_params() {
        let base = RequestExtensions::new().with_param("x", "1");
        let other = RequestExtensions::new()
            .with_param("x", "2") // Override
            .with_param("y", "3");

        let merged = base.merge(&other);

        let params = merged.params.unwrap();
        assert_eq!(params.get("x"), Some(&"2".to_string())); // Overridden
        assert_eq!(params.get("y"), Some(&"3".to_string()));
    }

    #[test]
    fn test_request_extensions_merge_body() {
        let base = RequestExtensions::new()
            .with_body_field("a", serde_json::json!(1))
            .with_body_field("b", serde_json::json!(2));

        let other = RequestExtensions::new()
            .with_body_field("b", serde_json::json!(3)) // Override
            .with_body_field("c", serde_json::json!(4));

        let merged = base.merge(&other);

        let body = merged.body.unwrap();
        assert_eq!(body.get("a"), Some(&serde_json::json!(1)));
        assert_eq!(body.get("b"), Some(&serde_json::json!(3))); // Overridden
        assert_eq!(body.get("c"), Some(&serde_json::json!(4)));
    }

    #[test]
    fn test_request_extensions_is_empty() {
        assert!(RequestExtensions::new().is_empty());

        assert!(!RequestExtensions::new().with_header("X", "Y").is_empty());
        assert!(!RequestExtensions::new().with_param("X", "Y").is_empty());
        assert!(
            !RequestExtensions::new()
                .with_body_field("X", serde_json::json!("Y"))
                .is_empty()
        );
    }

    #[test]
    fn test_request_extensions_merge_with_none() {
        let ext = RequestExtensions::new().with_header("A", "1");

        // Merge with empty
        let merged = ext.merge(&RequestExtensions::new());
        assert_eq!(
            merged.headers.as_ref().unwrap().get("A"),
            Some(&"1".to_string())
        );

        // Empty merge with non-empty
        let merged = RequestExtensions::new().merge(&ext);
        assert_eq!(
            merged.headers.as_ref().unwrap().get("A"),
            Some(&"1".to_string())
        );
    }
}
