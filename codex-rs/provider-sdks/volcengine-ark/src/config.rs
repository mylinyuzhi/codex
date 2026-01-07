//! Client configuration for the Volcengine Ark SDK.

use std::time::Duration;

/// Configuration for the Ark API client.
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
}

impl ClientConfig {
    /// Default base URL for Volcengine Ark API.
    pub const DEFAULT_BASE_URL: &'static str = "https://ark.cn-beijing.volces.com/api/v3";

    /// Default request timeout (10 minutes).
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

    /// Default maximum retries.
    pub const DEFAULT_MAX_RETRIES: i32 = 2;

    /// Create a new configuration with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: Self::DEFAULT_BASE_URL.to_string(),
            timeout: Self::DEFAULT_TIMEOUT,
            max_retries: Self::DEFAULT_MAX_RETRIES,
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

    /// Set the maximum number of retries.
    pub fn max_retries(mut self, retries: i32) -> Self {
        self.max_retries = retries;
        self
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: Self::DEFAULT_BASE_URL.to_string(),
            timeout: Self::DEFAULT_TIMEOUT,
            max_retries: Self::DEFAULT_MAX_RETRIES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_new() {
        let config = ClientConfig::new("test-key");
        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.base_url, ClientConfig::DEFAULT_BASE_URL);
        assert_eq!(config.timeout, ClientConfig::DEFAULT_TIMEOUT);
        assert_eq!(config.max_retries, ClientConfig::DEFAULT_MAX_RETRIES);
    }

    #[test]
    fn test_config_builder() {
        let config = ClientConfig::new("test-key")
            .base_url("https://custom.api.com")
            .timeout(Duration::from_secs(30))
            .max_retries(5);

        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.base_url, "https://custom.api.com");
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.max_retries, 5);
    }
}
