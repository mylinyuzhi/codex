//! Provider trait for AI service providers.

use crate::error::HyperError;
use crate::model::Model;
use async_trait::async_trait;
use std::fmt::Debug;
use std::sync::Arc;

/// A provider for AI models.
///
/// Providers are responsible for creating model instances.
/// Each provider represents a specific AI service (OpenAI, Anthropic, Google, etc.).
///
/// hyper-sdk is a thin network layer - it just makes API calls.
/// Model selection, capability checking, and routing are handled
/// by the upper layer (core/api, config).
#[async_trait]
pub trait Provider: Send + Sync + Debug {
    /// Get the provider name (e.g., "openai", "anthropic", "gemini").
    fn name(&self) -> &str;

    /// Get a model instance by ID.
    ///
    /// Returns an error if the model is not found or not supported.
    #[must_use = "this returns a Result that must be handled"]
    fn model(&self, model_id: &str) -> Result<Arc<dyn Model>, HyperError>;
}

/// Configuration for creating a provider.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    /// API key for authentication.
    pub api_key: Option<String>,
    /// Additional API keys for rotation on rate-limit errors.
    pub extra_api_keys: Vec<String>,
    /// Base URL override.
    pub base_url: Option<String>,
    /// Request timeout in seconds.
    pub timeout_secs: Option<i64>,
    /// Additional provider-specific configuration.
    pub extra: Option<serde_json::Value>,
}

impl ProviderConfig {
    /// Create a new provider config with an API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(api_key.into()),
            ..Default::default()
        }
    }

    /// Set the base URL.
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set the request timeout.
    pub fn with_timeout(mut self, secs: i64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Set extra configuration.
    pub fn with_extra(mut self, extra: serde_json::Value) -> Self {
        self.extra = Some(extra);
        self
    }

    /// Add extra API keys for rotation.
    pub fn with_extra_api_keys(mut self, keys: Vec<String>) -> Self {
        self.extra_api_keys = keys;
        self
    }

    /// Get the API key, returning an error if not set.
    #[must_use = "this returns a Result that must be handled"]
    pub fn require_api_key(&self) -> Result<&str, HyperError> {
        self.api_key
            .as_deref()
            .ok_or_else(|| HyperError::ConfigError("API key is required".to_string()))
    }

    /// Get all API keys (primary + extras), for use with [`ApiKeyRotator`].
    pub fn all_api_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if let Some(ref primary) = self.api_key {
            keys.push(primary.clone());
        }
        keys.extend(self.extra_api_keys.iter().cloned());
        keys
    }
}

#[cfg(test)]
#[path = "provider.test.rs"]
mod tests;
