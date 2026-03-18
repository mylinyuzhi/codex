//! Factory for creating vercel-ai providers from protocol config.
//!
//! Bridges cocode-protocol's ProviderInfo to vercel-ai providers.

use crate::LanguageModel;
use crate::Provider;
use crate::error::Result;
use cocode_protocol::ProviderInfo;
use cocode_protocol::ProviderType;
use cocode_protocol::WireApi;
use std::sync::Arc;
use std::time::Duration;

/// Build a `reqwest::Client` with the provider's configured timeout.
fn build_http_client(timeout_secs: i64) -> Arc<reqwest::Client> {
    Arc::new(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs as u64))
            .build()
            .unwrap_or_default(),
    )
}

/// Create a provider from ProviderInfo configuration.
pub fn create_provider(info: &ProviderInfo) -> Result<Arc<dyn Provider>> {
    let client = build_http_client(info.timeout_secs);

    let provider: Arc<dyn Provider> = match info.provider_type {
        ProviderType::Openai => {
            let mut settings = vercel_ai_openai::OpenAIProviderSettings {
                api_key: Some(info.api_key.clone()),
                base_url: Some(info.base_url.clone()),
                client: Some(client),
                ..Default::default()
            };

            // Handle organization ID from provider options
            if let Some(options) = &info.options
                && let Some(org_id) = options.get("organization_id").and_then(|v| v.as_str())
            {
                settings.organization = Some(org_id.to_string());
            }

            Arc::new(vercel_ai_openai::OpenAIProvider::new(settings))
        }
        ProviderType::Anthropic => {
            let settings = vercel_ai_anthropic::AnthropicProviderSettings {
                api_key: Some(info.api_key.clone()),
                base_url: Some(info.base_url.clone()),
                client: Some(client),
                ..Default::default()
            };
            Arc::new(vercel_ai_anthropic::AnthropicProvider::new(settings))
        }
        ProviderType::Gemini => {
            let settings = vercel_ai_google::GoogleGenerativeAIProviderSettings {
                api_key: Some(info.api_key.clone()),
                base_url: Some(info.base_url.clone()),
                ..Default::default()
            };
            Arc::new(vercel_ai_google::create_google_generative_ai(settings))
        }
        ProviderType::Volcengine => {
            let settings = vercel_ai_openai_compatible::OpenAICompatibleProviderSettings {
                api_key: Some(info.api_key.clone()),
                base_url: Some(info.base_url.clone()),
                name: Some("volcengine".into()),
                client: Some(client),
                ..Default::default()
            };
            Arc::new(vercel_ai_openai_compatible::OpenAICompatibleProvider::new(
                settings,
            ))
        }
        ProviderType::Zai => {
            let settings = vercel_ai_openai_compatible::OpenAICompatibleProviderSettings {
                api_key: Some(info.api_key.clone()),
                base_url: Some(info.base_url.clone()),
                name: Some("zai".into()),
                client: Some(client),
                ..Default::default()
            };
            Arc::new(vercel_ai_openai_compatible::OpenAICompatibleProvider::new(
                settings,
            ))
        }
        ProviderType::OpenaiCompat => {
            let settings = vercel_ai_openai_compatible::OpenAICompatibleProviderSettings {
                api_key: Some(info.api_key.clone()),
                base_url: Some(info.base_url.clone()),
                name: Some(info.name.clone()),
                client: Some(client),
                ..Default::default()
            };
            Arc::new(vercel_ai_openai_compatible::OpenAICompatibleProvider::new(
                settings,
            ))
        }
    };
    Ok(provider)
}

/// Create a model from ProviderInfo for a specific model slug.
pub fn create_model(info: &ProviderInfo, model_slug: &str) -> Result<Arc<dyn LanguageModel>> {
    // Get the API model name (handles aliases like endpoint IDs for Volcengine)
    let api_name = info.api_model_name(model_slug).unwrap_or(model_slug);

    // P25: For OpenAI, respect wire_api to select Responses vs Chat Completions API.
    // The ProviderV4::language_model() trait method always defaults to Responses,
    // so we need to create the model directly for Chat.
    if info.provider_type == ProviderType::Openai && info.wire_api == WireApi::Chat {
        return create_openai_chat_model(info, api_name);
    }

    let provider = create_provider(info)?;

    provider.language_model(api_name).map_err(|e| {
        crate::error::api_error::SdkSnafu {
            message: e.to_string(),
        }
        .build()
    })
}

/// Create an OpenAI model using the Chat Completions API.
fn create_openai_chat_model(info: &ProviderInfo, api_name: &str) -> Result<Arc<dyn LanguageModel>> {
    let client = build_http_client(info.timeout_secs);
    let mut settings = vercel_ai_openai::OpenAIProviderSettings {
        api_key: Some(info.api_key.clone()),
        base_url: Some(info.base_url.clone()),
        client: Some(client),
        ..Default::default()
    };

    if let Some(options) = &info.options
        && let Some(org_id) = options.get("organization_id").and_then(|v| v.as_str())
    {
        settings.organization = Some(org_id.to_string());
    }

    let provider = vercel_ai_openai::OpenAIProvider::new(settings);
    Ok(Arc::new(provider.chat(api_name)))
}

#[cfg(test)]
#[path = "provider_factory.test.rs"]
mod tests;
