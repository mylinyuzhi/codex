//! Configuration types for multi-provider management.
//!
//! This module defines the types used to configure models and providers
//! from JSON/TOML files. The configuration follows a layered approach:
//!
//! - `models.json`: Provider-independent model metadata
//! - `providers.json` / `config.toml`: Provider configuration with model entries
//!
//! For resolved runtime types, see `ProviderInfo` in cocode_protocol.

pub mod domain;

pub use domain::ApiKey;

use crate::error::config_error::ConfigValidationSnafu;
use cocode_protocol::Capability;
use cocode_protocol::ModelInfo;
use cocode_protocol::ProviderApi;
use cocode_protocol::WireApi;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

/// Insert items into a map, returning an error on duplicate keys.
fn add_items_unique<T>(
    map: &mut HashMap<String, T>,
    items: Vec<T>,
    key_fn: impl Fn(&T) -> &str,
    item_type: &str,
    source: &impl std::fmt::Display,
) -> Result<(), crate::error::ConfigError> {
    for item in items {
        let key = key_fn(&item);
        if map.contains_key(key) {
            return ConfigValidationSnafu {
                file: source.to_string(),
                message: format!("duplicate {item_type}: {key}"),
            }
            .fail();
        }
        map.insert(key.to_string(), item);
    }
    Ok(())
}

/// Internal storage for model configurations.
///
/// **Important**: External config files use **array format**:
/// ```json
/// [{"slug": "gpt-5", "display_name": "GPT-5", ...}]
/// ```
///
/// This struct is populated by `ConfigLoader` which deserializes the array
/// and converts it to a HashMap keyed by `slug`.
///
/// Do NOT deserialize config files directly into this type.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsFile {
    /// Map of model slug to model configuration.
    #[serde(default)]
    pub models: HashMap<String, ModelInfo>,
}

impl ModelsFile {
    /// Add models from a list, error on duplicate slug.
    pub fn add_models(
        &mut self,
        models: Vec<ModelInfo>,
        source: impl std::fmt::Display,
    ) -> Result<(), crate::error::ConfigError> {
        add_items_unique(&mut self.models, models, |m| &m.slug, "model slug", &source)
    }
}

/// Internal storage for provider configurations.
///
/// **Important**: External config files use **array format**:
/// ```json
/// [{"name": "openai", "api": "openai", "base_url": "...", ...}]
/// ```
///
/// This struct is populated by `ConfigLoader` which deserializes the array
/// and converts it to a HashMap keyed by `name`.
///
/// Do NOT deserialize config files directly into this type.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvidersFile {
    /// Map of provider name (identifier) to provider configuration.
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
}

impl ProvidersFile {
    /// Add providers from a list, error on duplicate name.
    pub fn add_providers(
        &mut self,
        providers: Vec<ProviderConfig>,
        source: impl std::fmt::Display,
    ) -> Result<(), crate::error::ConfigError> {
        add_items_unique(
            &mut self.providers,
            providers,
            |p| &p.name,
            "provider name",
            &source,
        )
    }
}

fn default_timeout() -> i64 {
    600
}

fn default_true() -> bool {
    true
}

/// Provider configuration from JSON.
///
/// Example JSON:
/// ```json
/// {
///   "name": "openai",
///   "api": "openai",
///   "base_url": "https://api.openai.com/v1",
///   "env_key": "OPENAI_API_KEY",
///   "streaming": true,
///   "wire_api": "responses",
///   "models": [
///     {"slug": "gpt-5"},
///     {"slug": "gpt-4o", "api_model_name": "gpt-4o-2024-08-06"}
///   ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider identifier (used as map key, e.g., "openai", "anthropic").
    pub name: String,

    /// Provider API type for selecting the implementation.
    #[serde(rename = "api")]
    pub api: ProviderApi,

    /// Base URL for API endpoint.
    pub base_url: String,

    /// Request timeout in seconds (default: 600).
    /// Note: Can be overridden per-model via ModelInfo.timeout_secs.
    #[serde(default = "default_timeout")]
    pub timeout_secs: i64,

    /// Environment variable name for API key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,

    /// API key (prefer env_key for security).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Enable streaming mode (default: true).
    #[serde(default = "default_true")]
    pub streaming: bool,

    /// Wire protocol (responses or chat, default: responses).
    #[serde(default)]
    pub wire_api: WireApi,

    /// Models this provider serves.
    #[serde(default)]
    pub models: Vec<ProviderModelConfig>,

    /// Provider-specific SDK client options (e.g., organization_id, use_zhipuai).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,

    /// HTTP interceptors to apply to requests.
    ///
    /// Interceptors are applied in order of their priority (lower = earlier).
    /// Available built-in interceptors:
    /// - `byted_model_hub`: Adds session_id to "extra" header for ByteDance ModelHub
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interceptors: Vec<String>,
}

impl ProviderConfig {
    /// Validate required fields.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("provider name is required".to_string());
        }
        if self.base_url.is_empty() {
            return Err("provider base_url is required".to_string());
        }
        Ok(())
    }

    /// Convert to domain type (partial, without resolved API key or models).
    ///
    /// Use `ConfigResolver::resolve_provider()` to get a fully resolved `ProviderInfo`.
    pub fn to_provider_info(&self) -> cocode_protocol::ProviderInfo {
        cocode_protocol::ProviderInfo::new(&self.name, self.api, &self.base_url)
            .with_timeout(self.timeout_secs)
            .with_streaming(self.streaming)
            .with_wire_api(self.wire_api)
    }

    /// Find a model config by slug.
    pub fn find_model(&self, slug: &str) -> Option<&ProviderModelConfig> {
        self.models.iter().find(|m| m.slug == slug)
    }

    /// List all model slugs in this provider.
    pub fn list_model_slugs(&self) -> Vec<&str> {
        self.models.iter().map(|m| m.slug.as_str()).collect()
    }
}

/// Per-model configuration within a provider config.
///
/// This is a thin slug reference — all model metadata (context_window, max_output_tokens,
/// thinking levels, etc.) belongs in `models.json` or builtins, not here.
///
/// Example JSON:
/// ```json
/// {"slug": "deepseek-r1", "api_model_name": "ep-20250101-xxxxx"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderModelConfig {
    /// Model slug — references a model defined in models.json or builtins.
    pub slug: String,

    /// API model name if different from slug (e.g., "ep-xxx" endpoint ID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_model_name: Option<String>,

    /// Model-specific options (temperature, seed, etc.).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub model_options: HashMap<String, serde_json::Value>,
}

impl ProviderModelConfig {
    /// Create a new entry with just a slug.
    pub fn new(slug: impl Into<String>) -> Self {
        Self {
            slug: slug.into(),
            api_model_name: None,
            model_options: HashMap::new(),
        }
    }

    /// Create a new entry with a slug and api_model_name.
    pub fn with_api_model_name(slug: impl Into<String>, api_model_name: impl Into<String>) -> Self {
        Self {
            slug: slug.into(),
            api_model_name: Some(api_model_name.into()),
            model_options: HashMap::new(),
        }
    }

    /// Get the slug (model identifier).
    pub fn slug(&self) -> &str {
        &self.slug
    }

    /// Get the API model name (alias if set and non-empty, otherwise slug).
    pub fn api_model_name(&self) -> &str {
        self.api_model_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.slug)
    }
}

/// Summary of a provider for listing.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderSummary {
    /// Provider key/name.
    pub name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Provider API type.
    pub api: ProviderApi,
    /// Whether API key is configured.
    pub has_api_key: bool,
    /// Number of models configured.
    pub model_count: i32,
}

impl ProviderSummary {
    /// Create from a configured `ProviderConfig`.
    pub fn from_config(key: &str, config: &ProviderConfig) -> Self {
        Self {
            name: key.to_string(),
            display_name: config.name.clone(),
            api: config.api,
            has_api_key: config.api_key.is_some() || config.env_key.is_some(),
            model_count: config.models.len() as i32,
        }
    }

    /// Create from a built-in provider defaults.
    pub fn from_builtin(name: &str, config: &ProviderConfig) -> Self {
        Self {
            name: name.to_string(),
            display_name: config.name.clone(),
            api: config.api,
            has_api_key: config.env_key.is_some(),
            model_count: 0,
        }
    }
}

/// Summary of a model for listing.
#[derive(Debug, Clone, Serialize)]
pub struct ModelSummary {
    /// Model ID.
    pub id: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Context window size.
    pub context_window: Option<i64>,
    /// Capabilities summary.
    pub capabilities: Vec<Capability>,
}

impl ModelSummary {
    /// Create from a resolved `ModelInfo`.
    pub fn from_model_info(id: &str, info: &ModelInfo) -> Self {
        Self {
            id: id.to_string(),
            display_name: info.display_name_or_slug().to_string(),
            context_window: info.context_window,
            capabilities: info.capabilities.clone().unwrap_or_default(),
        }
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
