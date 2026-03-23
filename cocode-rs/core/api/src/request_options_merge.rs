//! Merge model `request_options` into vercel-ai `ProviderOptions`.
//!
//! All keys from the generic `request_options` HashMap are merged into the
//! provider-specific entry in the `ProviderOptions` HashMap.

use crate::ProviderOptions;
use cocode_protocol::ProviderApi;
use std::collections::HashMap;

/// Merge `request_options` into existing (or new) `ProviderOptions`.
///
/// Keys from `request_options` are placed into the provider-specific
/// section of the ProviderOptions HashMap.
pub fn merge_into_provider_options(
    existing: Option<ProviderOptions>,
    request_options: &HashMap<String, serde_json::Value>,
    provider: ProviderApi,
) -> ProviderOptions {
    let provider_name = provider_name_for_type(provider);
    let mut opts = existing.unwrap_or_default();

    // Get or create the provider-specific entry
    let entry = opts.0.entry(provider_name.to_string()).or_default();

    // Merge all request_options into the provider entry
    for (k, v) in request_options {
        entry.insert(k.clone(), v.clone());
    }

    opts
}

/// Map ProviderApi to the provider name key used in ProviderOptions.
pub fn provider_name_for_type(provider: ProviderApi) -> &'static str {
    match provider {
        ProviderApi::Openai | ProviderApi::OpenaiCompat => "openai",
        ProviderApi::Anthropic => "anthropic",
        ProviderApi::Gemini => "google",
        ProviderApi::Volcengine => "volcengine",
        ProviderApi::Zai => "zai",
    }
}

/// Generate provider-specific base options that should be injected as Step 0.
///
/// These are defaults that all requests to a given provider should include
/// unless overridden by later steps (thinking_convert, request_options, interceptors).
///
/// - OpenAI: `store: false` (prevent training data use)
/// - Gemini: `thinkingConfig: { includeThoughts: true }` (thought visibility)
pub fn provider_base_options(provider: ProviderApi) -> Option<ProviderOptions> {
    let provider_name = provider_name_for_type(provider);
    let mut opts = HashMap::new();
    match provider {
        ProviderApi::Openai | ProviderApi::OpenaiCompat => {
            opts.insert("store".into(), serde_json::json!(false));
        }
        ProviderApi::Gemini => {
            opts.insert(
                "thinkingConfig".into(),
                serde_json::json!({"includeThoughts": true}),
            );
        }
        _ => return None,
    }
    Some(build_options(provider_name, opts))
}

/// Build a ProviderOptions with a single provider entry.
pub(crate) fn build_options(
    provider_name: &str,
    opts: HashMap<String, serde_json::Value>,
) -> ProviderOptions {
    let mut map = HashMap::new();
    map.insert(provider_name.to_string(), opts);
    ProviderOptions::from_map(map)
}

/// Merge two ProviderOptions together. Values from `override_opts` take precedence.
pub fn merge_provider_options(
    base: Option<ProviderOptions>,
    override_opts: Option<ProviderOptions>,
) -> Option<ProviderOptions> {
    match (base, override_opts) {
        (None, None) => None,
        (Some(b), None) => Some(b),
        (None, Some(o)) => Some(o),
        (Some(mut b), Some(o)) => {
            for (provider, opts) in o.0 {
                let entry = b.0.entry(provider).or_insert_with(HashMap::new);
                entry.extend(opts);
            }
            Some(b)
        }
    }
}

#[cfg(test)]
#[path = "request_options_merge.test.rs"]
mod tests;
