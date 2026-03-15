//! Convert Google Generative AI usage metadata to unified usage.

use serde::Deserialize;
use serde::Serialize;
use vercel_ai_provider::InputTokens;
use vercel_ai_provider::OutputTokens;
use vercel_ai_provider::Usage;

/// Google API usage metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleUsageMetadata {
    #[serde(default)]
    pub prompt_token_count: Option<u64>,
    #[serde(default)]
    pub candidates_token_count: Option<u64>,
    #[serde(default)]
    pub cached_content_token_count: Option<u64>,
    #[serde(default)]
    pub thoughts_token_count: Option<u64>,
    #[serde(default)]
    pub total_token_count: Option<u64>,
}

/// Convert Google usage metadata to unified Usage type.
pub fn convert_usage(usage: Option<&GoogleUsageMetadata>) -> Usage {
    let Some(usage) = usage else {
        return Usage::empty();
    };

    let input_total = usage.prompt_token_count.unwrap_or(0);
    let output_total = usage.candidates_token_count.unwrap_or(0);
    let cache_read = usage.cached_content_token_count;
    let reasoning = usage.thoughts_token_count;

    Usage {
        input_tokens: InputTokens {
            total: Some(input_total),
            cache_read,
            ..Default::default()
        },
        output_tokens: OutputTokens {
            total: Some(output_total),
            reasoning,
            ..Default::default()
        },
        raw: None,
    }
}

#[cfg(test)]
#[path = "convert_google_generative_ai_usage.test.rs"]
mod tests;
