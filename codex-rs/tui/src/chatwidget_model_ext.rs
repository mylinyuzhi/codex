//! Extension functions for model popup to support custom providers.

use codex_core::config::Config;
use codex_core::models_manager::provider_preset::provider_to_preset;
use codex_protocol::openai_models::ModelPreset;

/// Built-in provider IDs that should not appear in the custom providers list.
const BUILTIN_PROVIDER_IDS: &[&str] = &["openai", "ollama", "lmstudio"];

/// Get custom provider presets from Config.model_providers.
///
/// This filters out the built-in providers (openai, ollama, lmstudio) and
/// converts remaining user-defined providers to ModelPreset format.
///
/// Note: This function ensures `derive_model_family()` is called on each provider
/// to populate `model_family` before conversion, enabling proper `default_reasoning_effort`
/// resolution.
pub fn get_custom_provider_presets(config: &Config) -> Vec<ModelPreset> {
    config
        .model_providers
        .iter()
        .filter(|(id, _)| !BUILTIN_PROVIDER_IDS.contains(&id.as_str()))
        .map(|(id, provider)| {
            // Ensure model_family is derived for default_reasoning_effort resolution
            let mut provider = provider.clone();
            provider.ext.derive_model_family();
            provider_to_preset(id, &provider)
        })
        .collect()
}

/// Extract provider_id from a preset ID in "providername/model" format.
pub fn extract_provider_id(preset_id: &str) -> Option<String> {
    preset_id.split('/').next().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_provider_id() {
        assert_eq!(
            extract_provider_id("deepseek/deepseek-r1"),
            Some("deepseek".to_string())
        );
        assert_eq!(
            extract_provider_id("azure/gpt-4-turbo"),
            Some("azure".to_string())
        );
        assert_eq!(
            extract_provider_id("no_slash"),
            Some("no_slash".to_string())
        );
    }
}
