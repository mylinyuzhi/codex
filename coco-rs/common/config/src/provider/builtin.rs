use coco_types::ProviderApi;

use super::ProviderConfig;

/// Built-in providers with their env_key and base URL.
pub fn builtin_providers() -> Vec<ProviderConfig> {
    vec![
        ProviderConfig {
            name: "anthropic".into(),
            api: ProviderApi::Anthropic,
            env_key: "ANTHROPIC_API_KEY".into(),
            base_url: "https://api.anthropic.com".into(),
            default_model: Some("claude-sonnet-4-6-20250514".into()),
            ..Default::default()
        },
        ProviderConfig {
            name: "openai".into(),
            api: ProviderApi::Openai,
            env_key: "OPENAI_API_KEY".into(),
            base_url: "https://api.openai.com/v1".into(),
            default_model: Some("gpt-4o".into()),
            ..Default::default()
        },
        ProviderConfig {
            name: "google".into(),
            api: ProviderApi::Gemini,
            env_key: "GOOGLE_API_KEY".into(),
            base_url: "https://generativelanguage.googleapis.com".into(),
            default_model: Some("gemini-2.5-pro".into()),
            ..Default::default()
        },
        ProviderConfig {
            name: "volcengine".into(),
            api: ProviderApi::Volcengine,
            env_key: "ARK_API_KEY".into(),
            base_url: "https://ark.cn-beijing.volces.com/api/v3".into(),
            ..Default::default()
        },
        ProviderConfig {
            name: "zai".into(),
            api: ProviderApi::Zai,
            env_key: "ZAI_API_KEY".into(),
            base_url: "https://api.z.ai/v1".into(),
            ..Default::default()
        },
    ]
}

/// Find a built-in provider by API type.
pub fn find_builtin_provider(api: ProviderApi) -> Option<ProviderConfig> {
    builtin_providers().into_iter().find(|p| p.api == api)
}

#[cfg(test)]
#[path = "builtin.test.rs"]
mod tests;
