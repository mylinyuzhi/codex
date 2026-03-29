//! Token usage and cost types.

use serde::Deserialize;
use serde::Serialize;

use crate::event_types::TokenUsage;

/// Token usage for a turn or session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

impl From<TokenUsage> for Usage {
    fn from(u: TokenUsage) -> Self {
        Self {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cache_read_tokens: u.cache_read_tokens,
            cache_creation_tokens: u.cache_creation_tokens,
            reasoning_tokens: u.reasoning_tokens,
        }
    }
}
