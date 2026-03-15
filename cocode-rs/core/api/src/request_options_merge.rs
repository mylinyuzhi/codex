//! Merge model `request_options` into typed `ProviderOptions`.
//!
//! ALL keys from the generic `request_options` HashMap go directly into
//! `ProviderOptions.extra`. The SDK's `#[serde(flatten)]` on `params.extra`
//! ensures these values override same-named typed fields during serialization
//! via `serde_json::to_value()` → `Map::insert`.
//!
//! Thinking-derived values (from `thinking_convert`) are preserved — they live
//! on typed fields which are set before this merge runs.

use cocode_protocol::ProviderType;
use hyper_sdk::AnthropicOptions;
use hyper_sdk::GeminiOptions;
use hyper_sdk::OpenAIOptions;
use hyper_sdk::VolcengineOptions;
use hyper_sdk::ZaiOptions;
use hyper_sdk::options::ProviderOptions;
use hyper_sdk::options::downcast_options;
use std::collections::HashMap;

/// Merge `request_options` into existing (or new) `ProviderOptions`.
///
/// All keys go to `extra` — the SDK's `#[serde(flatten)]` handles override.
pub fn merge_into_provider_options(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
    provider: ProviderType,
) -> ProviderOptions {
    match provider {
        ProviderType::Openai | ProviderType::OpenaiCompat => {
            merge_openai(existing, request_options)
        }
        ProviderType::Anthropic => merge_anthropic(existing, request_options),
        ProviderType::Gemini => merge_gemini(existing, request_options),
        ProviderType::Volcengine => merge_volcengine(existing, request_options),
        ProviderType::Zai => merge_zai(existing, request_options),
    }
}

fn merge_openai(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
) -> ProviderOptions {
    let mut opts = existing
        .and_then(|e| downcast_options::<OpenAIOptions>(&e).cloned())
        .unwrap_or_default();
    opts.extra
        .extend(request_options.iter().map(|(k, v)| (k.clone(), v.clone())));
    opts.boxed()
}

fn merge_anthropic(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
) -> ProviderOptions {
    let mut opts = existing
        .and_then(|e| downcast_options::<AnthropicOptions>(&e).cloned())
        .unwrap_or_default();
    opts.extra
        .extend(request_options.iter().map(|(k, v)| (k.clone(), v.clone())));
    opts.boxed()
}

fn merge_gemini(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
) -> ProviderOptions {
    let mut opts = existing
        .and_then(|e| downcast_options::<GeminiOptions>(&e).cloned())
        .unwrap_or_default();
    opts.extra
        .extend(request_options.iter().map(|(k, v)| (k.clone(), v.clone())));
    opts.boxed()
}

fn merge_volcengine(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
) -> ProviderOptions {
    let mut opts = existing
        .and_then(|e| downcast_options::<VolcengineOptions>(&e).cloned())
        .unwrap_or_default();
    opts.extra
        .extend(request_options.iter().map(|(k, v)| (k.clone(), v.clone())));
    opts.boxed()
}

fn merge_zai(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
) -> ProviderOptions {
    let mut opts = existing
        .and_then(|e| downcast_options::<ZaiOptions>(&e).cloned())
        .unwrap_or_default();
    opts.extra
        .extend(request_options.iter().map(|(k, v)| (k.clone(), v.clone())));
    opts.boxed()
}

#[cfg(test)]
#[path = "request_options_merge.test.rs"]
mod tests;
