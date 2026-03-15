//! vercel-ai - Vercel AI SDK core functions for Rust
//!
//! This crate provides high-level API functions for LLM interactions, matching
//! `@ai-sdk/ai` TypeScript package. It builds on top of `vercel-ai-provider`
//! types and `vercel-ai-provider-utils` utilities.
//!
//! # Core Functions
//!
//! - [`generate_text`] - Generate text from a prompt (non-streaming)
//! - [`stream_text`] - Stream text generation
//! - [`generate_object`] - Generate structured output matching a JSON schema
//! - [`stream_object`] - Stream structured output generation
//! - [`embed`] - Generate embeddings for text
//! - [`embed_many`] - Generate embeddings for multiple texts
//! - [`rerank`] - Rerank documents by relevance to a query
//! - [`generate_image`] - Generate images from prompts
//! - [`generate_speech`] - Generate speech audio from text
//! - [`transcribe`] - Transcribe audio to text
//!
//! # Warning Logging
//!
//! The crate provides a warning logging system for provider warnings:
//!
//! ```ignore
//! use vercel_ai::logger::{set_log_warnings, LogWarningsFunction};
//!
//! // Set a custom warning logger
//! set_log_warnings(Some(LogWarningsFunction::new(|options| {
//!     eprintln!("Provider {} / Model {}: {:?}",
//!         options.provider, options.model, options.warnings);
//! })));
//! ```
//!
//! # Global Provider Pattern
//!
//! The crate supports a global default provider that can be set once and used
//! for all model resolution:
//!
//! ```ignore
//! use vercel_ai::{set_default_provider, generate_text, GenerateTextOptions, Prompt};
//! use std::sync::Arc;
//!
//! // Set a default provider
//! set_default_provider(Arc::new(my_provider));
//!
//! // Now generate_text can use string model IDs
//! let result = generate_text(GenerateTextOptions {
//!     model: "claude-3-sonnet".into(),
//!     prompt: Prompt::user("Hello"),
//!     ..Default::default()
//! }).await?;
//! ```
//!
//! # Example
//!
//! ```ignore
//! use vercel_ai::{generate_text, GenerateTextOptions, Prompt, LanguageModel};
//!
//! async fn example() -> Result<(), vercel_ai::AIError> {
//!     let result = generate_text(GenerateTextOptions {
//!         model: LanguageModel::from_v4(my_model),
//!         prompt: Prompt::user("Tell me a joke"),
//!         ..Default::default()
//!     }).await?;
//!
//!     println!("Response: {}", result.text);
//!     Ok(())
//! }
//! ```

// Modules
pub mod embed;
pub mod error;
pub mod generate_image;
pub mod generate_object;
pub mod generate_speech;
pub mod generate_text;
pub mod generate_video;
pub mod logger;
pub mod middleware;
pub mod model;
pub mod prompt;
pub mod provider;
pub mod registry;
pub mod rerank;
pub mod telemetry;
pub mod transcribe;
pub mod types;
pub mod util;

// Re-exports from generate_text module
pub use generate_text::{
    FinishEventMetadata,
    GenerateTextCallbacks,
    GenerateTextOptions,
    GenerateTextResult,
    // Callback event types
    OnChunkEvent,
    OnFinishEvent,
    OnStartEvent,
    OnStepFinishEvent,
    OnStepStartEvent,
    OnToolCallFinishEvent,
    OnToolCallStartEvent,
    // Output types
    Output,
    OutputMode,
    OutputStrategy,
    // Prepare step types
    PrepareStepContext,
    PrepareStepFn,
    PrepareStepOverrides,
    // Reasoning output
    ReasoningOutput,
    // Result types
    ResponseMessageData,
    StepResult,
    StreamTextCallbacks,
    StreamTextOptions,
    StreamTextResult,
    TextStreamPart,
    ToolCall,
    // Tool error/output types
    ToolError,
    ToolOutput,
    ToolOutputContent,
    ToolResult,
    // Response message building
    build_assistant_message,
    build_assistant_message_from_text,
    build_single_tool_result_message,
    build_tool_result_message,
    // Shared content extraction utilities
    extract_reasoning,
    // Content extraction (original modules)
    extract_reasoning_content,
    extract_reasoning_outputs,
    extract_reasoning_text,
    extract_reasoning_with_stats,
    extract_text,
    extract_text_content,
    extract_text_content_with_metadata,
    extract_tool_calls,
    // Active tools filtering
    filter_active_tools,
    generate_text,
    has_reasoning_content,
    stream_text,
};

// Re-exports from generate_object module
pub use generate_object::GenerateObjectFinishEvent;
pub use generate_object::GenerateObjectOptions;
pub use generate_object::GenerateObjectResult;
pub use generate_object::ObjectGenerationMode;
pub use generate_object::ObjectStreamPart;
pub use generate_object::StreamObjectResult;
pub use generate_object::generate_object;
pub use generate_object::stream_object;

// Re-exports from embed module
pub use embed::EmbedManyOptions;
pub use embed::EmbedManyResult;
pub use embed::EmbedOptions;
pub use embed::EmbedResult;
pub use embed::embed;
pub use embed::embed_many;

// Re-exports from generate_speech module
pub use generate_speech::GenerateSpeechOptions;
pub use generate_speech::GeneratedAudioFile;
pub use generate_speech::SpeechResult;
pub use generate_speech::generate_speech;

// Re-exports from generate_image module
pub use generate_image::GenerateImageOptions;
pub use generate_image::GenerateImageResult;
pub use generate_image::GeneratedImage;
pub use generate_image::ImageModel;
pub use generate_image::ImagePrompt;
pub use generate_image::ImageQuality;
pub use generate_image::ImageSize;
pub use generate_image::ImageStyle;
pub use generate_image::generate_image;

// Re-exports from generate_video module
pub use generate_video::AspectRatio;
pub use generate_video::DownloadFn;
pub use generate_video::GenerateVideoOptions;
pub use generate_video::GenerateVideoResult;
pub use generate_video::GeneratedVideo;
pub use generate_video::Resolution;
pub use generate_video::VideoData;
pub use generate_video::VideoDuration;
pub use generate_video::VideoModel;
pub use generate_video::VideoPrompt;
pub use generate_video::VideoSize;
pub use generate_video::generate_video;

// Re-exports from transcribe module
pub use transcribe::AudioData;
pub use transcribe::TranscribeOptions;
pub use transcribe::TranscriptionFormat;
pub use transcribe::TranscriptionModel;
pub use transcribe::TranscriptionResult;
pub use transcribe::TranscriptionSegment;
pub use transcribe::transcribe;

// Re-exports from prompt module
pub use prompt::{
    CallSettings,
    DataContentValue,
    Prompt,
    PromptAssistantContent,
    PromptAssistantContentPart,
    PromptAssistantMessage,
    PromptContent,
    PromptContentItem,
    PromptFileData,
    PromptFilePart,
    PromptImageData,
    PromptImagePart,
    PromptMessage,
    PromptReasoningPart,
    PromptSystemMessage,
    PromptTextPart,
    PromptToolCallPart,
    PromptToolContentPart,
    PromptToolMessage,
    PromptToolResultOutput,
    PromptToolResultPart,
    PromptUserContent,
    PromptUserContentPart,
    PromptUserMessage,
    StandardizedPrompt,
    SystemPrompt,
    TimeoutConfiguration,
    combine_tool_messages,
    convert_to_language_model_data_content,
    convert_to_language_model_message,
    convert_to_language_model_prompt,
    convert_uint8_array_to_text,
    // Tool utilities
    determine_tool_choice,
    // Error handling
    get_user_friendly_message,
    is_gateway_error_retryable,
    is_tool_call_disabled,
    is_tool_call_required,
    // Call settings utilities
    prepare_call_settings,
    prepare_call_settings_with_defaults,
    prepare_tool_definitions,
    prepare_tools_and_tool_choice,
    standardize_messages_prompt,
    standardize_prompt,
    standardize_text_prompt,
    wrap_gateway_error,
    wrap_gateway_error_with_context,
};

// Re-exports from model module
pub use model::EmbeddingModel;
pub use model::ImageModelRef;
pub use model::LanguageModel;
pub use model::VideoModelRef;
pub use model::resolve_embedding_model;
pub use model::resolve_embedding_model_with_provider;
pub use model::resolve_image_model;
pub use model::resolve_image_model_with_provider;
pub use model::resolve_language_model;
pub use model::resolve_language_model_with_provider;
pub use model::resolve_video_model;
pub use model::resolve_video_model_with_provider;

// Re-exports from provider module
pub use provider::clear_default_provider;
pub use provider::get_default_provider;
pub use provider::has_default_provider;
pub use provider::set_default_provider;

// Re-exports from error module
pub use error::AIError;
pub use error::InvalidArgumentError;
pub use error::InvalidStreamPartError;
pub use error::InvalidToolApprovalError;
pub use error::InvalidToolInputError;
pub use error::MissingToolResultsError;
pub use error::NoOutputGeneratedError;
pub use error::NoSuchToolError;
pub use error::NoVideoGeneratedError;
pub use error::SchemaValidationError;
pub use error::ToolCallNotFoundForApprovalError;
pub use error::ToolCallRepairError;
pub use error::ToolCallRepairOriginalError;
pub use error::UnsupportedModelVersionError;
pub use error::VideoModelResponseMetadata;

// Re-exports from logger module
pub use logger::FIRST_WARNING_INFO_MESSAGE;
pub use logger::LogWarningsFunction;
pub use logger::LogWarningsOptions;
pub use logger::log_warnings;
pub use logger::reset_log_warnings_state;
pub use logger::set_log_warnings;

// Re-exports from rerank module
pub use rerank::RerankOptions;
pub use rerank::RerankResult;
pub use rerank::RerankedDocument;
pub use rerank::RerankingModel;
pub use rerank::rerank;

// Re-exports from telemetry module
pub use telemetry::TelemetrySettings;

// Re-exports from util module
pub use util::retry::RetryConfig;
pub use util::retry::RetryableError;
pub use util::retry::with_retry;
pub use util::{
    CancellationManager,
    RetrySettings,
    // Partial JSON parsing
    complete_partial_json,
    // Abort signals
    create_deadline_token,
    create_timeout_token,
    extract_partial_value,
    // Headers
    get_header,
    merge_abort_signals,
    merge_abort_signals_with_timeout,
    merge_headers,
    parse_partial_json,
    parse_partial_json_with_repair,
    prepare_headers,
    prepare_headers_with_auth,
    prepare_provider_headers,
    // Retries
    prepare_provider_retries,
    prepare_retries,
};

// Re-exports from registry module
pub use registry::CustomProviderOptions;
pub use registry::NoSuchProviderError;
pub use registry::ProviderRegistry;
pub use registry::ProviderRegistryOptions;
pub use registry::create_provider_registry;
pub use registry::custom_provider;

// Re-exports from middleware module
pub use middleware::DefaultEmbeddingSettings;
pub use middleware::DefaultSettings;
pub use middleware::EmbeddingMiddleware;
pub use middleware::ImageMiddleware;
pub use middleware::add_tool_input_examples_middleware;
pub use middleware::default_embedding_settings_middleware;
pub use middleware::default_settings_middleware;
pub use middleware::extract_json_middleware;
pub use middleware::extract_reasoning_middleware;
pub use middleware::simulate_streaming_middleware;
pub use middleware::wrap_embedding_model;
pub use middleware::wrap_image_model;
pub use middleware::wrap_language_model;
pub use middleware::wrap_provider;

// Re-export commonly used types from the provider crate
pub use types::{
    // Error types
    AISdkError,
    // Content types
    AssistantContentPart,
    DataContent,
    // Model traits
    EmbeddingModelV4,
    // Embedding types
    EmbeddingModelV4CallOptions,
    EmbeddingModelV4EmbedResult,
    EmbeddingType,
    EmbeddingUsage,
    EmbeddingValue,
    // Tool execution
    ExecutableTool,
    FilePart,
    // Finish reason
    FinishReason,
    // JSON types
    JSONSchema,
    JSONValue,
    // Metadata types
    LanguageModelRequestMetadata,
    LanguageModelResponseMetadata,
    LanguageModelV4,
    // Response types
    LanguageModelV4CallOptions,
    LanguageModelV4GenerateResult,
    LanguageModelV4StreamPart,
    LanguageModelV4StreamResult,
    // Tool types
    LanguageModelV4Tool,
    LanguageModelV4ToolChoice,
    // Message types
    ModelMessage,
    ModelPrompt,
    // Shared types
    ProviderMetadata,
    ProviderOptions,
    ProviderV4,
    ReasoningPart,
    ResponseFormat,
    SimpleTool,
    // Stream types
    Source,
    SourceType,
    StreamError,
    TextPart,
    ToolCallPart,
    ToolChoice,
    ToolContentPart,
    ToolDefinitionV4,
    ToolExecutionOptions,
    ToolInvocation,
    ToolRegistry,
    ToolResultContent,
    ToolResultContentPart,
    ToolResultPart,
    // Usage types
    Usage,
    UserContentPart,
    Warning,
};

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
