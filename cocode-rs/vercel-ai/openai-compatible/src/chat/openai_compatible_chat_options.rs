use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// Provider-specific options for OpenAI-compatible Chat models.
///
/// Only includes the 4 fields defined in the openai-compatible schema.
/// All other provider-specific keys are passed through as-is into the request body.
///
/// Extracted from `options.provider_options[provider_name]` (with fallbacks).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAICompatibleChatProviderOptions {
    pub user: Option<String>,
    /// Reasoning effort level as a string (e.g., "low", "medium", "high").
    pub reasoning_effort: Option<String>,
    /// Text verbosity level as a string (e.g., "low", "medium", "high").
    pub text_verbosity: Option<String>,
    /// Defaults to true when response_format is json_schema.
    pub strict_json_schema: Option<bool>,
}

/// Known schema keys that should NOT be passed through into the body.
const SCHEMA_KEYS: &[&str] = &[
    "user",
    "reasoningEffort",
    "textVerbosity",
    "strictJsonSchema",
];

/// Extract provider-specific options from the generic provider options map,
/// with fallback key resolution: `providerOptionsName` → `openaiCompatible`.
///
/// Returns `(typed_options, passthrough_map)` where `passthrough_map` contains
/// any keys not in the schema that should be spread into the request body.
pub fn extract_compatible_options(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
    provider_name: &str,
) -> (OpenAICompatibleChatProviderOptions, HashMap<String, Value>) {
    let Some(opts) = provider_options.as_ref() else {
        return (
            OpenAICompatibleChatProviderOptions::default(),
            HashMap::new(),
        );
    };

    // Resolve raw value with precedence: providerOptionsName > openaiCompatible
    // (TS also checks deprecated "openai-compatible" but Rust omits hyphenated keys)
    let raw = opts
        .0
        .get(provider_name)
        .or_else(|| opts.0.get("openaiCompatible"));

    let Some(raw) = raw else {
        return (
            OpenAICompatibleChatProviderOptions::default(),
            HashMap::new(),
        );
    };

    let value = match serde_json::to_value(raw) {
        Ok(v) => v,
        Err(_) => {
            return (
                OpenAICompatibleChatProviderOptions::default(),
                HashMap::new(),
            );
        }
    };

    let typed: OpenAICompatibleChatProviderOptions =
        serde_json::from_value(value.clone()).unwrap_or_default();

    // Build passthrough map: all keys NOT in the schema
    let mut passthrough = HashMap::new();
    if let Value::Object(map) = &value {
        for (k, v) in map {
            if !SCHEMA_KEYS.contains(&k.as_str()) {
                passthrough.insert(k.clone(), v.clone());
            }
        }
    }

    (typed, passthrough)
}
