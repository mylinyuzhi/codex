use std::collections::HashMap;
use std::sync::Arc;

/// Shared configuration passed to each OpenAI model instance.
pub struct OpenAIConfig {
    /// Provider identifier (e.g., "openai.chat", "openai.responses").
    pub provider: String,
    /// Base URL for the API (e.g., "https://api.openai.com/v1").
    pub base_url: String,
    /// Lazy header supplier — called per-request to get auth + custom headers.
    pub headers: Arc<dyn Fn() -> HashMap<String, String> + Send + Sync>,
    /// Optional shared HTTP client for connection pooling.
    pub client: Option<Arc<reqwest::Client>>,
    /// When `true`, `base_url` is treated as the complete endpoint URL and
    /// no API path suffix (e.g., `/chat/completions`) is appended.
    /// Default (`None`/`false`): auto-detect — if `base_url` already ends
    /// with the path, appending is skipped automatically.
    pub full_url: Option<bool>,
}

impl OpenAIConfig {
    /// Build a full URL from a path segment (e.g., "/chat/completions").
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
