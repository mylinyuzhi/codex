//! Configuration resolution and merging logic.
//!
//! This module implements the layered configuration resolution:
//!
//! **Precedence (highest to lowest):**
//! 1. Runtime overrides (API calls, `/model` command)
//! 2. Environment variables (for secrets)
//! 3. Model entry in provider config (flattened ModelInfo + model_options)
//! 4. User model config (`models.json`)
//! 5. Built-in defaults (compiled into binary)

use crate::builtin;
use crate::error::ConfigError;
use crate::error::NotFoundKind;
use crate::error::config_error::AuthSnafu;
use crate::error::config_error::ConfigValidationSnafu;
use crate::error::config_error::NotFoundSnafu;
use crate::types::ModelsFile;
use crate::types::ProviderConfig;
use crate::types::ProvidersFile;
use cocode_protocol::ModelInfo;
use cocode_protocol::ProviderInfo;
use cocode_protocol::ProviderModel;
use cocode_protocol::ProviderType;
use snafu::OptionExt;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use tracing::debug;
use tracing::info;
use tracing::warn;

/// Configuration resolver that merges layers of configuration.
#[derive(Debug, Clone)]
pub struct ConfigResolver {
    pub(crate) models: HashMap<String, ModelInfo>,
    pub(crate) providers: HashMap<String, ProviderConfig>,
    /// Config directory for resolving relative paths (e.g., base_instructions_file).
    pub(crate) config_dir: Option<PathBuf>,
}

impl ConfigResolver {
    /// Create a new resolver from loaded configuration.
    pub fn new(models_file: ModelsFile, providers_file: ProvidersFile) -> Self {
        Self {
            models: models_file.models,
            providers: providers_file.providers,
            config_dir: None,
        }
    }

    /// Create a new resolver with a config directory.
    pub fn with_config_dir(
        models_file: ModelsFile,
        providers_file: ProvidersFile,
        config_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            models: models_file.models,
            providers: providers_file.providers,
            config_dir: Some(config_dir.into()),
        }
    }

    /// Create an empty resolver (uses only built-in defaults).
    pub fn empty() -> Self {
        Self {
            models: HashMap::new(),
            providers: HashMap::new(),
            config_dir: None,
        }
    }

    /// Set the config directory for resolving relative paths.
    pub fn set_config_dir(&mut self, config_dir: impl Into<PathBuf>) {
        self.config_dir = Some(config_dir.into());
    }

    /// Resolve model info by merging all configuration layers.
    ///
    /// Resolution order (later overrides earlier):
    /// 1. Built-in defaults
    /// 2. User model config (models.json)
    /// 3. Model entry config (flattened ModelInfo fields)
    /// 4. Model entry options (merged into ModelInfo.options)
    ///
    /// # Arguments
    /// * `provider_name` - Provider identifier (e.g., "openai", "anthropic")
    /// * `slug` - Model configuration identifier (e.g., "gpt-4o", "deepseek-r1")
    pub fn resolve_model_info(
        &self,
        provider_name: &str,
        slug: &str,
    ) -> Result<ModelInfo, ConfigError> {
        // Get provider config, or use a default empty one
        let config = if let Some(provider_config) = self.providers.get(provider_name) {
            self.resolve_model_info_for_provider(provider_config, slug)
        } else {
            // No provider config, use defaults only
            self.resolve_model_info_no_provider(slug)
        };

        // Validate required fields
        if config.context_window.is_none() || config.max_output_tokens.is_none() {
            return ConfigValidationSnafu {
                file: format!("model:{slug}"),
                message: "context_window and max_output_tokens are required".to_string(),
            }
            .fail();
        }

        Ok(config)
    }

    /// Resolve base model info from built-in defaults and user config (layers 1-2).
    fn resolve_base_model_info(&self, slug: &str) -> ModelInfo {
        let mut config = builtin::get_model_defaults(slug).unwrap_or_default();
        config.slug = slug.to_string();
        if let Some(user_config) = self.models.get(slug) {
            config.merge_from(user_config);
            debug!(slug = slug, "Applied user model config");
        }
        config
    }

    /// Resolve model info without a provider config (fallback path).
    fn resolve_model_info_no_provider(&self, slug: &str) -> ModelInfo {
        let mut config = self.resolve_base_model_info(slug);
        if let Some(resolved_instructions) = self.resolve_base_instructions(&config) {
            config.base_instructions = Some(resolved_instructions);
            config.base_instructions_file = None;
        }
        config
    }

    /// Resolve base_instructions from inline string or file.
    ///
    /// If `base_instructions_file` is set and the file exists, load its content.
    /// Otherwise, use the inline `base_instructions`.
    fn resolve_base_instructions(&self, config: &ModelInfo) -> Option<String> {
        // Try to load from file first if config_dir is set
        if let (Some(file_path), Some(config_dir)) =
            (&config.base_instructions_file, &self.config_dir)
        {
            let full_path = config_dir.join(file_path);
            match std::fs::read_to_string(&full_path) {
                Ok(content) => {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        // Log the overwrite if inline instructions were also set
                        if config.base_instructions.is_some() {
                            info!(
                                file = %full_path.display(),
                                "Loaded base_instructions from file (overwriting inline)"
                            );
                        } else {
                            debug!(file = %full_path.display(), "Loaded base_instructions from file");
                        }
                        return Some(trimmed.to_string());
                    }
                }
                Err(e) => {
                    warn!(
                        file = %full_path.display(),
                        error = %e,
                        "Failed to read base_instructions_file"
                    );
                }
            }
        }

        // Fall back to inline instructions
        config.base_instructions.clone()
    }

    /// Resolve a model alias to its API model name.
    ///
    /// Returns the alias if set and non-empty, otherwise returns the slug.
    /// For example, slug "deepseek-r1" might return "ep-20250109-xxxxx".
    pub fn resolve_model_alias<'a>(&'a self, provider_name: &str, slug: &'a str) -> &'a str {
        self.providers
            .get(provider_name)
            .and_then(|p| p.find_model(slug))
            .map(super::types::ProviderModelEntry::api_model_name)
            .unwrap_or(slug)
    }

    /// Resolve provider configuration into a complete `ProviderInfo`.
    ///
    /// This resolves:
    /// - API key from environment variables or config
    /// - All models with their resolved `ModelInfo`
    pub fn resolve_provider(&self, provider_name: &str) -> Result<ProviderInfo, ConfigError> {
        self.resolve_provider_impl(provider_name, None)
    }

    /// Resolve provider using pre-resolved model cache (avoids redundant model resolution).
    ///
    /// This is used by `build_config()` to avoid re-resolving models that were already
    /// resolved in the role resolution phase. The cache is keyed by `ModelSpec`.
    pub fn resolve_provider_with_cache(
        &self,
        provider_name: &str,
        model_cache: &HashMap<cocode_protocol::model::ModelSpec, ModelInfo>,
    ) -> Result<ProviderInfo, ConfigError> {
        self.resolve_provider_impl(provider_name, Some(model_cache))
    }

    /// Internal implementation for provider resolution with optional cache.
    fn resolve_provider_impl(
        &self,
        provider_name: &str,
        model_cache: Option<&HashMap<cocode_protocol::model::ModelSpec, ModelInfo>>,
    ) -> Result<ProviderInfo, ConfigError> {
        use cocode_protocol::model::ModelSpec;

        let provider_config = self.providers.get(provider_name).context(NotFoundSnafu {
            kind: NotFoundKind::Provider,
            name: provider_name.to_string(),
        })?;

        // Resolve API key: env var takes precedence
        let api_key = self.resolve_api_key(provider_config).ok_or_else(|| {
            let env_hint = provider_config
                .env_key
                .as_ref()
                .map(|k| format!(" (set {k} or api_key in config)"))
                .unwrap_or_default();
            AuthSnafu {
                message: format!("API key not found for provider '{provider_name}'{env_hint}"),
            }
            .build()
        })?;

        // Build models using cache if provided, otherwise resolve directly
        let mut models = HashMap::new();
        for model_entry in &provider_config.models {
            let slug = model_entry.slug();

            // Resolve model info: use cache if available, otherwise resolve fresh
            let model_info = if let Some(cache) = model_cache {
                let cache_key = ModelSpec::new(provider_name, slug);
                if let Some(cached_info) = cache.get(&cache_key) {
                    cached_info.clone()
                } else {
                    // Fall back to resolution if not in cache
                    self.resolve_model_info_for_provider(provider_config, slug)
                }
            } else {
                // No cache provided, resolve directly
                self.resolve_model_info_for_provider(provider_config, slug)
            };

            // Create ProviderModel with model_alias preserved
            let provider_model = if let Some(alias) = &model_entry.model_alias {
                ProviderModel::with_alias(model_info, alias)
            } else {
                ProviderModel::new(model_info)
            };
            models.insert(slug.to_string(), provider_model);
        }

        let mut info = ProviderInfo::new(
            &provider_config.name,
            provider_config.provider_type,
            &provider_config.base_url,
        )
        .with_api_key(api_key)
        .with_timeout(provider_config.timeout_secs)
        .with_streaming(provider_config.streaming)
        .with_wire_api(provider_config.wire_api)
        .with_models(models);

        if let Some(extra) = &provider_config.options {
            info = info.with_options(extra.clone());
        }

        Ok(info)
    }

    /// Resolve and merge model config layers, returning a `ModelInfo`.
    ///
    /// This is used when building `ProviderInfo.models` to store fully resolved configs.
    ///
    /// # Arguments
    /// * `provider_config` - Provider configuration
    /// * `slug` - Model configuration identifier
    fn resolve_model_info_for_provider(
        &self,
        provider_config: &ProviderConfig,
        slug: &str,
    ) -> ModelInfo {
        let mut config = self.resolve_base_model_info(slug);

        // Layer 3: Model entry config and options
        if let Some(model_entry) = provider_config.find_model(slug) {
            config.merge_from(&model_entry.model_info);
            if !model_entry.model_options.is_empty() {
                let opts = config.options.get_or_insert_with(HashMap::new);
                for (k, v) in &model_entry.model_options {
                    opts.insert(k.clone(), v.clone());
                }
            }
        }

        // Timeout fallback
        if config.timeout_secs.is_none() {
            config.timeout_secs = Some(provider_config.timeout_secs);
        }

        // Resolve base_instructions
        if let Some(resolved_instructions) = self.resolve_base_instructions(&config) {
            config.base_instructions = Some(resolved_instructions);
            config.base_instructions_file = None;
        }

        config
    }

    /// Resolve API key from env var or config.
    fn resolve_api_key(&self, config: &ProviderConfig) -> Option<String> {
        // Try environment variable first
        if let Some(env_key) = &config.env_key
            && let Ok(key) = env::var(env_key)
            && !key.is_empty()
        {
            debug!(env_key = env_key, "Resolved API key from environment");
            return Some(key);
        }

        // Fall back to config
        config.api_key.clone()
    }

    /// Get the provider type by name (O(1) HashMap lookup, no resolution).
    pub fn provider_type(&self, provider_name: &str) -> Result<ProviderType, ConfigError> {
        let config = self.providers.get(provider_name).context(NotFoundSnafu {
            kind: NotFoundKind::Provider,
            name: provider_name.to_string(),
        })?;
        Ok(config.provider_type)
    }

    /// Check if a provider is configured.
    pub fn has_provider(&self, name: &str) -> bool {
        self.providers.contains_key(name)
    }

    /// List all configured provider names.
    pub fn list_providers(&self) -> Vec<&str> {
        self.providers.keys().map(String::as_str).collect()
    }

    /// List models configured for a provider.
    pub fn list_models(&self, provider_name: &str) -> Vec<&str> {
        self.providers
            .get(provider_name)
            .map(|p| p.list_model_slugs())
            .unwrap_or_default()
    }

    /// Get provider config by name (for inspection).
    pub fn get_provider_config(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }

    /// Get model config by ID (for inspection).
    pub fn get_model_config(&self, id: &str) -> Option<&ModelInfo> {
        self.models.get(id)
    }
}

#[cfg(test)]
#[path = "resolver.test.rs"]
mod tests;
