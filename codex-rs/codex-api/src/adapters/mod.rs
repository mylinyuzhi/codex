//! Multi-LLM adapter support for codex-api.
//!
//! This module provides a trait-based abstraction for supporting multiple LLM providers
//! beyond the default OpenAI/Anthropic APIs. Adapters convert between codex-api's
//! canonical types (Prompt, ResponseEvent) and provider-specific wire formats.

pub mod genai;

use crate::common::Prompt;
use crate::common::ResponseEvent;
use crate::error::ApiError;
use async_trait::async_trait;
use codex_protocol::protocol::TokenUsage;

/// Configuration for an adapter instance.
#[derive(Debug, Clone, Default)]
pub struct AdapterConfig {
    /// API key for authentication.
    pub api_key: Option<String>,
    /// Base URL override (if not using default).
    pub base_url: Option<String>,
    /// Model name to use.
    pub model: String,
    /// Additional provider-specific configuration as JSON.
    pub extra: Option<serde_json::Value>,
}

/// Result of a non-streaming generate call.
#[derive(Debug)]
pub struct GenerateResult {
    /// Response events (OutputItemDone for each response item).
    pub events: Vec<ResponseEvent>,
    /// Token usage statistics.
    pub usage: Option<TokenUsage>,
    /// Response ID for conversation continuity (if supported).
    pub response_id: Option<String>,
}

/// Trait for LLM provider adapters.
///
/// Adapters are responsible for:
/// 1. Converting codex-api's Prompt to provider-specific request format
/// 2. Making the API call (non-streaming only for now)
/// 3. Converting provider response back to ResponseEvent stream
/// 4. Mapping provider errors to ApiError
#[async_trait]
pub trait ProviderAdapter: Send + Sync + std::fmt::Debug {
    /// Unique name identifying this adapter (e.g., "genai", "bedrock").
    fn name(&self) -> &str;

    /// Generate a response (non-streaming).
    ///
    /// This is the main entry point for using an adapter. It:
    /// 1. Converts the Prompt to provider format
    /// 2. Makes the API call
    /// 3. Returns ResponseEvents for each output item
    async fn generate(&self, prompt: &Prompt, config: &AdapterConfig)
        -> Result<GenerateResult, ApiError>;

    /// Check if this adapter supports conversation continuity via response IDs.
    fn supports_response_id(&self) -> bool {
        false
    }
}
