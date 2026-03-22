use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// Provider-specific options for OpenAI-compatible image models.
///
/// Only includes the 4 fields defined in the openai-compatible schema.
/// All other provider-specific keys are passed through as-is into the request body.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAICompatibleImageProviderOptions {
    pub quality: Option<String>,
    pub style: Option<String>,
    pub size: Option<String>,
    pub user: Option<String>,
}

/// Known schema keys that should NOT be passed through into the body.
const SCHEMA_KEYS: &[&str] = &["quality", "style", "size", "user"];

/// Extract image-specific options from provider options,
/// with fallback key resolution: `providerOptionsName` → `openaiCompatible`.
///
/// Returns `(typed_options, passthrough_map)` where `passthrough_map` contains
/// any keys not in the schema that should be spread into the request body.
pub fn extract_image_options(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
    provider_name: &str,
) -> (OpenAICompatibleImageProviderOptions, HashMap<String, Value>) {
    let Some(opts) = provider_options.as_ref() else {
        return (
            OpenAICompatibleImageProviderOptions::default(),
            HashMap::new(),
        );
    };

    // Resolve raw value with precedence: providerOptionsName > openaiCompatible
    let raw = opts
        .0
        .get(provider_name)
        .or_else(|| opts.0.get("openaiCompatible"));

    let Some(raw) = raw else {
        return (
            OpenAICompatibleImageProviderOptions::default(),
            HashMap::new(),
        );
    };

    let value = match serde_json::to_value(raw) {
        Ok(v) => v,
        Err(_) => {
            return (
                OpenAICompatibleImageProviderOptions::default(),
                HashMap::new(),
            );
        }
    };

    let typed: OpenAICompatibleImageProviderOptions =
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
