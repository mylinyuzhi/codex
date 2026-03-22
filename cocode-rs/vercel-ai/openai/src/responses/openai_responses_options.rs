use serde::Deserialize;

use crate::chat::openai_chat_options::PromptCacheRetention;
use crate::chat::openai_chat_options::ReasoningEffort;
use crate::chat::openai_chat_options::ServiceTier;
use crate::chat::openai_chat_options::TextVerbosity;
use crate::openai_capabilities::SystemMessageMode;

/// Provider-specific options for the OpenAI Responses API.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIResponsesProviderOptions {
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

/// Extract Responses-specific options from the generic provider options map.
pub fn extract_responses_options(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
) -> OpenAIResponsesProviderOptions {
    provider_options
        .as_ref()
        .and_then(|opts| opts.0.get("openai"))
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| serde_json::from_value::<OpenAIResponsesProviderOptions>(v).ok())
        .unwrap_or_default()
}
