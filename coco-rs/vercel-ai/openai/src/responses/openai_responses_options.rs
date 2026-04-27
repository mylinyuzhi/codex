use serde::Deserialize;

use crate::chat::openai_chat_options::PromptCacheRetention;
use crate::chat::openai_chat_options::ReasoningEffort;
use crate::chat::openai_chat_options::ServiceTier;
use crate::chat::openai_chat_options::TextVerbosity;
use crate::openai_capabilities::SystemMessageMode;

/// A context management entry for server-side compaction.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextManagementEntry {
    #[serde(rename = "type")]
    pub entry_type: String,
    pub compact_threshold: f64,
}

/// Provider-specific options for the OpenAI Responses API.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIResponsesProviderOptions {
    pub context_management: Option<Vec<ContextManagementEntry>>,
    pub conversation: Option<String>,
    pub include: Option<Vec<String>>,
    pub instructions: Option<String>,
    pub logprobs: Option<serde_json::Value>,
    pub max_tool_calls: Option<u64>,
    pub metadata: Option<serde_json::Value>,
    pub parallel_tool_calls: Option<bool>,
    pub previous_response_id: Option<String>,
    pub prompt_cache_key: Option<String>,
    pub prompt_cache_retention: Option<PromptCacheRetention>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub reasoning_summary: Option<String>,
    pub safety_identifier: Option<String>,
    pub service_tier: Option<ServiceTier>,
    pub store: Option<bool>,
    pub strict_json_schema: Option<bool>,
    pub text_verbosity: Option<TextVerbosity>,
    pub truncation: Option<String>,
    pub user: Option<String>,
    pub system_message_mode: Option<SystemMessageMode>,
    pub force_reasoning: Option<bool>,
}

/// Extract OpenAI Responses-specific options from the generic
/// provider options map.
///
/// Returns `(typed, raw)`:
///
/// - `typed` — parsed `OpenAIResponsesProviderOptions`, used for
///   reasoning-model / system-message-mode side-effects and typed
///   body writes (`include` auto-fill, `top_logprobs`, etc.).
/// - `raw` — verbatim user-supplied `provider_options["openai"]`
///   map. The language model shallow-merges this into the wire body
///   root **as-is**, every key wins over earlier typed body writes
///   (multi-provider-plan §7.3). Opaque to coco-rs; users own
///   correctness.
pub fn extract_responses_options(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
) -> (
    OpenAIResponsesProviderOptions,
    std::collections::BTreeMap<String, serde_json::Value>,
) {
    let raw_value = provider_options
        .as_ref()
        .and_then(|opts| opts.0.get("openai"))
        .and_then(|v| serde_json::to_value(v).ok());
    let typed: OpenAIResponsesProviderOptions = raw_value
        .clone()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let mut raw = std::collections::BTreeMap::new();
    if let Some(serde_json::Value::Object(map)) = raw_value {
        for (k, v) in map {
            raw.insert(k, v);
        }
    }
    (typed, raw)
}

#[cfg(test)]
#[path = "openai_responses_options.test.rs"]
mod tests;
