use std::collections::HashMap;
use std::sync::Arc;

/// Shared configuration passed to each Anthropic model instance.
pub struct AnthropicConfig {
    /// Provider identifier (e.g., "anthropic.messages").
    pub provider: String,
    /// Base URL for the API (e.g., "https://api.anthropic.com/v1").
    pub base_url: String,
    /// Lazy header supplier — called per-request to get auth + custom headers.
    pub headers: Arc<dyn Fn() -> HashMap<String, String> + Send + Sync>,
    /// Optional shared HTTP client for connection pooling.
    pub client: Option<Arc<reqwest::Client>>,
    /// When false, the model will use JSON tool fallback for structured outputs.
    /// Defaults to true.
    pub supports_native_structured_output: Option<bool>,
    /// When false, `strict` on tool definitions will be ignored and a warning emitted.
    /// Defaults to true.
    pub supports_strict_tools: Option<bool>,
    /// When `true`, `base_url` is the complete endpoint URL — no API path
    /// suffix is appended. Default (`None`): auto-detect duplicate suffixes.
    pub full_url: Option<bool>,
}

impl AnthropicConfig {
    /// Build a full URL from a path segment (e.g., "/messages").
    ///
    /// If `full_url` is set, or `base_url` already ends with the path,
    /// returns `base_url` as-is to avoid duplication.
    pub fn url(&self, path: &str) -> String {
        if self.full_url.unwrap_or(false) || self.base_url.ends_with(path) {
            self.base_url.clone()
        } else {
            format!("{}{path}", self.base_url)
        }
    }

    /// Get the current headers by invoking the lazy supplier.
    pub fn get_headers(&self) -> HashMap<String, String> {
        (self.headers)()
    }
}
