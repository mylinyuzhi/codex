use serde::Deserialize;
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

/// Extract completion-specific options from provider options.
pub fn extract_completion_options(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
    provider_name: &str,
) -> OpenAICompatibleCompletionProviderOptions {
    provider_options
        .as_ref()
        .and_then(|opts| opts.0.get(provider_name))
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| serde_json::from_value::<OpenAICompatibleCompletionProviderOptions>(v).ok())
        .unwrap_or_default()
}
