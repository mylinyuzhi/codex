//! Client configuration for Z.AI SDK.

use std::time::Duration;

/// Configuration for the Z.AI / ZhipuAI client.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// API key for authentication.
    pub api_key: String,
    /// Base URL for the API.
    pub base_url: String,
    /// Request timeout.
    pub timeout: Duration,
    /// Maximum number of retries for failed requests.
    pub max_retries: i32,
    /// Whether to disable JWT token caching (use raw API key).
    pub disable_token_cache: bool,
    /// Source channel identifier.
    pub source_channel: Option<String>,
}

impl ClientConfig {
    /// Default base URL for Z.AI API.
    pub const ZAI_BASE_URL: &'static str = "https://api.z.ai/api/paas/v4";

    /// Default base URL for ZhipuAI API.
    pub const ZHIPUAI_BASE_URL: &'static str = "https://open.bigmodel.cn/api/paas/v4";

    /// Default timeout (10 minutes).
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

    /// Default max retries.
    pub const DEFAULT_MAX_RETRIES: i32 = 2;

    /// Create a new configuration for Z.AI client.
    pub fn zai(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: Self::ZAI_BASE_URL.to_string(),
            timeout: Self::DEFAULT_TIMEOUT,
            max_retries: Self::DEFAULT_MAX_RETRIES,
            disable_token_cache: true,
            source_channel: None,
        }
    }

    /// Create a new configuration for ZhipuAI client.
    pub fn zhipuai(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: Self::ZHIPUAI_BASE_URL.to_string(),
            timeout: Self::DEFAULT_TIMEOUT,
            max_retries: Self::DEFAULT_MAX_RETRIES,
            disable_token_cache: true,
            source_channel: None,
        }
    }

    /// Set the base URL.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Set the request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the maximum retries.
    pub fn max_retries(mut self, max_retries: i32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Enable JWT token caching.
    pub fn enable_token_cache(mut self) -> Self {
        self.disable_token_cache = false;
        self
    }

    /// Set source channel.
    pub fn source_channel(mut self, channel: impl Into<String>) -> Self {
        self.source_channel = Some(channel.into());
        self
    }
}
