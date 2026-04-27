use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::openai_capabilities::SystemMessageMode;

/// Reasoning effort level for reasoning models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
}

impl ReasoningEffort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }
}

/// Service tier for request routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceTier {
    Auto,
    Flex,
    Priority,
    Default,
}

impl ServiceTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Flex => "flex",
            Self::Priority => "priority",
            Self::Default => "default",
        }
    }
}

/// Text verbosity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextVerbosity {
    Low,
    Medium,
    High,
}

impl TextVerbosity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Prompt cache retention policy.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum PromptCacheRetention {
    #[serde(rename = "in_memory")]
    InMemory,
    #[serde(rename = "24h")]
    TwentyFourHours,
}

impl PromptCacheRetention {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InMemory => "in_memory",
            Self::TwentyFourHours => "24h",
        }
    }
}

/// Provider-specific options for OpenAI Chat models.
///
/// Extracted from `options.provider_options["openai"]`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIChatProviderOptions {
    pub logit_bias: Option<HashMap<String, f64>>,
    /// `true` for default logprobs, or a number for top N.
    pub logprobs: Option<serde_json::Value>,
    pub parallel_tool_calls: Option<bool>,
    pub user: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub max_completion_tokens: Option<u64>,
    pub store: Option<bool>,
    pub metadata: Option<HashMap<String, String>>,
    pub prediction: Option<serde_json::Value>,
    pub service_tier: Option<ServiceTier>,
    /// Defaults to true when response_format is json_schema.
    pub strict_json_schema: Option<bool>,
    pub text_verbosity: Option<TextVerbosity>,
    pub system_message_mode: Option<SystemMessageMode>,
    pub force_reasoning: Option<bool>,
    pub prompt_cache_key: Option<String>,
    pub prompt_cache_retention: Option<PromptCacheRetention>,
    pub safety_identifier: Option<String>,
}

impl<'de> Deserialize<'de> for SystemMessageMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "system" => Ok(Self::System),
            "developer" => Ok(Self::Developer),
            "remove" => Ok(Self::Remove),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["system", "developer", "remove"],
            )),
        }
    }
}

/// Extract OpenAI Chat-specific options from the generic provider
/// options map.
pub fn extract_openai_options(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
) -> (
    OpenAIChatProviderOptions,
    BTreeMap<String, serde_json::Value>,
) {
    extract_openai_namespace(provider_options)
}

/// Extract the `provider_options["openai"]` namespace into a typed
/// struct + verbatim raw map. Returns `(T::default(), empty)` when
/// the namespace is missing or fails to deserialize.
///
/// The raw map is shallow-merged into the wire body root by the
/// language model after typed writes — opaque to coco-rs; users own
/// correctness.
pub(crate) fn extract_openai_namespace<T>(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
) -> (T, BTreeMap<String, serde_json::Value>)
where
    T: DeserializeOwned + Default,
{
    let Some(map) = provider_options
        .as_ref()
        .and_then(|opts| opts.0.get("openai"))
    else {
        return (T::default(), BTreeMap::new());
    };
    let typed: T = serde_json::to_value(map)
        .and_then(serde_json::from_value)
        .unwrap_or_default();
    let raw: BTreeMap<String, serde_json::Value> =
        map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    (typed, raw)
}

#[cfg(test)]
#[path = "openai_chat_options.test.rs"]
mod tests;
