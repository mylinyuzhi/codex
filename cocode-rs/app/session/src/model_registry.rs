//! Model factory for creating models from ConfigManager.
//!
//! This module provides [`ConfigModelFactory`] which implements [`ModelFactory`]
//! for creating models using configuration from `ConfigManager`.
//!
//! # Migration Note
//!
//! For new code, prefer using [`cocode_api::ModelResolver`] directly, which
//! provides provider caching and takes selections as a parameter instead of
//! creating new providers on each model creation.
//!
//! `ConfigModelFactory` is still useful when:
//! - You need to use `MultiModel` with the `ModelFactory` trait pattern
//! - You're integrating with code that expects `ModelFactory`
//!
//! # Example
//!
//! ```ignore
//! use cocode_api::MultiModel;
//! use cocode_session::ConfigModelFactory;
//! use cocode_protocol::model::ModelRole;
//!
//! let factory = ConfigModelFactory::new(config);
//! let multi_model = Arc::new(MultiModel::new(factory));
//!
//! multi_model.set_selection(ModelRole::Main, "anthropic/claude-opus-4");
//! let model = multi_model.get(ModelRole::Main)?;
//! ```

use std::sync::Arc;

use cocode_api::ModelFactory;
use cocode_config::ConfigManager;
use cocode_protocol::ProviderType;
use cocode_protocol::model::ModelRole;
use hyper_sdk::Model;
use hyper_sdk::Provider;
use tracing::info;

/// Factory for creating model instances from ConfigManager.
///
/// This factory implements [`ModelFactory`] and is designed to be used with
/// [`cocode_api::MultiModel`] for role-based model resolution.
///
/// # Note
///
/// This factory creates a new provider instance for each model creation.
/// For better performance with provider caching, consider using
/// [`cocode_api::ModelResolver`] directly.
///
/// # Example
///
/// ```ignore
/// use cocode_api::MultiModel;
/// use cocode_session::ConfigModelFactory;
/// use cocode_protocol::model::ModelRole;
///
/// let factory = ConfigModelFactory::new(config);
/// let multi_model = Arc::new(MultiModel::new(factory));
///
/// // Set selections
/// multi_model.set_selection(ModelRole::Main, "anthropic/claude-opus-4");
/// multi_model.set_selection(ModelRole::Fast, "anthropic/claude-haiku");
///
/// // Get models
/// let main = multi_model.get(ModelRole::Main)?;
/// let fast = multi_model.get(ModelRole::Fast)?;
/// ```
pub struct ConfigModelFactory {
    config: Arc<ConfigManager>,
}

impl ConfigModelFactory {
    /// Create a new ConfigModelFactory.
    pub fn new(config: Arc<ConfigManager>) -> Self {
        Self { config }
    }

    /// Parse a selection key into provider and model name.
    ///
    /// Keys are in the format "provider/model" (e.g., "anthropic/claude-opus-4").
    fn parse_key(key: &str) -> anyhow::Result<(&str, &str)> {
        let parts: Vec<&str> = key.splitn(2, '/').collect();
        if parts.len() != 2 {
            anyhow::bail!(
                "Invalid selection key '{}': expected format 'provider/model'",
                key
            );
        }
        Ok((parts[0], parts[1]))
    }

    /// Create a model from provider info and model name.
    fn create_from_provider_info(
        provider_info: &cocode_protocol::ProviderInfo,
        model_name: &str,
    ) -> anyhow::Result<(Arc<dyn Model>, ProviderType)> {
        use hyper_sdk::providers::anthropic::AnthropicConfig;
        use hyper_sdk::providers::gemini::GeminiConfig;
        use hyper_sdk::providers::openai::OpenAIConfig;
        use hyper_sdk::providers::volcengine::VolcengineConfig;
        use hyper_sdk::providers::zai::ZaiConfig;

        // Get the actual model name to use for API
        let api_model_name = provider_info
            .get_model(model_name)
            .map(|m| m.api_model_name())
            .unwrap_or(model_name);

        // Create provider-specific model
        let model: Arc<dyn Model> = match provider_info.provider_type {
            ProviderType::Openai | ProviderType::OpenaiCompat => {
                let config = OpenAIConfig {
                    api_key: provider_info.api_key.clone(),
                    base_url: provider_info.base_url.clone(),
                    ..Default::default()
                };
                let provider = hyper_sdk::OpenAIProvider::new(config)
                    .map_err(|e| anyhow::anyhow!("Failed to create OpenAI provider: {e}"))?;
                provider
                    .model(api_model_name)
                    .map_err(|e| anyhow::anyhow!("Failed to create model: {e}"))?
            }
            ProviderType::Anthropic => {
                let config = AnthropicConfig {
                    api_key: provider_info.api_key.clone(),
                    base_url: provider_info.base_url.clone(),
                    ..Default::default()
                };
                let provider = hyper_sdk::AnthropicProvider::new(config)
                    .map_err(|e| anyhow::anyhow!("Failed to create Anthropic provider: {e}"))?;
                provider
                    .model(api_model_name)
                    .map_err(|e| anyhow::anyhow!("Failed to create model: {e}"))?
            }
            ProviderType::Gemini => {
                let config = GeminiConfig {
                    api_key: provider_info.api_key.clone(),
                    base_url: provider_info.base_url.clone(),
                    ..Default::default()
                };
                let provider = hyper_sdk::GeminiProvider::new(config)
                    .map_err(|e| anyhow::anyhow!("Failed to create Gemini provider: {e}"))?;
                provider
                    .model(api_model_name)
                    .map_err(|e| anyhow::anyhow!("Failed to create model: {e}"))?
            }
            ProviderType::Volcengine => {
                let config = VolcengineConfig {
                    api_key: provider_info.api_key.clone(),
                    base_url: provider_info.base_url.clone(),
                    ..Default::default()
                };
                let provider = hyper_sdk::VolcengineProvider::new(config)
                    .map_err(|e| anyhow::anyhow!("Failed to create Volcengine provider: {e}"))?;
                provider
                    .model(api_model_name)
                    .map_err(|e| anyhow::anyhow!("Failed to create model: {e}"))?
            }
            ProviderType::Zai => {
                let config = ZaiConfig {
                    api_key: provider_info.api_key.clone(),
                    base_url: provider_info.base_url.clone(),
                    ..Default::default()
                };
                let provider = hyper_sdk::ZaiProvider::new(config)
                    .map_err(|e| anyhow::anyhow!("Failed to create Z.AI provider: {e}"))?;
                provider
                    .model(api_model_name)
                    .map_err(|e| anyhow::anyhow!("Failed to create model: {e}"))?
            }
        };

        Ok((model, provider_info.provider_type))
    }
}

impl ModelFactory for ConfigModelFactory {
    fn create(&self, role: ModelRole, key: &str) -> anyhow::Result<(Arc<dyn Model>, ProviderType)> {
        let (provider_name, model_name) = Self::parse_key(key)?;

        info!(
            role = %role,
            provider = %provider_name,
            model = %model_name,
            "Creating model for role"
        );

        // Resolve provider info
        let provider_info = self.config.resolve_provider(provider_name)?;

        Self::create_from_provider_info(&provider_info, model_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_valid() {
        let (provider, model) = ConfigModelFactory::parse_key("anthropic/claude-opus-4").unwrap();
        assert_eq!(provider, "anthropic");
        assert_eq!(model, "claude-opus-4");
    }

    #[test]
    fn test_parse_key_with_slash_in_model() {
        let (provider, model) = ConfigModelFactory::parse_key("openai/gpt-4/turbo").unwrap();
        assert_eq!(provider, "openai");
        assert_eq!(model, "gpt-4/turbo");
    }

    #[test]
    fn test_parse_key_invalid() {
        let result = ConfigModelFactory::parse_key("no-slash");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid selection key")
        );
    }
}
