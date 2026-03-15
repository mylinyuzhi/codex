//! Web fetch configuration types.

use serde::Deserialize;
use serde::Serialize;

/// Web fetch configuration.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct WebFetchConfig {
    /// Request timeout in seconds.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Maximum content length in bytes (after HTML→text conversion).
    #[serde(default = "default_max_content_length")]
    pub max_content_length: usize,
    /// User-Agent header for HTTP requests.
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout_secs(),
            max_content_length: default_max_content_length(),
            user_agent: default_user_agent(),
        }
    }
}

fn default_timeout_secs() -> u64 {
    15
}

fn default_max_content_length() -> usize {
    100_000
}

fn default_user_agent() -> String {
    "cocode/1.0".to_string()
}
