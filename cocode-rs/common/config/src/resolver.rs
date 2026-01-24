//! Configuration resolution and merging logic.
//!
//! This module implements the layered configuration resolution:
//!
//! **Precedence (highest to lowest):**
//! 1. Runtime overrides (API calls, `/model` command)
//! 2. Environment variables (for secrets)
//! 3. Provider-specific model override (`providers.json -> models -> model_info_override`)
//! 4. User model config (`models.json`)
//! 5. Built-in defaults (compiled into binary)

use crate::builtin;
use crate::capability::Capability;
use crate::error::ConfigError;
use crate::types::ModelInfoConfig;
use crate::types::ModelsFile;
use crate::types::ProfileConfig;
use crate::types::ProfilesFile;
use crate::types::ProviderJsonConfig;
use crate::types::ProvidersFile;
use crate::types::ResolvedModelInfo;
use crate::types::ResolvedProviderConfig;
use std::collections::HashMap;
use std::env;
use tracing::debug;

/// Configuration resolver that merges layers of configuration.
#[derive(Debug, Clone)]
pub struct ConfigResolver {
    pub(crate) models: HashMap<String, ModelInfoConfig>,
    pub(crate) providers: HashMap<String, ProviderJsonConfig>,
    pub(crate) profiles: HashMap<String, ProfileConfig>,
    pub(crate) default_profile: Option<String>,
}

impl ConfigResolver {
    /// Create a new resolver from loaded configuration.
    pub fn new(
        models_file: ModelsFile,
        providers_file: ProvidersFile,
        profiles_file: ProfilesFile,
    ) -> Self {
        Self {
            models: models_file.models,
            providers: providers_file.providers,
            profiles: profiles_file.profiles,
            default_profile: profiles_file.default_profile,
        }
    }

    /// Create an empty resolver (uses only built-in defaults).
    pub fn empty() -> Self {
        Self {
            models: HashMap::new(),
            providers: HashMap::new(),
            profiles: HashMap::new(),
            default_profile: None,
        }
    }

    /// Resolve model info by merging all configuration layers.
    ///
    /// Merges in order (later overrides earlier):
    /// 1. Built-in defaults
    /// 2. User model config (models.json)
    /// 3. Provider-specific override (providers.json -> models -> model_info_override)
    pub fn resolve_model_info(
        &self,
        provider_name: &str,
        model_id: &str,
    ) -> Result<ResolvedModelInfo, ConfigError> {
        // Start with built-in defaults
        let mut config = builtin::get_model_defaults(model_id).unwrap_or_default();

        // Layer 2: User model config from models.json
        if let Some(user_config) = self.models.get(model_id) {
            config.merge_from(user_config);
            debug!(model = model_id, "Applied user model config");
        }

        // Check for model alias in provider config
        let canonical_model_id = self.resolve_model_alias(provider_name, model_id);

        // If alias resolved to different model, also apply its config
        if canonical_model_id != model_id {
            if let Some(alias_config) = self.models.get(&canonical_model_id) {
                config.merge_from(alias_config);
                debug!(
                    model = model_id,
                    canonical = canonical_model_id,
                    "Applied canonical model config"
                );
            }
        }

        // Layer 3: Provider-specific override
        if let Some(provider_config) = self.providers.get(provider_name) {
            if let Some(model_config) = provider_config.models.get(model_id) {
                if let Some(override_config) = &model_config.model_info_override {
                    config.merge_from(override_config);
                    debug!(
                        provider = provider_name,
                        model = model_id,
                        "Applied provider model override"
                    );
                }
            }
        }

        // Convert to resolved model info
        Ok(ResolvedModelInfo {
            id: model_id.to_string(),
            display_name: config.display_name.unwrap_or_else(|| model_id.to_string()),
            description: config.description,
            provider: provider_name.to_string(),
            context_window: config.context_window.unwrap_or(4096),
            max_output_tokens: config.max_output_tokens.unwrap_or(4096),
            capabilities: config
                .capabilities
                .unwrap_or_else(|| vec![Capability::TextGeneration]),
            auto_compact_token_limit: config.auto_compact_token_limit,
            effective_context_window_percent: config.effective_context_window_percent,
            default_reasoning_effort: config.default_reasoning_effort,
            supports_reasoning_summaries: config.supports_reasoning_summaries.unwrap_or(false),
            supports_parallel_tool_calls: config.supports_parallel_tool_calls.unwrap_or(false),
            thinking_budget_default: config.thinking_budget_default,
        })
    }

    /// Resolve a model alias to its canonical model ID.
    ///
    /// For example, "ep-20250109-xxxxx" might map to "deepseek-r1".
    pub fn resolve_model_alias(&self, provider_name: &str, model_id: &str) -> String {
        self.providers
            .get(provider_name)
            .and_then(|p| p.models.get(model_id))
            .and_then(|m| m.model_id.clone())
            .unwrap_or_else(|| model_id.to_string())
    }

    /// Resolve provider configuration.
    ///
    /// Resolves API key from environment variables if `env_key` is set.
    pub fn resolve_provider(
        &self,
        provider_name: &str,
    ) -> Result<ResolvedProviderConfig, ConfigError> {
        let provider_config = self
            .providers
            .get(provider_name)
            .ok_or_else(|| ConfigError::provider_not_found(provider_name))?;

        // Resolve API key: env var takes precedence
        let api_key = self.resolve_api_key(provider_config).ok_or_else(|| {
            let env_hint = provider_config
                .env_key
                .as_ref()
                .map(|k| format!(" (set {k} or api_key in config)"))
                .unwrap_or_default();
            ConfigError::auth(format!(
                "API key not found for provider '{provider_name}'{env_hint}"
            ))
        })?;

        Ok(ResolvedProviderConfig {
            name: provider_config.name.clone(),
            provider_type: provider_config.provider_type,
            api_key,
            base_url: provider_config.base_url.clone(),
            default_model: provider_config.default_model.clone(),
            timeout_secs: provider_config.timeout_secs.unwrap_or(600),
            organization_id: provider_config.organization_id.clone(),
            extra: provider_config.extra.clone(),
        })
    }

    /// Resolve API key from env var or config.
    fn resolve_api_key(&self, config: &ProviderJsonConfig) -> Option<String> {
        // Try environment variable first
        if let Some(env_key) = &config.env_key {
            if let Ok(key) = env::var(env_key) {
                if !key.is_empty() {
                    debug!(env_key = env_key, "Resolved API key from environment");
                    return Some(key);
                }
            }
        }

        // Fall back to config
        config.api_key.clone()
    }

    /// Resolve a profile configuration.
    pub fn resolve_profile(&self, profile_name: &str) -> Result<&ProfileConfig, ConfigError> {
        self.profiles
            .get(profile_name)
            .ok_or_else(|| ConfigError::profile_not_found(profile_name))
    }

    /// Get the default profile name.
    pub fn default_profile(&self) -> Option<&str> {
        self.default_profile.as_deref()
    }

    /// Check if a provider is configured.
    pub fn has_provider(&self, name: &str) -> bool {
        self.providers.contains_key(name)
    }

    /// Check if a profile is configured.
    pub fn has_profile(&self, name: &str) -> bool {
        self.profiles.contains_key(name)
    }

    /// List all configured provider names.
    pub fn list_providers(&self) -> Vec<&str> {
        self.providers.keys().map(String::as_str).collect()
    }

    /// List all configured profile names.
    pub fn list_profiles(&self) -> Vec<&str> {
        self.profiles.keys().map(String::as_str).collect()
    }

    /// List models configured for a provider.
    pub fn list_models(&self, provider_name: &str) -> Vec<&str> {
        self.providers
            .get(provider_name)
            .map(|p| p.models.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Get provider config by name (for inspection).
    pub fn get_provider_config(&self, name: &str) -> Option<&ProviderJsonConfig> {
        self.providers.get(name)
    }

    /// Get model config by ID (for inspection).
    pub fn get_model_config(&self, id: &str) -> Option<&ModelInfoConfig> {
        self.models.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ProviderModelConfig;
    use crate::types::ProviderType;

    fn create_test_resolver() -> ConfigResolver {
        let mut models = HashMap::new();
        models.insert(
            "test-model".to_string(),
            ModelInfoConfig {
                display_name: Some("Test Model".to_string()),
                context_window: Some(8192),
                max_output_tokens: Some(2048),
                capabilities: Some(vec![Capability::TextGeneration, Capability::Streaming]),
                ..Default::default()
            },
        );
        models.insert(
            "deepseek-r1".to_string(),
            ModelInfoConfig {
                display_name: Some("DeepSeek R1".to_string()),
                context_window: Some(64000),
                ..Default::default()
            },
        );

        let mut provider_models = HashMap::new();
        provider_models.insert(
            "test-model".to_string(),
            ProviderModelConfig {
                model_id: None,
                model_info_override: Some(ModelInfoConfig {
                    max_output_tokens: Some(4096), // Override
                    ..Default::default()
                }),
            },
        );
        provider_models.insert(
            "ep-12345".to_string(),
            ProviderModelConfig {
                model_id: Some("deepseek-r1".to_string()), // Alias
                model_info_override: Some(ModelInfoConfig {
                    context_window: Some(32000), // Override for this provider
                    ..Default::default()
                }),
            },
        );

        let mut providers = HashMap::new();
        providers.insert(
            "test-provider".to_string(),
            ProviderJsonConfig {
                name: "Test Provider".to_string(),
                provider_type: ProviderType::Openai,
                env_key: Some("TEST_API_KEY".to_string()),
                api_key: Some("fallback-key".to_string()),
                base_url: Some("https://api.test.com".to_string()),
                default_model: Some("test-model".to_string()),
                timeout_secs: Some(300),
                organization_id: None,
                models: provider_models,
                extra: None,
            },
        );

        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            ProfileConfig {
                provider: "test-provider".to_string(),
                model: "test-model".to_string(),
                session_config: None,
            },
        );

        ConfigResolver {
            models,
            providers,
            profiles,
            default_profile: Some("default".to_string()),
        }
    }

    #[test]
    fn test_resolve_model_info_basic() {
        let resolver = create_test_resolver();
        let info = resolver
            .resolve_model_info("test-provider", "test-model")
            .unwrap();

        assert_eq!(info.id, "test-model");
        assert_eq!(info.display_name, "Test Model");
        assert_eq!(info.context_window, 8192);
        // Provider override applied
        assert_eq!(info.max_output_tokens, 4096);
    }

    #[test]
    fn test_resolve_model_alias() {
        let resolver = create_test_resolver();

        // Direct alias resolution
        let canonical = resolver.resolve_model_alias("test-provider", "ep-12345");
        assert_eq!(canonical, "deepseek-r1");

        // Non-aliased model returns itself
        let canonical = resolver.resolve_model_alias("test-provider", "test-model");
        assert_eq!(canonical, "test-model");
    }

    #[test]
    fn test_resolve_model_with_alias() {
        let resolver = create_test_resolver();
        let info = resolver
            .resolve_model_info("test-provider", "ep-12345")
            .unwrap();

        assert_eq!(info.id, "ep-12345");
        // Provider override applied
        assert_eq!(info.context_window, 32000);
    }

    #[test]
    fn test_resolve_provider_with_env_key() {
        let resolver = create_test_resolver();

        // Set env var
        // SAFETY: This is a test, and we're using a unique env var name
        unsafe {
            env::set_var("TEST_API_KEY", "env-api-key");
        }

        let config = resolver.resolve_provider("test-provider").unwrap();
        assert_eq!(config.api_key, "env-api-key");

        // Clean up
        // SAFETY: This is a test cleanup
        unsafe {
            env::remove_var("TEST_API_KEY");
        }
    }

    #[test]
    fn test_resolve_provider_fallback_to_config() {
        let resolver = create_test_resolver();

        // Ensure env var is not set
        // SAFETY: This is a test cleanup
        unsafe {
            env::remove_var("TEST_API_KEY");
        }

        let config = resolver.resolve_provider("test-provider").unwrap();
        assert_eq!(config.api_key, "fallback-key");
    }

    #[test]
    fn test_resolve_provider_not_found() {
        use crate::error::NotFoundKind;
        let resolver = create_test_resolver();
        let result = resolver.resolve_provider("nonexistent");
        assert!(matches!(
            result,
            Err(ConfigError::NotFound {
                kind: NotFoundKind::Provider,
                ..
            })
        ));
    }

    #[test]
    fn test_resolve_profile() {
        let resolver = create_test_resolver();
        let profile = resolver.resolve_profile("default").unwrap();
        assert_eq!(profile.provider, "test-provider");
        assert_eq!(profile.model, "test-model");
    }

    #[test]
    fn test_default_profile() {
        let resolver = create_test_resolver();
        assert_eq!(resolver.default_profile(), Some("default"));
    }

    #[test]
    fn test_list_providers() {
        let resolver = create_test_resolver();
        let providers = resolver.list_providers();
        assert!(providers.contains(&"test-provider"));
    }

    #[test]
    fn test_list_models() {
        let resolver = create_test_resolver();
        let models = resolver.list_models("test-provider");
        assert!(models.contains(&"test-model"));
        assert!(models.contains(&"ep-12345"));
    }

    #[test]
    fn test_empty_resolver() {
        let resolver = ConfigResolver::empty();
        assert!(resolver.list_providers().is_empty());
        assert!(resolver.list_profiles().is_empty());
    }

    #[test]
    fn test_unknown_model_uses_defaults() {
        let resolver = create_test_resolver();
        let info = resolver
            .resolve_model_info("test-provider", "unknown-model")
            .unwrap();

        assert_eq!(info.id, "unknown-model");
        assert_eq!(info.display_name, "unknown-model"); // Falls back to ID
        assert_eq!(info.context_window, 4096); // Default
    }
}
