//! Token usage types for model responses.
//!
//! Matches the TS v4 `LanguageModelV4Usage` type with nested
//! `inputTokens` and `outputTokens` structures.

use serde::Deserialize;
use serde::Serialize;

use crate::json_value::JSONObject;

/// Token usage information for a model response.
///
/// Matches the TypeScript `LanguageModelV4Usage` type with nested
/// `inputTokens` and `outputTokens` structures.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Input token breakdown.
    pub input_tokens: InputTokens,
    /// Output token breakdown.
    pub output_tokens: OutputTokens,
    /// Raw usage data from the provider (for pass-through).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<JSONObject>,
}

impl Usage {
    /// Create a new Usage with the given total input/output token counts.
    pub fn new(input_total: u64, output_total: u64) -> Self {
        Self {
            input_tokens: InputTokens {
                total: Some(input_total),
                ..Default::default()
            },
            output_tokens: OutputTokens {
                total: Some(output_total),
                ..Default::default()
            },
            raw: None,
        }
    }

    /// Create an empty Usage.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Get total input tokens.
    pub fn total_input_tokens(&self) -> u64 {
        self.input_tokens.total.unwrap_or(0)
    }

    /// Get total output tokens.
    pub fn total_output_tokens(&self) -> u64 {
        self.output_tokens.total.unwrap_or(0)
    }

    /// Get total tokens (input + output).
    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens() + self.total_output_tokens()
    }

    /// Set input tokens details.
    pub fn with_input_tokens(mut self, input_tokens: InputTokens) -> Self {
        self.input_tokens = input_tokens;
        self
    }

    /// Set output tokens details.
    pub fn with_output_tokens(mut self, output_tokens: OutputTokens) -> Self {
        self.output_tokens = output_tokens;
        self
    }

    /// Set raw usage data.
    pub fn with_raw(mut self, raw: JSONObject) -> Self {
        self.raw = Some(raw);
        self
    }

    /// Add usage from another Usage (accumulate tokens).
    pub fn add(&mut self, other: &Usage) {
        self.input_tokens.total =
            Some(self.input_tokens.total.unwrap_or(0) + other.input_tokens.total.unwrap_or(0));
        self.output_tokens.total =
            Some(self.output_tokens.total.unwrap_or(0) + other.output_tokens.total.unwrap_or(0));
    }
}

/// Breakdown of input (prompt) tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputTokens {
    /// Total input tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Tokens that were not served from cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_cache: Option<u64>,
    /// Tokens that were served from cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
    /// Tokens that were written to cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u64>,
}

/// Breakdown of output (completion) tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTokens {
    /// Total output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Tokens used for text output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<u64>,
    /// Tokens used for reasoning output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
}

#[cfg(test)]
#[path = "usage.test.rs"]
mod tests;
