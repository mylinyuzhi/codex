//! LLM inference client via vercel-ai, retry engine, auth, rate limiting.
//!
//! `ApiClient` wraps any `Arc<dyn LanguageModel>` — real provider or mock.
//!
//! This crate is the **single re-export seam** between coco-rs and the
//! `vercel-ai-provider` SDK. Upper layers must reach for AI SDK types via
//! `coco_inference::*` (or `coco_inference::prelude::*`), never via
//! `vercel_ai_provider::*` directly. When the SDK upgrades (V4 → V5),
//! only the version-stripped aliases below need re-pointing.

pub mod auth;
pub mod build_call_options;
pub mod cache_convert;
pub mod cache_detection;
pub mod client;
pub mod errors;
pub mod fingerprint;
pub mod logging;
pub mod lsp_integration;
pub mod model_factory;
pub mod prompt_layout;
pub mod retry;
pub mod stream;
pub mod thinking_convert;
pub mod tool_schemas;
pub mod usage;

pub use build_call_options::PerCallOverrides;
pub use build_call_options::build_call_options;
pub use build_call_options::build_call_options_with_extra;
pub use cache_detection::CacheBreakDetector;
pub use cache_detection::CacheBreakResult;
pub use cache_detection::CacheState;
pub use cache_detection::PromptStateInput;
pub use client::ApiClient;
pub use client::QueryParams;
pub use client::QueryResult;
pub use errors::InferenceError;
pub use fingerprint::ProviderClientFingerprint;
pub use logging::ErrorLog;
pub use logging::KnownGateway;
pub use logging::RequestLog;
pub use logging::ResponseLog;
pub use logging::StopReason;
pub use logging::detect_gateway;
pub use logging::format_request_log;
pub use logging::format_response_log;
pub use prompt_layout::AnthropicCacheControl;
pub use prompt_layout::AnthropicSystemBlock;
pub use prompt_layout::CacheHint;
pub use prompt_layout::PROMPT_LAYOUT_NAMESPACE;
pub use prompt_layout::PromptEnvelope;
pub use prompt_layout::PromptHashInputs;
pub use prompt_layout::PromptLayoutOptions;
pub use prompt_layout::PromptPart;
pub use prompt_layout::PromptSection;
pub use prompt_layout::PromptSectionKind;
pub use prompt_layout::PromptSource;
pub use prompt_layout::build_prompt_layout_from_prompt;
pub use prompt_layout::put_layout_options;
pub use prompt_layout::take_layout_options;
pub use retry::RetryConfig;
pub use stream::AssistantTurnSnapshot;
pub use stream::CustomSegment;
pub use stream::FileSegment;
pub use stream::ReasoningFileSegment;
pub use stream::ReasoningSegment;
pub use stream::SourceSegment;
pub use stream::StreamEvent;
pub use stream::StreamMetrics;
pub use stream::StreamProcessorConfig;
pub use stream::TextSegment;
pub use stream::ToolApprovalRequestSegment;
pub use stream::ToolCallSegment;
pub use stream::TurnPart;
pub use stream::default_process_stream_config;
pub use stream::synthetic_stream_from_content;
pub use thinking_convert::to_extra_body;
pub use tool_schemas::GeneratedSchemas;
pub use tool_schemas::ToolSchemaOrigin;
pub use tool_schemas::ToolSchemaSource;
pub use tool_schemas::estimate_schema_tokens;
pub use tool_schemas::filter_schemas_by_model;
pub use tool_schemas::generate_tool_schemas;
pub use tool_schemas::merge_tool_schemas;
pub use usage::UsageAccumulator;

// ─── Vercel-ai re-export hub ──────────────────────────────────────────────
//
// Version-free aliases: downstream crates import these names from
// `coco_inference`. When `vercel-ai-provider` upgrades (e.g. V4 → V5),
// only these aliases need updating — call sites stay byte-identical.
//
// Naming convention:
// - Types whose vercel-ai name carries a version digit (`LanguageModelV4`,
//   `ProviderV4`, `LanguageModelV4Tool`, …) are renamed to strip the digit.
// - Types whose vercel-ai name has no version digit (`AssistantContentPart`,
//   `Usage`, `FinishReason`, …) are passed through unchanged.
//
// CI grep guard (`scripts/check-vercel-ai-seam.sh`) ensures no other crate
// imports `vercel_ai_provider::*` directly.

// Language-model protocol family — version-stripped renames.
pub use vercel_ai_provider::LanguageModelV4 as LanguageModel;
pub use vercel_ai_provider::LanguageModelV4CallOptions as LanguageModelCallOptions;
pub use vercel_ai_provider::LanguageModelV4GenerateResult as LanguageModelGenerateResult;
pub use vercel_ai_provider::LanguageModelV4Message as LanguageModelMessage;
pub use vercel_ai_provider::LanguageModelV4Prompt as LanguageModelPrompt;
pub use vercel_ai_provider::LanguageModelV4StreamResult as LanguageModelStreamResult;
pub use vercel_ai_provider::LanguageModelV4Tool as LanguageModelTool;
pub use vercel_ai_provider::ProviderV4 as Provider;
pub use vercel_ai_provider::language_model::v4::LanguageModelV4FunctionTool as LanguageModelFunctionTool;

// Content parts — pass-through, no version digit.
pub use vercel_ai_provider::AssistantContentPart;
pub use vercel_ai_provider::CustomPart;
pub use vercel_ai_provider::DataContent;
pub use vercel_ai_provider::FilePart;
pub use vercel_ai_provider::FileRawData;
pub use vercel_ai_provider::ReasoningFilePart;
pub use vercel_ai_provider::ReasoningPart;
pub use vercel_ai_provider::SharedV4FileData;
pub use vercel_ai_provider::TextPart;
pub use vercel_ai_provider::ToolCallPart;
pub use vercel_ai_provider::ToolContentPart;
pub use vercel_ai_provider::ToolResultContent;
pub use vercel_ai_provider::ToolResultContentPart;
pub use vercel_ai_provider::ToolResultPart;
pub use vercel_ai_provider::UserContentPart;

// Errors / metadata / usage / config knobs — pass-through.
pub use vercel_ai_provider::AISdkError;
pub use vercel_ai_provider::FinishReason;
pub use vercel_ai_provider::JSONValue;
pub use vercel_ai_provider::ProviderMetadata;
pub use vercel_ai_provider::ProviderOptions;
pub use vercel_ai_provider::ReasoningLevel;
pub use vercel_ai_provider::ResponseFormat;
pub use vercel_ai_provider::ResponseMetadata;
pub use vercel_ai_provider::UnifiedFinishReason;
pub use vercel_ai_provider::Usage;

/// One-line `use coco_inference::prelude::*;` to bring the common subset
/// of API client + LLM types into scope. Mirrors `cocode-inference::prelude`.
pub mod prelude {
    pub use crate::ApiClient;
    pub use crate::AssistantContentPart;
    pub use crate::FinishReason;
    pub use crate::LanguageModel;
    pub use crate::LanguageModelCallOptions;
    pub use crate::LanguageModelGenerateResult;
    pub use crate::LanguageModelMessage;
    pub use crate::LanguageModelPrompt;
    pub use crate::LanguageModelStreamResult;
    pub use crate::LanguageModelTool;
    pub use crate::QueryParams;
    pub use crate::QueryResult;
    pub use crate::StreamEvent;
    pub use crate::StreamMetrics;
    pub use crate::StreamProcessorConfig;
    pub use crate::TextPart;
    pub use crate::ToolCallPart;
    pub use crate::Usage;
    pub use crate::UserContentPart;
}
