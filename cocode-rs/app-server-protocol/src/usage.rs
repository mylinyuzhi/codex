//! Token usage and cost types.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Token usage for a turn or session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Usage {
    /// Input tokens consumed.
    #[serde(default)]
    pub input_tokens: i64,
    /// Output tokens generated.
    #[serde(default)]
    pub output_tokens: i64,
    /// Tokens served from prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,
    /// Tokens written to prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<i64>,
    /// Tokens used for extended thinking / reasoning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i64>,
}

impl Usage {
    /// Total tokens (input + output).
    pub fn total(&self) -> i64 {
        self.input_tokens + self.output_tokens
    }
}
