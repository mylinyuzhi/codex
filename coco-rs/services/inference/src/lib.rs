//! LLM inference client via vercel-ai, retry engine, auth, rate limiting.
//!
//! ApiClient wraps any `Arc<dyn LanguageModelV4>` — real provider or mock.

pub mod auth;
pub mod cache_detection;
pub mod client;
pub mod errors;
pub mod logging;
pub mod lsp_integration;
pub mod options_merge;
pub mod retry;
pub mod stream;
pub mod thinking_convert;
pub mod tool_schemas;
pub mod usage;

pub use cache_detection::CacheBreakDetector;
pub use cache_detection::CacheBreakResult;
pub use cache_detection::CacheState;
pub use cache_detection::PromptStateInput;
pub use client::ApiClient;
pub use client::QueryParams;
pub use client::QueryResult;
pub use errors::InferenceError;
pub use logging::ErrorLog;
pub use logging::KnownGateway;
pub use logging::RequestLog;
pub use logging::ResponseLog;
pub use logging::StopReason;
pub use logging::detect_gateway;
pub use logging::format_request_log;
pub use logging::format_response_log;
pub use options_merge::merge_provider_options;
pub use options_merge::provider_base_options;
pub use retry::RetryConfig;
pub use stream::StreamEvent;
pub use stream::synthetic_stream_from_content;
pub use tool_schemas::GeneratedSchemas;
pub use tool_schemas::ToolSchemaOrigin;
pub use tool_schemas::ToolSchemaSource;
pub use tool_schemas::estimate_schema_tokens;
pub use tool_schemas::filter_schemas_by_model;
pub use tool_schemas::generate_tool_schemas;
pub use tool_schemas::merge_tool_schemas;
pub use usage::UsageAccumulator;
