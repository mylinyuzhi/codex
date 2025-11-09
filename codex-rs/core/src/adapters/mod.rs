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
//!
//! # Example
//!
//! ```rust,ignore
//! use codex_core::adapters::{get_adapter, BaseWireApi};
//!
//! // Get an adapter
//! let adapter = get_adapter("anthropic")?;
//!
//! // Transform request
//! let request_body = adapter.transform_request(&prompt, &provider)?;
//!
//! // Use appropriate wire API based on adapter's base protocol
//! match adapter.base_wire_api() {
//!     BaseWireApi::Chat => { /* use chat completions */ }
//!     BaseWireApi::Responses => { /* use responses API */ }
//!     BaseWireApi::Custom => { /* adapter handles HTTP */ }
//! }
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
/// - **PassthroughAdapter/GptOpenapiAdapter**: 1-200 KB (accumulated response text)
/// - **AnthropicAdapter**: < 100 bytes (simple metadata tracking)
///
/// **Note**: Long responses may cause temporary memory peaks (e.g., 1-10 MB for
/// large document generation), but this is **not a leak** as memory is released
/// when the request completes.
///
/// # Usage Examples
///
/// ## Example 1: Storing Serialized Parser State
///
/// ```rust
/// use codex_core::adapters::AdapterContext;
/// use serde_json::json;
///
/// let mut ctx = AdapterContext::new();
///
/// // PassthroughAdapter pattern: serialize entire parser state
/// let parser = ChatCompletionsParserState::new();
/// ctx.state.insert(
///     "chat_parser_state".to_string(),
///     serde_json::to_value(&parser).unwrap()
/// );
///
/// // Later: deserialize and continue parsing
/// let mut parser: ChatCompletionsParserState =
///     serde_json::from_value(ctx.state["chat_parser_state"].clone()).unwrap();
/// ```
///
/// ## Example 2: Simple Key-Value Tracking
///
/// ```rust
/// use codex_core::adapters::AdapterContext;
/// use serde_json::json;
///
/// let mut ctx = AdapterContext::new();
///
/// // AnthropicAdapter pattern: track metadata
/// ctx.set("message_id", json!("msg_123"));
/// ctx.set("current_block_index", json!(0));
///
/// // Retrieve it later
/// if let Some(block_id) = ctx.get_str("message_id") {
///     println!("Processing message: {}", block_id);
/// }
///
/// // Clean up when block finishes
/// ctx.remove("current_block_index");
/// ```
#[derive(Debug, Default)]
pub struct AdapterContext {
    /// Arbitrary state storage for multi-chunk parsing
    ///
    /// Adapters can use this to store:
    /// - **Serialized parser state** (PassthroughAdapter, GptOpenapiAdapter)
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
    ///
    /// # Example
    ///
    /// ```
    /// use codex_core::adapters::AdapterContext;
    /// use serde_json::json;
    ///
    /// let mut ctx = AdapterContext::new();
    /// ctx.set("key", json!("value"));
    /// ctx.set("number", json!(42));
    /// ```
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
///
/// # Fields
///
/// - `conversation_id`: Unique identifier for the current conversation/session
/// - `session_source`: Origin of the session (CLI, VSCode, Exec, MCP, SubAgent, etc.)
///
/// # Example
///
/// ```rust,ignore
/// let context = RequestContext {
///     conversation_id: "conv_123".to_string(),
///     session_source: "Cli".to_string(),
/// };
///
/// // Adapter can use this to add custom headers
/// let metadata = adapter.build_request_metadata(&prompt, &provider, &context)?;
/// ```
#[derive(Debug, Clone)]
pub struct RequestContext {
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
}

/// HTTP metadata that adapters can dynamically add to requests
///
/// Adapters return this from `build_request_metadata()` to specify additional
/// HTTP headers and query parameters that should be included in the request.
///
/// # Example
///
/// ```rust,ignore
/// // In adapter implementation:
/// fn build_request_metadata(
///     &self,
///     _prompt: &Prompt,
///     _provider: &ModelProviderInfo,
///     context: &RequestContext,
/// ) -> Result<RequestMetadata> {
///     let mut metadata = RequestMetadata::default();
///
///     // Add conversation ID as log correlation header
///     metadata.headers.insert(
///         "x-log-id".to_string(),
///         context.conversation_id.clone(),
///     );
///
///     // Add session source for telemetry
///     metadata.headers.insert(
///         "x-session-source".to_string(),
///         context.session_source.clone(),
///     );
///
///     Ok(metadata)
/// }
/// ```
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
///
/// # Example
///
/// ```toml
/// [model_providers.anthropic]
/// adapter = "anthropic"
///
/// [model_providers.anthropic.adapter_config]
/// api_version = "2023-12-15"
/// default_max_tokens = 8192
/// beta_features = ["messages-2023-12-15"]
/// ```
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
///
/// # Example Implementation
///
/// ```rust,ignore
/// use codex_core::adapters::{ProviderAdapter, BaseWireApi, AdapterContext};
/// use codex_core::client_common::{Prompt, ResponseEvent};
/// use codex_core::model_provider_info::ModelProviderInfo;
/// use codex_core::error::Result;
/// use serde_json::{json, Value};
///
/// struct AnthropicAdapter;
///
/// impl ProviderAdapter for AnthropicAdapter {
///     fn name(&self) -> &str {
///         "anthropic"
///     }
///
///     fn base_wire_api(&self) -> BaseWireApi {
///         BaseWireApi::Chat  // Reuse Chat Completions HTTP layer
///     }
///
///     fn transform_request(&self, prompt: &Prompt, _provider: &ModelProviderInfo)
///         -> Result<Value>
///     {
///         // Transform to Anthropic Messages API format
///         Ok(json!({
///             "model": "claude-3-5-sonnet-20241022",
///             "messages": prompt.input,  // Map input to messages
///             "max_tokens": 4096,
///             "anthropic_version": "2023-06-01"
///         }))
///     }
///
///     fn transform_response_chunk(&self, chunk: &str, _ctx: &mut AdapterContext)
///         -> Result<Vec<ResponseEvent>>
///     {
///         let event: Value = serde_json::from_str(chunk)?;
///
///         // Parse Anthropic SSE events
///         match event["type"].as_str() {
///             Some("content_block_delta") => {
///                 let text = event["delta"]["text"].as_str().unwrap_or("");
///                 Ok(vec![ResponseEvent::OutputTextDelta(text.to_string())])
///             }
///             Some("message_stop") => {
///                 Ok(vec![ResponseEvent::Completed {
///                     response_id: "resp-1".to_string(),
///                     token_usage: None,
///                 }])
///             }
///             _ => Ok(vec![])
///         }
///     }
/// }
/// ```
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

    /// Get the API endpoint path for this adapter
    ///
    /// This method returns the endpoint path (without base URL) that should be used
    /// for API requests. The default implementation returns `None`, which means the
    /// endpoint will be determined by the provider's `wire_api` configuration.
    ///
    /// # Default Implementation
    ///
    /// Returns `None` - uses provider's `wire_api` to determine endpoint:
    /// - `WireApi::Chat` → `/chat/completions`
    /// - `WireApi::Responses` → `/responses`
    ///
    /// # Example Override
    ///
    /// ```rust,ignore
    /// // Anthropic uses custom endpoint
    /// fn endpoint_path(&self) -> Option<&str> {
    ///     Some("/v1/messages")
    /// }
    /// ```
    fn endpoint_path(&self) -> Option<&str> {
        None
    }

    /// Transform unified Prompt into provider-specific request body
    ///
    /// This method maps the codex-rs `Prompt` format to the provider's API format.
    ///
    /// # Arguments
    ///
    /// * `prompt` - Unified prompt format containing conversation history and tools
    /// * `provider` - Provider configuration (for accessing base_url, headers, etc.)
    ///
    /// # Returns
    ///
    /// JSON value representing the provider's API request format
    ///
    /// # Example Transformations
    ///
    /// ```text
    /// OpenAI Chat Completions:
    ///   Prompt.input → {"messages": [...]}
    ///
    /// Anthropic Messages API:
    ///   Prompt.input → {"messages": [...], "anthropic_version": "2023-06-01"}
    ///
    /// Google Gemini:
    ///   Prompt.input → {"contents": [...], "generationConfig": {...}}
    /// ```
    fn transform_request(&self, prompt: &Prompt, provider: &ModelProviderInfo)
    -> Result<JsonValue>;

    /// Build dynamic HTTP metadata (headers, query params) for the request
    ///
    /// This method allows adapters to add runtime-specific HTTP headers and query
    /// parameters based on the current session context (conversation_id, session_source).
    ///
    /// # Default Implementation
    ///
    /// Returns empty metadata - adapters that don't need dynamic headers can skip this.
    ///
    /// # Arguments
    ///
    /// * `prompt` - Unified prompt format containing conversation history and tools
    /// * `provider` - Provider configuration (for accessing adapter_config, etc.)
    /// * `context` - Runtime context (conversation_id, session_source)
    ///
    /// # Returns
    ///
    /// `RequestMetadata` with headers and query params to add to the HTTP request
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // AnthropicAdapter: Add API version header
    /// fn build_request_metadata(
    ///     &self,
    ///     _prompt: &Prompt,
    ///     _provider: &ModelProviderInfo,
    ///     _context: &RequestContext,
    /// ) -> Result<RequestMetadata> {
    ///     let mut metadata = RequestMetadata::default();
    ///     metadata.headers.insert(
    ///         "anthropic-version".to_string(),
    ///         "2023-06-01".to_string(),
    ///     );
    ///     Ok(metadata)
    /// }
    ///
    /// // Enterprise Gateway: Add session tracking headers
    /// fn build_request_metadata(
    ///     &self,
    ///     _prompt: &Prompt,
    ///     _provider: &ModelProviderInfo,
    ///     context: &RequestContext,
    /// ) -> Result<RequestMetadata> {
    ///     let mut metadata = RequestMetadata::default();
    ///     metadata.headers.insert(
    ///         "x-log-id".to_string(),
    ///         context.conversation_id.clone(),
    ///     );
    ///     metadata.headers.insert(
    ///         "x-session-source".to_string(),
    ///         context.session_source.clone(),
    ///     );
    ///     Ok(metadata)
    /// }
    /// ```
    ///
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
    ///
    /// This method indicates whether the adapter's target API supports continuing
    /// conversations using a previous response ID (e.g., OpenAI Responses API).
    ///
    /// When this returns `true`, the ModelClient will:
    /// - Populate `prompt.previous_response_id` from SessionState
    /// - Include `previous_response_id` in the request JSON
    /// - Store response IDs in SessionState for next turn
    ///
    /// # Default Implementation
    ///
    /// Returns `false` - most adapters don't support this feature.
    ///
    /// # Supported Adapters
    ///
    /// - `PassthroughAdapter`: Returns `true` (OpenAI Responses API)
    /// - `GptOpenapiAdapter`: Returns `true` (OpenAI-compatible gateways)
    /// - Others: Return `false`
    ///
    /// # Notes
    ///
    /// - previous_response_id is automatically cleared on compact or model switch
    /// - Only works with Responses API (WireApi::Responses), not Chat API
    fn supports_previous_response_id(&self) -> bool {
        false
    }

    /// Transform provider's SSE chunk into unified ResponseEvent(s)
    ///
    /// This method parses provider-specific streaming events and converts them
    /// to codex-rs `ResponseEvent` types.
    ///
    /// # Arguments
    ///
    /// * `chunk` - Raw SSE data line (without "data: " prefix)
    /// * `context` - Stateful context for multi-chunk parsing
    ///
    /// # Returns
    ///
    /// Vector of ResponseEvent (can be empty, one, or multiple events per chunk)
    ///
    /// # Notes
    ///
    /// - Return empty `vec![]` for chunks that don't map to events (e.g., metadata)
    /// - Return multiple events if one chunk contains multiple logical events
    /// - Use `context` to track state across chunks (e.g., accumulating tool calls)
    ///
    /// # Example
    ///
    /// ```text
    /// Anthropic SSE:
    ///   {"type":"content_block_delta","delta":{"text":"Hi"}}
    ///   → vec![ResponseEvent::OutputTextDelta("Hi".to_string())]
    ///
    /// Anthropic SSE:
    ///   {"type":"message_start"}
    ///   → vec![ResponseEvent::Created]
    ///
    /// Malformed chunk:
    ///   "incomplete json..."
    ///   → Err(...)  // Parser errors propagate
    /// ```
    fn transform_response_chunk(
        &self,
        chunk: &str,
        context: &mut AdapterContext,
    ) -> Result<Vec<ResponseEvent>>;
}

// Re-export submodules
pub mod anthropic;
pub mod gpt_openapi;
pub mod openai_common;
pub mod passthrough;
pub mod registry;

pub use anthropic::AnthropicAdapter;
pub use gpt_openapi::GptOpenapiAdapter;
pub use passthrough::PassthroughAdapter;
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
