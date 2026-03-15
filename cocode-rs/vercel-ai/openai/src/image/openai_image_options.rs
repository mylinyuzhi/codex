use serde::Deserialize;

/// Provider-specific options for OpenAI image models.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIImageProviderOptions {
    pub quality: Option<String>,
    pub style: Option<String>,
    pub size: Option<String>,
    pub user: Option<String>,
}

/// Extract image-specific options from provider options.
pub fn extract_image_options(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
) -> OpenAIImageProviderOptions {
    provider_options
        .as_ref()
        .and_then(|opts| opts.0.get("openai"))
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| serde_json::from_value::<OpenAIImageProviderOptions>(v).ok())
        .unwrap_or_default()
}
