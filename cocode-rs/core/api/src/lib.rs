//! cocode-api - Provider abstraction layer for the agent system.
//!
//! This crate wraps hyper-sdk to provide:
//! - Unified streaming abstraction (stream vs non-stream)
//! - Retry logic with exponential backoff
//! - Model fallback on overload
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
//! │  - fallback        │  - Non-stream mode  │  - fallback logic   │
//! │  - caching         │  - Event emission   │                     │
//! ├────────────────────┴───────────────────────────────────────────┤
//! │                        hyper-sdk                                │
//! │  HyperClient, Message, StreamProcessor, GenerateRequest, ...   │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Quick Start
//!
//! ```ignore
//! use cocode_api::{ApiClient, StreamOptions};
//! use hyper_sdk::{HyperClient, GenerateRequest, Message};
//!
//! // Create the hyper-sdk client
//! let hyper_client = HyperClient::from_env()?;
//!
//! // Wrap with cocode-api client
//! let client = ApiClient::new(hyper_client);
//!
//! // Make a streaming request
//! let request = GenerateRequest::new(vec![Message::user("Hello!")])
//!     .with_model("claude-3-5-sonnet-20241022");
//!
//! let mut stream = client.stream_request(request, StreamOptions::streaming()).await?;
//!
//! // Process results
//! while let Some(result) = stream.next().await {
//!     let result = result?;
//!     if result.has_content() {
//!         // Handle completed content blocks
//!     }
//! }
//! ```
//!
//! # Module Structure
//!
//! - [`error`] - Error types with status codes
//! - [`aggregation`] - Stream event aggregation
//! - [`retry`] - Retry context with backoff
//! - [`unified_stream`] - Unified stream abstraction
//! - [`cache`] - Prompt caching helpers
//! - [`client`] - High-level API client

pub mod aggregation;
pub mod cache;
pub mod client;
pub mod error;
pub mod retry;
pub mod unified_stream;

// Re-export main types at crate root
pub use aggregation::{AggregationState, PartialBlock, StreamTelemetry};
pub use cache::{CacheStats, Cacheable, PromptCacheConfig};
pub use client::{ApiClient, ApiClientBuilder, ApiClientConfig, StreamOptions};
pub use error::{ApiError, Result};
pub use retry::{RetryConfig, RetryContext, RetryDecision};
pub use unified_stream::{CollectedResponse, QueryResultType, StreamingQueryResult, UnifiedStream};

// Re-export commonly used hyper-sdk types for convenience
pub use hyper_sdk::{
    ContentBlock, FinishReason, GenerateRequest, GenerateResponse, Message, Role, StreamEvent,
    StreamProcessor, StreamSnapshot, StreamUpdate, TokenUsage, ToolCall, ToolChoice,
    ToolDefinition, ToolResultContent,
};

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::aggregation::AggregationState;
    pub use crate::cache::PromptCacheConfig;
    pub use crate::client::{ApiClient, StreamOptions};
    pub use crate::error::{ApiError, Result};
    pub use crate::retry::{RetryConfig, RetryContext};
    pub use crate::unified_stream::{StreamingQueryResult, UnifiedStream};
    pub use crate::{
        ContentBlock, FinishReason, GenerateRequest, GenerateResponse, Message, Role, StreamEvent,
        ToolCall, ToolChoice, ToolDefinition,
    };
}
