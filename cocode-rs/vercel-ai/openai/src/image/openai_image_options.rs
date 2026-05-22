use std::collections::HashMap;

use serde::Deserialize;

/// Provider-specific options for OpenAI image models.
///
/// Known fields are typed explicitly; all other provider options (background,
/// output_format, output_compression, input_fidelity, partial_images, stream,
/// response_format, etc.) are captured via the `extra` map so they can be
/// forwarded to the API body as-is.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIImageProviderOptions {
    pub quality: Option<String>,
    pub style: Option<String>,
    pub size: Option<String>,
    pub user: Option<String>,
    /// All other unknown provider options, forwarded to the API body.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Extract the raw OpenAI provider options as a `serde_json::Value::Object`.
///
/// This returns the entire `providerOptions.openai` value so callers can
/// spread it into the request body, matching the TS `...providerOptions.openai`
/// pattern.
pub fn extract_raw_image_options(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    provider_options
        .as_ref()
        .and_then(|opts| opts.0.get("openai"))
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| match v {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
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

/// Get the maximum number of images per call for a given model.
///
/// Matches TS `openai-image-options.ts`: explicit lookup for known models,
/// default `1` for everything else (via `?? 1`).
pub fn model_max_images_per_call(model_id: &str) -> usize {
    match model_id {
        "dall-e-2"
        | "gpt-image-1"
        | "gpt-image-1-mini"
        | "gpt-image-1.5"
        | "chatgpt-image-latest" => 10,
        _ => 1,
    }
}

#[cfg(test)]
#[path = "openai_image_options.test.rs"]
mod tests;

/// Check if a model has a default response format (doesn't need explicit `b64_json`).
///
/// Models matching these prefixes return base64-encoded images by default.
pub fn has_default_response_format(model_id: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "chatgpt-image-",
        "gpt-image-1-mini",
        "gpt-image-1.5",
        "gpt-image-1",
    ];
    PREFIXES.iter().any(|prefix| model_id.starts_with(prefix))
}
