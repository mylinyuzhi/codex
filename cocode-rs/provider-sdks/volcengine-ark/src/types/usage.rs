//! Token usage types.

use serde::Deserialize;
use serde::Serialize;

/// Detailed breakdown of input tokens.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputTokensDetails {
    /// Number of tokens retrieved from cache.
    #[serde(default)]
    pub cached_tokens: i32,
}

/// Detailed breakdown of output tokens.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputTokensDetails {
    /// Number of reasoning tokens.
    #[serde(default)]
    pub reasoning_tokens: i32,
}

/// Token usage information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Number of input tokens.
    pub input_tokens: i32,

    /// Number of output tokens.
    pub output_tokens: i32,

    /// Total number of tokens.
    #[serde(default)]
    pub total_tokens: i32,

    /// Detailed breakdown of input tokens.
    #[serde(default)]
    pub input_tokens_details: InputTokensDetails,

    /// Detailed breakdown of output tokens.
    #[serde(default)]
    pub output_tokens_details: OutputTokensDetails,
}

impl Usage {
    /// Get reasoning tokens from output details.
    pub fn reasoning_tokens(&self) -> i32 {
        self.output_tokens_details.reasoning_tokens
    }

    /// Get cached tokens from input details.
    pub fn cached_tokens(&self) -> i32 {
        self.input_tokens_details.cached_tokens
    }
}

#[cfg(test)]
#[path = "usage.test.rs"]
mod tests;
