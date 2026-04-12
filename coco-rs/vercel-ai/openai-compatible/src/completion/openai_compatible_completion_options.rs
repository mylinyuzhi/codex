use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// Provider-specific options for OpenAI-compatible completion models.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAICompatibleCompletionProviderOptions {
    pub echo: Option<bool>,
    pub logit_bias: Option<HashMap<String, f64>>,
    pub suffix: Option<String>,
    pub user: Option<String>,
}

/// Known schema keys for completion options (used to filter passthrough).
const SCHEMA_KEYS: &[&str] = &["echo", "logitBias", "suffix", "user"];

/// Extract completion-specific options and passthrough keys from provider options.
pub fn extract_completion_options(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
    provider_name: &str,
) -> (
    OpenAICompatibleCompletionProviderOptions,
    HashMap<String, Value>,
) {
    let raw = provider_options.as_ref().and_then(|opts| {
        opts.0
            .get(provider_name)
            .or_else(|| opts.0.get("openaiCompatible"))
    });

    let typed = raw
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| serde_json::from_value::<OpenAICompatibleCompletionProviderOptions>(v).ok())
        .unwrap_or_default();

    // Collect passthrough keys (everything not in the schema)
    let passthrough: HashMap<String, Value> = raw
        .map(|inner| {
            inner
                .iter()
                .filter(|(k, _)| !SCHEMA_KEYS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();

    (typed, passthrough)
}
