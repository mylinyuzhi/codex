//! Factory for creating vercel-ai providers from protocol config.
//!
//! Bridges cocode-protocol's ProviderInfo to vercel-ai providers.
//!
//! Provider-specific options can be set via `ProviderInfo.options`:
//! - `auth_token` (string): Bearer token auth for Anthropic gateways
//! - `full_url` (bool): Treat base_url as complete endpoint URL, skip path suffix
//! - `headers` (object): Custom HTTP headers (e.g., `User-Agent`)
//! - `include_usage` (bool): Include usage in streaming for OpenAI-compatible

use crate::LanguageModel;
use crate::Provider;
use crate::error::Result;
use cocode_protocol::ProviderApi;
use cocode_protocol::ProviderInfo;
use cocode_protocol::WireApi;
use std::collections::HashMap;
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

/// Extract custom headers from `ProviderInfo.options.headers`.
fn extract_headers(info: &ProviderInfo) -> Option<HashMap<String, String>> {
    let headers_val = info.options.as_ref()?.get("headers")?;
    let map = headers_val.as_object()?;
    let headers: HashMap<String, String> = map
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect();
    if headers.is_empty() {
        None
    } else {
        Some(headers)
    }
}

/// Extract a string option from `ProviderInfo.options`.
fn extract_opt_str(info: &ProviderInfo, key: &str) -> Option<String> {
    info.options
        .as_ref()?
        .get(key)?
        .as_str()
        .map(str::to_string)
}

/// Extract a bool option from `ProviderInfo.options`.
fn extract_opt_bool(info: &ProviderInfo, key: &str) -> Option<bool> {
    info.options.as_ref()?.get(key)?.as_bool()
}

/// Build OpenAI provider settings with organization ID handling.
fn create_openai_settings(
    info: &ProviderInfo,
    client: Arc<reqwest::Client>,
    headers: Option<HashMap<String, String>>,
) -> vercel_ai_openai::OpenAIProviderSettings {
    let mut settings = vercel_ai_openai::OpenAIProviderSettings {
        api_key: Some(info.api_key.clone()),
        base_url: Some(info.base_url.clone()),
        client: Some(client),
        headers,
        ..Default::default()
    };
    if let Some(org_id) = extract_opt_str(info, "organization_id") {
        settings.organization = Some(org_id);
    }
    settings.full_url = extract_opt_bool(info, "full_url");
    settings
}

/// Build an OpenAI-compatible provider with the given name.
fn create_openai_compat_provider(
    info: &ProviderInfo,
    name: String,
    client: Arc<reqwest::Client>,
    headers: Option<HashMap<String, String>>,
) -> Arc<dyn Provider> {
    let settings = vercel_ai_openai_compatible::OpenAICompatibleProviderSettings {
        api_key: Some(info.api_key.clone()),
        base_url: Some(info.base_url.clone()),
        name: Some(name),
        client: Some(client),
        headers,
        include_usage: extract_opt_bool(info, "include_usage").or(Some(true)),
        full_url: extract_opt_bool(info, "full_url"),
        ..Default::default()
    };
    Arc::new(vercel_ai_openai_compatible::OpenAICompatibleProvider::new(
        settings,
    ))
}

/// Create a provider from ProviderInfo configuration.
pub fn create_provider(info: &ProviderInfo) -> Result<Arc<dyn Provider>> {
    let client = build_http_client(info.timeout_secs);
    let headers = extract_headers(info);

    let provider: Arc<dyn Provider> = match info.api {
        ProviderApi::Openai => {
            let settings = create_openai_settings(info, client, headers);
            Arc::new(vercel_ai_openai::OpenAIProvider::new(settings))
        }
        ProviderApi::Anthropic => {
            let auth_token = extract_opt_str(info, "auth_token");
            let settings = vercel_ai_anthropic::AnthropicProviderSettings {
                // Use auth_token if set; otherwise fall back to api_key
                api_key: if auth_token.is_some() {
                    None
                } else {
                    Some(info.api_key.clone())
                },
                auth_token,
                base_url: Some(info.base_url.clone()),
                client: Some(client),
                headers,
                full_url: extract_opt_bool(info, "full_url"),
                ..Default::default()
            };
            Arc::new(vercel_ai_anthropic::AnthropicProvider::new(settings))
        }
        ProviderApi::Gemini => {
            let settings = vercel_ai_google::GoogleGenerativeAIProviderSettings {
                api_key: Some(info.api_key.clone()),
                base_url: Some(info.base_url.clone()),
                headers,
                ..Default::default()
            };
            Arc::new(vercel_ai_google::create_google_generative_ai(settings))
        }
        ProviderApi::Volcengine => {
            create_openai_compat_provider(info, "volcengine".into(), client, headers)
        }
        ProviderApi::Zai => create_openai_compat_provider(info, "zai".into(), client, headers),
        ProviderApi::OpenaiCompat => {
            create_openai_compat_provider(info, info.name.clone(), client, headers)
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
    if info.api == ProviderApi::Openai && info.wire_api == WireApi::Chat {
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
    let headers = extract_headers(info);
    let settings = create_openai_settings(info, client, headers);
    let provider = vercel_ai_openai::OpenAIProvider::new(settings);
    Ok(Arc::new(provider.chat(api_name)))
}

#[cfg(test)]
#[path = "provider_factory.test.rs"]
mod tests;
