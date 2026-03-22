//! cocode-api - Provider abstraction layer for the agent system.
//!
//! This crate wraps vercel-ai to provide:
//! - Unified streaming abstraction (stream vs non-stream)
//! - Retry logic with exponential backoff
//! - Prompt caching support
//! - Stall detection
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                         cocode-api                              │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  ApiClient         │  UnifiedStream      │  RetryContext       │
//! │  - retry           │  - Streaming mode   │  - backoff          │
//! │  - caching         │  - Non-stream mode  │                     │
//! │                    │  - Event emission   │                     │
//! ├────────────────────┴───────────────────────────────────────────┤
//! │                       vercel-ai SDK                             │
//! │  LanguageModel, Provider, StreamProcessor, ...                  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

pub mod client;
pub mod error;
pub mod message_normalize;
pub mod model_hub;
pub mod provider_factory;
pub mod request_builder;
pub mod request_options_merge;
pub mod retry;
pub mod schema_sanitize;
pub mod thinking_convert;
pub mod unified_stream;

// Re-export main types at crate root
pub use client::ApiClient;
pub use client::ApiClientConfig;
pub use client::StreamOptions;
pub use error::ApiError;
pub use error::Result;
pub use model_hub::ModelHub;
pub use model_hub::resolve_identity;
pub use provider_factory::create_model;
pub use provider_factory::create_provider;
pub use request_builder::RequestBuilder;
pub use request_builder::build_request;
pub use retry::RetryContext;
pub use retry::RetryDecision;
pub use thinking_convert::to_provider_options;

pub use unified_stream::CollectedResponse;
pub use unified_stream::QueryResultType;
pub use unified_stream::StreamingQueryResult;
pub use unified_stream::UnifiedStream;
pub use unified_stream::convert_generate_usage;

// Re-export vercel-ai types used by downstream crates.
// Version-free aliases: downstream crates import these names from cocode_api.
// When vercel-ai upgrades (e.g. V4→V5), only these aliases need updating.
pub use vercel_ai::stream::StreamProcessor;
pub use vercel_ai::stream::StreamSnapshot;
pub use vercel_ai_provider::AISdkError;
pub use vercel_ai_provider::AssistantContentPart;
pub use vercel_ai_provider::DataContent;
pub use vercel_ai_provider::FilePart;
pub use vercel_ai_provider::FinishReason;
pub use vercel_ai_provider::JSONValue;
pub use vercel_ai_provider::LanguageModelV4 as LanguageModel;
pub use vercel_ai_provider::LanguageModelV4CallOptions as LanguageModelCallOptions;
pub use vercel_ai_provider::LanguageModelV4GenerateResult as LanguageModelGenerateResult;
pub use vercel_ai_provider::LanguageModelV4Message as LanguageModelMessage;
pub use vercel_ai_provider::LanguageModelV4Prompt as LanguageModelPrompt;
pub use vercel_ai_provider::LanguageModelV4StreamPart as LanguageModelStreamPart;
pub use vercel_ai_provider::LanguageModelV4Tool as LanguageModelTool;
pub use vercel_ai_provider::LanguageModelV4ToolChoice as LanguageModelToolChoice;
pub use vercel_ai_provider::ProviderMetadata;
pub use vercel_ai_provider::ProviderOptions;
pub use vercel_ai_provider::ProviderV4 as Provider;
pub use vercel_ai_provider::ReasoningFilePart;
pub use vercel_ai_provider::ReasoningPart;
pub use vercel_ai_provider::ResponseFormat;
pub use vercel_ai_provider::TextPart;
pub use vercel_ai_provider::ToolCallPart;
pub use vercel_ai_provider::ToolContentPart;
pub use vercel_ai_provider::ToolDefinitionV4 as LanguageModelFunctionTool;
pub use vercel_ai_provider::ToolResultContent;
pub use vercel_ai_provider::ToolResultPart;
pub use vercel_ai_provider::UnifiedFinishReason;
pub use vercel_ai_provider::Usage;
pub use vercel_ai_provider::UserContentPart;
pub use vercel_ai_provider::tool::ToolCall;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::AssistantContentPart;
    pub use crate::FinishReason;
    pub use crate::LanguageModel;
    pub use crate::LanguageModelCallOptions;
    pub use crate::LanguageModelGenerateResult;
    pub use crate::LanguageModelMessage;
    pub use crate::LanguageModelStreamPart;
    pub use crate::LanguageModelTool;
    pub use crate::LanguageModelToolChoice;
    pub use crate::ToolCall;
    pub use crate::client::ApiClient;
    pub use crate::client::StreamOptions;
    pub use crate::error::ApiError;
    pub use crate::error::Result;
    pub use crate::retry::RetryContext;
    pub use crate::unified_stream::StreamingQueryResult;
    pub use crate::unified_stream::UnifiedStream;
}
