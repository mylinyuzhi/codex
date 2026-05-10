use std::collections::BTreeMap;

use coco_types::ProviderApi;

use crate::EnvKey;
use crate::error::ConfigError;

use super::PartialProviderConfig;
use super::PartialProviderModelOverride;
use super::ProviderConfig;

/// Built-in provider partial overlays. Identity invariant: built
/// through [`ProviderConfig::from_partial`] so `name` is set in
/// exactly one code path — the same as user-supplied entries.
pub fn builtin_provider_partials() -> Vec<(&'static str, PartialProviderConfig)> {
    vec![
        (
            "anthropic",
            PartialProviderConfig {
                api: Some(ProviderApi::Anthropic),
                env_key: Some(EnvKey::AnthropicApiKey.to_string()),
                // **Must end with `/v1`.** `AnthropicConfig::url(path)` only
                // appends `path` (e.g. `/messages`) when `base_url` does not
                // already end with `path`; it does NOT auto-detect missing
                // version segments. So `https://api.anthropic.com` would
                // produce `/messages` (404) instead of `/v1/messages`.
                base_url: Some("https://api.anthropic.com/v1".into()),
                ..Default::default()
            },
        ),
        (
            "openai",
            PartialProviderConfig {
                api: Some(ProviderApi::Openai),
                env_key: Some("OPENAI_API_KEY".into()),
                base_url: Some("https://api.openai.com/v1".into()),
                // OpenAI direct defaults to the Responses API (the
                // SDK's `language_model()` default). Users with
                // legacy Chat Completions deployments override via
                // `wire_api: "chat"` in providers.json.
                wire_api: Some(coco_types::WireApi::Responses),
                ..Default::default()
            },
        ),
        (
            "google",
            PartialProviderConfig {
                api: Some(ProviderApi::Gemini),
                env_key: Some("GOOGLE_API_KEY".into()),
                // **Must end with `/v1beta`.** Same reason as Anthropic — the
                // SDK appends `/models/<id>:generateContent` to `base_url`
                // without auto-detecting missing version segments.
                base_url: Some("https://generativelanguage.googleapis.com/v1beta".into()),
                ..Default::default()
            },
        ),
        (
            "volcengine",
            PartialProviderConfig {
                api: Some(ProviderApi::Volcengine),
                env_key: Some("ARK_API_KEY".into()),
                base_url: Some("https://ark.cn-beijing.volces.com/api/v3".into()),
                ..Default::default()
            },
        ),
        (
            "zai",
            PartialProviderConfig {
                api: Some(ProviderApi::Zai),
                env_key: Some("ZAI_API_KEY".into()),
                base_url: Some("https://api.z.ai/v1".into()),
                ..Default::default()
            },
        ),
        (
            "deepseek-openai",
            PartialProviderConfig {
                api: Some(ProviderApi::OpenaiCompat),
                env_key: Some(EnvKey::DeepseekApiKey.to_string()),
                // OpenAI-compatible endpoint — SDK appends `/chat/completions`.
                base_url: Some("https://api.deepseek.com/v1".into()),
                models: Some(deepseek_v4_models()),
                ..Default::default()
            },
        ),
        (
            "deepseek-anthropic",
            PartialProviderConfig {
                api: Some(ProviderApi::Anthropic),
                env_key: Some(EnvKey::DeepseekApiKey.to_string()),
                // Anthropic-compatible endpoint — must end with `/v1`; SDK
                // appends `/messages` (same rule as `api.anthropic.com/v1`).
                base_url: Some("https://api.deepseek.com/anthropic/v1".into()),
                models: Some(deepseek_v4_models()),
                ..Default::default()
            },
        ),
    ]
}

/// Pre-registered DeepSeek V4 model entries shared by both builtin
/// DeepSeek providers. Empty overrides — metadata comes from the
/// compiled-in `builtin_models_partial()` catalog.
fn deepseek_v4_models() -> BTreeMap<String, PartialProviderModelOverride> {
    BTreeMap::from([
        (
            "deepseek-v4-flash".into(),
            PartialProviderModelOverride::default(),
        ),
        (
            "deepseek-v4-pro".into(),
            PartialProviderModelOverride::default(),
        ),
    ])
}

/// Resolve every builtin partial through `from_partial` so the
/// "name = parent map key" invariant covers builtins as well as
/// user-supplied entries. Returns `ConfigError::IncompleteProviderEntry`
/// if a builtin partial is missing a required field — caught at
/// crate test time.
pub fn builtin_providers() -> Result<Vec<ProviderConfig>, ConfigError> {
    builtin_provider_partials()
        .into_iter()
        .map(|(name, partial)| ProviderConfig::from_partial(name, &partial))
        .collect()
}

#[cfg(test)]
#[path = "builtin.test.rs"]
mod tests;
