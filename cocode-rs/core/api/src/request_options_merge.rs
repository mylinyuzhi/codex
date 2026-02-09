//! Merge model `request_options` into typed `ProviderOptions`.
//!
//! This module bridges the gap between the generic `request_options` HashMap
//! (carried from `ModelInfo` → `InferenceContext`) and the typed
//! `ProviderOptions` structs in hyper-sdk.
//!
//! Known keys are mapped to typed fields per provider; unknown keys go to the
//! catchall `extra` HashMap on each provider's options struct.
//!
//! # Merge Priority
//!
//! Thinking-derived values (from `thinking_convert`) take precedence over
//! request_options — existing typed fields are NOT overwritten.

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
/// If `existing` already contains options (e.g., from thinking config),
/// typed fields that are already set are NOT overwritten.
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

    let mut extra = HashMap::new();

    for (key, value) in request_options {
        match key.as_str() {
            "seed" => {
                if opts.seed.is_none() {
                    opts.seed = value.as_i64();
                }
            }
            "response_format" => {
                if opts.response_format.is_none() {
                    opts.response_format = value.as_str().map(String::from);
                }
            }
            "previous_response_id" => {
                if opts.previous_response_id.is_none() {
                    opts.previous_response_id = value.as_str().map(String::from);
                }
            }
            _ => {
                extra.insert(key.clone(), value.clone());
            }
        }
    }

    if !extra.is_empty() {
        opts.extra = extra;
    }

    opts.boxed()
}

fn merge_anthropic(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
) -> ProviderOptions {
    let mut opts = existing
        .and_then(|e| downcast_options::<AnthropicOptions>(&e).cloned())
        .unwrap_or_default();

    let mut extra = HashMap::new();

    for (key, value) in request_options {
        match key.as_str() {
            "cache_control" => {
                // Only map "ephemeral" value
                if opts.cache_control.is_none() {
                    if let Some(s) = value.as_str() {
                        if s == "ephemeral" {
                            opts.cache_control =
                                Some(hyper_sdk::options::anthropic::CacheControl::Ephemeral);
                        }
                    }
                }
            }
            _ => {
                extra.insert(key.clone(), value.clone());
            }
        }
    }

    if !extra.is_empty() {
        opts.extra = extra;
    }

    opts.boxed()
}

fn merge_gemini(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
) -> ProviderOptions {
    let mut opts = existing
        .and_then(|e| downcast_options::<GeminiOptions>(&e).cloned())
        .unwrap_or_default();

    let mut extra = HashMap::new();

    for (key, value) in request_options {
        match key.as_str() {
            "grounding" => {
                if opts.grounding.is_none() {
                    opts.grounding = value.as_bool();
                }
            }
            _ => {
                extra.insert(key.clone(), value.clone());
            }
        }
    }

    if !extra.is_empty() {
        opts.extra = extra;
    }

    opts.boxed()
}

fn merge_volcengine(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
) -> ProviderOptions {
    let mut opts = existing
        .and_then(|e| downcast_options::<VolcengineOptions>(&e).cloned())
        .unwrap_or_default();

    let mut extra = HashMap::new();

    for (key, value) in request_options {
        match key.as_str() {
            "previous_response_id" => {
                if opts.previous_response_id.is_none() {
                    opts.previous_response_id = value.as_str().map(String::from);
                }
            }
            "caching_enabled" => {
                if opts.caching_enabled.is_none() {
                    opts.caching_enabled = value.as_bool();
                }
            }
            _ => {
                extra.insert(key.clone(), value.clone());
            }
        }
    }

    if !extra.is_empty() {
        opts.extra = extra;
    }

    opts.boxed()
}

fn merge_zai(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
) -> ProviderOptions {
    let mut opts = existing
        .and_then(|e| downcast_options::<ZaiOptions>(&e).cloned())
        .unwrap_or_default();

    let mut extra = HashMap::new();

    for (key, value) in request_options {
        match key.as_str() {
            "do_sample" => {
                if opts.do_sample.is_none() {
                    opts.do_sample = value.as_bool();
                }
            }
            "request_id" => {
                if opts.request_id.is_none() {
                    opts.request_id = value.as_str().map(String::from);
                }
            }
            "user_id" => {
                if opts.user_id.is_none() {
                    opts.user_id = value.as_str().map(String::from);
                }
            }
            _ => {
                extra.insert(key.clone(), value.clone());
            }
        }
    }

    if !extra.is_empty() {
        opts.extra = extra;
    }

    opts.boxed()
}

#[cfg(test)]
#[path = "request_options_merge.test.rs"]
mod tests;
