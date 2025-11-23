//! Provider adapter system for multi-LLM support
//!
//! This module provides a flexible adapter pattern that allows codex-rs to support
//! multiple LLM providers (Anthropic, Google Gemini, etc.) with minimal code changes.
//!
//! # Architecture
//!
//! ```text
//! ModelClient (orchestration)
//!     ↓
//! ProviderAdapter (protocol transformation)
//!     ↓
//! Wire API Layer (HTTP communication - existing code)
//! ```

use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::error::Result;
use crate::model_provider_info::ModelProviderInfo;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Context for stateful response chunk processing
///
/// Some streaming APIs send data across multiple chunks that need to be
/// accumulated or tracked. For example:
///
/// - Anthropic: `content_block_start` → `content_block_delta` (x N) → `content_block_stop`
/// - Multi-part tool calls that span chunks
///
/// Adapters can store arbitrary state in this context to handle such cases.
///
/// # Lifetime & Memory Management
///
/// **IMPORTANT**: `AdapterContext` is scoped to a single streaming request.
///
/// - **Created**: Once per request in `process_sse_with_adapter()`
/// - **Used**: Passed to `transform_response_chunk()` for each SSE chunk
/// - **Destroyed**: Automatically when request completes (Rust RAII)
///
/// This design ensures:
/// - No memory leaks across requests
/// - No manual cleanup needed
/// - Automatic deallocation via Rust's ownership system
///
/// ## Memory Accumulation Pattern
///
/// State accumulates during a single request:
///
/// ```text
/// Request Start:  context.state = {}
///   Chunk 1:      context.state["parser"] = {...assistant_text: "Hello"}
///   Chunk 2:      context.state["parser"] = {...assistant_text: "Hello world"}
///   Chunk N:      context.state["parser"] = {...assistant_text: "Hello world!"}
/// Request End:    context drops → HashMap freed → all memory released
/// ```
///
/// ## Typical Memory Usage (per request)
///
/// - **GptOpenapiAdapter**: 1-200 KB (accumulated response text)
///
/// **Note**: Long responses may cause temporary memory peaks (e.g., 1-10 MB for
/// large document generation), but this is **not a leak** as memory is released
/// when the request completes.
#[derive(Debug, Default)]
pub struct AdapterContext {
    /// Arbitrary state storage for multi-chunk parsing
    ///
    /// Adapters can use this to store:
    /// - **Serialized parser state** (GptOpenapiAdapter)
    ///   - Accumulated assistant text across chunks
    ///   - Partial tool call arguments
    ///   - Reasoning content buffers
    /// - **Simple metadata** (AnthropicAdapter)
    ///   - Current block IDs
    ///   - Message IDs
    ///   - Block indices
    ///
    /// ## Memory Growth Pattern
    ///
    /// State grows during request processing:
    /// - Initial: Empty HashMap (24-48 bytes)
    /// - Per chunk: Adds/updates entries (typically 1-5 KB per chunk)
    /// - Peak: Sum of all accumulated state (typically < 1 MB, may reach 10 MB for very long responses)
    /// - End: Entire HashMap dropped and freed automatically
    ///
    /// **No manual cleanup required** - Rust's ownership ensures deallocation.
    pub state: HashMap<String, JsonValue>,
}

impl AdapterContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self {
            state: HashMap::new(),
        }
    }

    /// Get state value as string reference
    ///
    /// Returns `None` if key doesn't exist or value is not a string.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.state.get(key)?.as_str()
    }

    /// Set state value
    pub fn set(&mut self, key: impl Into<String>, value: JsonValue) {
        self.state.insert(key.into(), value);
    }

    /// Get state value as i64
    ///
    /// Returns `None` if key doesn't exist or value is not a number.
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.state.get(key)?.as_i64()
    }

    /// Get state value as bool
    ///
    /// Returns `None` if key doesn't exist or value is not a boolean.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.state.get(key)?.as_bool()
    }

    /// Remove a key from state and return its value
    pub fn remove(&mut self, key: &str) -> Option<JsonValue> {
        self.state.remove(key)
    }

    /// Check if a key exists in state
    pub fn contains_key(&self, key: &str) -> bool {
        self.state.contains_key(key)
    }

    /// Clear all state
    pub fn clear(&mut self) {
        self.state.clear();
    }
}

/// Runtime context passed to adapters for request building
///
/// Contains runtime information that adapters can use to dynamically construct
/// HTTP headers, query parameters, or request metadata.
#[derive(Debug, Clone)]
pub struct RequestContext {
    // ===== Runtime context (existing) =====
    /// Unique identifier for the current conversation
    ///
    /// This is typically used for:
    /// - Tracking requests across multiple API calls
    /// - Log correlation in enterprise LLM gateways
    /// - Session identification
    pub conversation_id: String,

    /// Source/origin of the session
    ///
    /// Possible values: "Cli", "VSCode", "Exec", "Mcp", "SubAgent", "Unknown"
    ///
    /// This can be used to:
    /// - Add telemetry headers
    /// - Implement source-specific request handling
    /// - Debug/audit request origins
    pub session_source: String,

    // ===== Model configuration parameters (new) =====
    /// Effective model sampling parameters resolved from Config and ModelProviderInfo.
    ///
    /// Source: ModelClient.resolve_parameters()
    /// Lifecycle: Per-turn (may change if provider config changes)
    ///
    /// Adapters use these to control model behavior (temperature, top_p, etc.).
    /// If a parameter is None, the adapter should not include it in the request.
    pub effective_parameters: codex_protocol::config_types_ext::ModelParameters,

    /// Reasoning effort level for models that support reasoning.
    ///
    /// Source: ModelClient.effort (from Config.model_reasoning_effort)
    /// Lifecycle: Per-session (stable across turns)
    ///
    /// Values: None | Low | Medium | High
    pub reasoning_effort: Option<codex_protocol::config_types::ReasoningEffort>,

    /// Reasoning summary configuration.
    ///
    /// Source: ModelClient.summary (from Config.model_reasoning_summary)
    /// Lifecycle: Per-session (stable across turns)
    ///
    /// Controls how reasoning content is presented (Detailed vs Concise).
    pub reasoning_summary: Option<codex_protocol::config_types::ReasoningSummary>,

    /// Verbosity level for models that support it (GPT-5 family).
    ///
    /// Source: Config.model_verbosity or ModelFamily.default_verbosity
    /// Lifecycle: Per-session (stable across turns)
    ///
    /// Controls output length and detail level:
    /// - Low: Concise responses
    /// - Medium: Balanced detail (default for GPT-5.1)
    /// - High: Detailed responses with explanations
    ///
    /// Only effective for models with ModelFamily.support_verbosity = true.
    pub verbosity: Option<codex_protocol::config_types::Verbosity>,
}

/// HTTP metadata that adapters can dynamically add to requests
///
/// Adapters return this from `build_request_metadata()` to specify additional
/// HTTP headers and query parameters that should be included in the request.
#[derive(Debug, Clone, Default)]
pub struct RequestMetadata {
    /// HTTP headers to add to the request
    ///
    /// These headers are applied in addition to:
    /// - Static headers from `ModelProviderInfo.http_headers`
    /// - Authentication headers (Bearer token)
    /// - Standard headers (content-type, accept, etc.)
    pub headers: HashMap<String, String>,

    /// Query parameters to add to the request URL
    ///
    /// These params are appended to the URL in addition to:
    /// - Static params from `ModelProviderInfo.query_params`
    pub query_params: HashMap<String, String>,
}

/// Configuration for a provider adapter
///
/// Adapters can use this to customize their behavior on a per-provider basis.
/// Configuration is loaded from the provider's `adapter_config` field in config.toml.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdapterConfig {
    /// Generic configuration options
    #[serde(flatten)]
    pub options: HashMap<String, JsonValue>,
}

impl AdapterConfig {
    /// Create a new empty configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a string configuration value
    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.options.get(key)?.as_str()
    }

    /// Get a boolean configuration value
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.options.get(key)?.as_bool()
    }

    /// Get an integer configuration value
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.options.get(key)?.as_i64()
    }

    /// Get an array configuration value
    pub fn get_array(&self, key: &str) -> Option<&Vec<JsonValue>> {
        self.options.get(key)?.as_array()
    }

    /// Set a configuration value
    pub fn set(&mut self, key: impl Into<String>, value: JsonValue) {
        self.options.insert(key.into(), value);
    }

    /// Check if a key exists
    pub fn contains_key(&self, key: &str) -> bool {
        self.options.contains_key(key)
    }
}

/// Provider adapter for transforming requests/responses between formats
///
/// This trait allows implementing protocol translation for different LLM providers
/// while reusing the existing HTTP communication layer.
pub trait ProviderAdapter: Send + Sync + std::fmt::Debug {
    /// Unique identifier for this adapter (e.g., "anthropic", "gemini")
    ///
    /// This name is used in configuration:
    /// ```toml
    /// [model_providers.my_provider]
    /// adapter = "anthropic"  # ← This name
    /// ```
    fn name(&self) -> &str;

    /// Configure the adapter with provider-specific settings
    ///
    /// This method is called when the adapter is initialized with a provider's
    /// `adapter_config` settings. Adapters can override this to customize behavior.
    ///
    /// # Default Implementation
    ///
    /// Does nothing - adapters that don't need configuration can skip this.
    fn configure(&mut self, _config: &AdapterConfig) -> Result<()> {
        Ok(())
    }

    /// Validate adapter configuration
    ///
    /// This method is called after `configure()` to ensure the configuration is valid.
    /// Adapters can return errors for invalid configurations.
    ///
    /// # Default Implementation
    ///
    /// Always succeeds - adapters that don't need validation can skip this.
    fn validate_config(&self, _config: &AdapterConfig) -> Result<()> {
        Ok(())
    }

    /// Validate adapter compatibility with provider settings
    ///
    /// This method is called after `validate_config()` to ensure the adapter is
    /// compatible with the provider's configuration (e.g., wire_api, base_url).
    ///
    /// Adapters that have strict requirements (e.g., only support Responses API)
    /// can override this to reject incompatible configurations early.
    fn validate_provider(
        &self,
        _provider: &crate::model_provider_info::ModelProviderInfo,
    ) -> Result<()> {
        Ok(())
    }

    /// Get the API endpoint path for this adapter
    ///
    /// This method returns the endpoint path (without base URL) that should be used
    /// for API requests. The default implementation returns `None`, which means the
    /// endpoint will be determined by the provider's `wire_api` configuration.
    fn endpoint_path(&self) -> Option<&str> {
        None
    }

    /// Transform unified Prompt into provider-specific request body
    ///
    /// This method maps the codex-rs `Prompt` format to the provider's API format.
    fn transform_request(
        &self,
        prompt: &Prompt,
        context: &RequestContext,
        provider: &ModelProviderInfo,
    ) -> Result<JsonValue>;

    /// Build dynamic HTTP metadata (headers, query params) for the request
    ///
    /// This method allows adapters to add runtime-specific HTTP headers and query
    /// parameters based on the current session context (conversation_id, session_source).
    ///
    /// # Default Implementation
    ///
    /// Returns empty metadata - adapters that don't need dynamic headers can skip this.
    /// # Notes
    ///
    /// - Headers from this method are applied AFTER static headers from `ModelProviderInfo`
    /// - Dynamic headers can override static headers with the same name
    /// - Query params are appended to those from `ModelProviderInfo.query_params`
    fn build_request_metadata(
        &self,
        _prompt: &Prompt,
        _provider: &ModelProviderInfo,
        _context: &RequestContext,
    ) -> Result<RequestMetadata> {
        // Default: no dynamic metadata
        Ok(RequestMetadata::default())
    }

    /// Check if this adapter supports previous_response_id for conversation continuity
    /// - previous_response_id is automatically cleared on compact or model switch
    /// - Only works with Responses API (WireApi::Responses), not Chat API
    fn supports_previous_response_id(&self) -> bool {
        false
    }

    /// Transform provider's SSE chunk or complete JSON response into unified ResponseEvent(s)
    ///
    /// This method parses provider-specific responses and converts them
    /// to codex-rs `ResponseEvent` types.
    fn transform_response_chunk(
        &self,
        chunk: &str,
        context: &mut AdapterContext,
        provider: &crate::model_provider_info::ModelProviderInfo,
    ) -> Result<Vec<ResponseEvent>>;
}

// Re-export submodules
pub mod gpt_openapi;
pub(crate) mod http;
pub mod item_utils;
pub mod openai_common;
pub mod registry;

pub use gpt_openapi::GeminiAdapter;
pub use gpt_openapi::GptAdapter;
pub use item_utils::filter_incremental_input;
pub use item_utils::get_item_type_name;
pub use item_utils::get_item_type_names;
pub use item_utils::is_llm_generated;
pub use registry::get_adapter;
pub use registry::list_adapters;
pub use registry::register_adapter;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_adapter_context_new() {
        let ctx = AdapterContext::new();
        assert!(ctx.state.is_empty());
    }

    #[test]
    fn test_adapter_context_set_get() {
        let mut ctx = AdapterContext::new();

        ctx.set("key1", json!("value1"));
        assert_eq!(ctx.get_str("key1"), Some("value1"));

        ctx.set("key2", json!(42));
        assert_eq!(ctx.get_i64("key2"), Some(42));

        ctx.set("key3", json!(true));
        assert_eq!(ctx.get_bool("key3"), Some(true));
    }

    #[test]
    fn test_adapter_context_get_wrong_type() {
        let mut ctx = AdapterContext::new();

        ctx.set("string_key", json!("value"));
        // Trying to get as i64 should return None
        assert_eq!(ctx.get_i64("string_key"), None);

        ctx.set("number_key", json!(42));
        // Trying to get as str should return None
        assert_eq!(ctx.get_str("number_key"), None);
    }

    #[test]
    fn test_adapter_context_get_nonexistent() {
        let ctx = AdapterContext::new();
        assert_eq!(ctx.get_str("nonexistent"), None);
        assert_eq!(ctx.get_i64("nonexistent"), None);
        assert_eq!(ctx.get_bool("nonexistent"), None);
    }

    #[test]
    fn test_adapter_context_remove() {
        let mut ctx = AdapterContext::new();

        ctx.set("key", json!("value"));
        assert!(ctx.contains_key("key"));

        let removed = ctx.remove("key");
        assert_eq!(removed, Some(json!("value")));
        assert!(!ctx.contains_key("key"));
    }

    #[test]
    fn test_adapter_context_contains_key() {
        let mut ctx = AdapterContext::new();

        assert!(!ctx.contains_key("key"));
        ctx.set("key", json!("value"));
        assert!(ctx.contains_key("key"));
    }

    #[test]
    fn test_adapter_context_clear() {
        let mut ctx = AdapterContext::new();

        ctx.set("key1", json!("value1"));
        ctx.set("key2", json!(42));
        assert_eq!(ctx.state.len(), 2);

        ctx.clear();
        assert!(ctx.state.is_empty());
    }
}
