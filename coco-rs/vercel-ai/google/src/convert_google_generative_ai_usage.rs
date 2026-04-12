//! Convert Google Generative AI usage metadata to unified usage.

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
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
    #[serde(default)]
    pub traffic_type: Option<String>,
}

/// Convert Google usage metadata to unified Usage type.
pub fn convert_usage(usage: Option<&GoogleUsageMetadata>) -> Usage {
    let Some(usage) = usage else {
        return Usage::empty();
    };

    let prompt = usage.prompt_token_count.unwrap_or(0);
    let cached = usage.cached_content_token_count.unwrap_or(0);
    let candidates = usage.candidates_token_count.unwrap_or(0);
    let thoughts = usage.thoughts_token_count.unwrap_or(0);

    Usage {
        input_tokens: InputTokens {
            total: Some(prompt),
            no_cache: Some(prompt.saturating_sub(cached)),
            cache_read: usage.cached_content_token_count,
            ..Default::default()
        },
        output_tokens: OutputTokens {
            total: Some(candidates + thoughts),
            text: Some(candidates),
            reasoning: usage.thoughts_token_count,
        },
        raw: serde_json::to_value(usage).ok().and_then(|v| {
            if let serde_json::Value::Object(map) = v {
                Some(
                    map.into_iter()
                        .collect::<HashMap<String, serde_json::Value>>(),
                )
            } else {
                None
            }
        }),
    }
}

#[cfg(test)]
#[path = "convert_google_generative_ai_usage.test.rs"]
mod tests;
