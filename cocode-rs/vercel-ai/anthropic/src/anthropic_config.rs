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
}

impl AnthropicConfig {
    /// Build a full URL from a path segment (e.g., "/messages").
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// Get the current headers by invoking the lazy supplier.
    pub fn get_headers(&self) -> HashMap<String, String> {
        (self.headers)()
    }
}
