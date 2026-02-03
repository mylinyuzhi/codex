//! Model resolver with provider and model caching.
//!
//! [`ModelResolver`] provides:
//! - Role-based model resolution with fallback to Main
//! - Provider caching (HTTP clients are expensive to create)
//! - Model caching keyed by ModelSpec
//! - Selections passed as parameter (not owned)
//!
//! # Example
//!
//! ```ignore
//! use cocode_api::ModelResolver;
//! use cocode_protocol::model::ModelRole;
//! use cocode_protocol::RoleSelections;
//!
//! let resolver = ModelResolver::new(config);
//!
//! // Selections are passed in, not stored
//! let selections = RoleSelections::default();
//! let (model, provider_type) = resolver.get_for_role(ModelRole::Main, &selections)?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use cocode_config::ConfigManager;
use cocode_protocol::ProviderType;
use cocode_protocol::RoleSelections;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelSpec;
use hyper_sdk::Model;
use hyper_sdk::Provider;
use tracing::debug;
use tracing::info;

use crate::provider_factory;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur when resolving models.
#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    /// No model is configured for the requested role or the Main fallback.
    #[error("No model configured for role {role} or Main")]
    NoModelConfigured { role: ModelRole },

    /// Failed to resolve provider configuration.
    #[error("Failed to resolve provider '{provider}': {source}")]
    ProviderResolution {
        provider: String,
        #[source]
        source: anyhow::Error,
    },

    /// Failed to create provider instance.
    #[error("Failed to create provider '{provider}': {source}")]
    ProviderCreation {
        provider: String,
        #[source]
        source: crate::error::ApiError,
    },

    /// Failed to create model instance.
    #[error("Failed to create model '{model}' from provider '{provider}': {source}")]
    ModelCreation {
        provider: String,
        model: String,
        #[source]
        source: crate::error::ApiError,
    },

    /// Internal lock was poisoned.
    #[error("Internal lock poisoned")]
    LockPoisoned,
}

impl ResolverError {
    /// Check if this error indicates no model is configured.
    pub fn is_no_model_configured(&self) -> bool {
        matches!(self, Self::NoModelConfigured { .. })
    }
}

// ============================================================================
// Cached Types
// ============================================================================

/// A cached provider instance.
struct CachedProvider {
    provider: Arc<dyn Provider>,
    provider_type: ProviderType,
}

/// A cached model instance.
struct CachedModel {
    model: Arc<dyn Model>,
    provider_type: ProviderType,
}

// ============================================================================
// ModelResolver
// ============================================================================

/// Model resolver with provider and model caching.
///
/// Unlike [`MultiModel`], `ModelResolver` does not own selections.
/// Instead, selections are passed to `get_for_role()` as a parameter.
/// This makes `SessionState.current_selections` the single source of truth.
///
/// # Caching
///
/// - **Provider cache**: Keyed by provider name (e.g., "anthropic", "openai").
///   HTTP clients are expensive to create, so we reuse them.
/// - **Model cache**: Keyed by `ModelSpec` (provider + model name).
///   Model instances are relatively cheap but still worth caching.
///
/// # Thread Safety
///
/// Uses `RwLock` for caches to allow concurrent reads with exclusive writes.
/// Model creation happens outside the lock to avoid blocking other threads.
pub struct ModelResolver {
    config: Arc<ConfigManager>,
    /// Cached providers keyed by provider name.
    providers: RwLock<HashMap<String, CachedProvider>>,
    /// Cached models keyed by ModelSpec.
    models: RwLock<HashMap<ModelSpec, CachedModel>>,
}

impl ModelResolver {
    /// Create a new model resolver.
    pub fn new(config: Arc<ConfigManager>) -> Self {
        Self {
            config,
            providers: RwLock::new(HashMap::new()),
            models: RwLock::new(HashMap::new()),
        }
    }

    /// Get model for a role using the provided selections.
    ///
    /// Falls back to Main if the requested role has no selection.
    ///
    /// # Arguments
    ///
    /// * `role` - The role to get the model for
    /// * `selections` - The current role selections (passed by reference)
    ///
    /// # Returns
    ///
    /// A tuple of (model, provider_type) on success.
    pub fn get_for_role(
        &self,
        role: ModelRole,
        selections: &RoleSelections,
    ) -> Result<(Arc<dyn Model>, ProviderType), ResolverError> {
        // Get selection with fallback to Main
        let selection = selections
            .get_or_main(role)
            .ok_or(ResolverError::NoModelConfigured { role })?;

        let spec = &selection.model;
        self.get(spec)
    }

    /// Get model by ModelSpec.
    ///
    /// Uses the model cache, falling back to provider cache and creation.
    pub fn get(&self, spec: &ModelSpec) -> Result<(Arc<dyn Model>, ProviderType), ResolverError> {
        // Phase 1: Check model cache (read lock)
        {
            let cache = self
                .models
                .read()
                .map_err(|_| ResolverError::LockPoisoned)?;
            if let Some(cached) = cache.get(spec) {
                debug!(
                    provider = %spec.provider,
                    model = %spec.model,
                    "Model cache hit"
                );
                return Ok((cached.model.clone(), cached.provider_type));
            }
        }

        // Phase 2: Get or create provider
        let (provider, provider_type) = self.get_or_create_provider(&spec.provider)?;

        // Phase 3: Resolve model info and create model
        let provider_info = self.config.resolve_provider(&spec.provider).map_err(|e| {
            ResolverError::ProviderResolution {
                provider: spec.provider.clone(),
                source: e.into(),
            }
        })?;

        // Get the actual API model name (handles aliases)
        let api_model_name = provider_info
            .api_model_name(&spec.model)
            .unwrap_or(&spec.model);

        info!(
            provider = %spec.provider,
            model = %spec.model,
            api_model = %api_model_name,
            "Creating model"
        );

        let model = provider
            .model(api_model_name)
            .map_err(|e| ResolverError::ModelCreation {
                provider: spec.provider.clone(),
                model: spec.model.clone(),
                source: e.into(),
            })?;

        // Phase 4: Double-check and store in model cache (write lock)
        {
            let mut cache = self
                .models
                .write()
                .map_err(|_| ResolverError::LockPoisoned)?;

            // Another thread might have created it
            if let Some(cached) = cache.get(spec) {
                debug!(
                    provider = %spec.provider,
                    model = %spec.model,
                    "Model created by another thread, using existing"
                );
                return Ok((cached.model.clone(), cached.provider_type));
            }

            cache.insert(
                spec.clone(),
                CachedModel {
                    model: model.clone(),
                    provider_type,
                },
            );
        }

        Ok((model, provider_type))
    }

    /// Get the main model (shorthand for get_for_role(Main, selections)).
    pub fn main(&self, selections: &RoleSelections) -> Result<Arc<dyn Model>, ResolverError> {
        self.get_for_role(ModelRole::Main, selections)
            .map(|(m, _)| m)
    }

    /// Invalidate cached model for a specific spec.
    pub fn invalidate_model(&self, spec: &ModelSpec) {
        if let Ok(mut cache) = self.models.write() {
            if cache.remove(spec).is_some() {
                debug!(
                    provider = %spec.provider,
                    model = %spec.model,
                    "Invalidated cached model"
                );
            }
        }
    }

    /// Invalidate cached provider (and all its models).
    pub fn invalidate_provider(&self, provider_name: &str) {
        // Remove provider
        if let Ok(mut cache) = self.providers.write() {
            if cache.remove(provider_name).is_some() {
                debug!(provider = %provider_name, "Invalidated cached provider");
            }
        }

        // Remove all models for this provider
        if let Ok(mut cache) = self.models.write() {
            let to_remove: Vec<ModelSpec> = cache
                .keys()
                .filter(|spec| spec.provider == provider_name)
                .cloned()
                .collect();

            for spec in to_remove {
                cache.remove(&spec);
                debug!(
                    provider = %spec.provider,
                    model = %spec.model,
                    "Invalidated cached model (provider invalidation)"
                );
            }
        }
    }

    /// Invalidate all caches.
    pub fn invalidate_all(&self) {
        if let Ok(mut cache) = self.providers.write() {
            cache.clear();
        }
        if let Ok(mut cache) = self.models.write() {
            cache.clear();
        }
        debug!("Invalidated all cached providers and models");
    }

    /// Get the number of cached providers.
    pub fn provider_cache_size(&self) -> usize {
        self.providers.read().map(|c| c.len()).unwrap_or(0)
    }

    /// Get the number of cached models.
    pub fn model_cache_size(&self) -> usize {
        self.models.read().map(|c| c.len()).unwrap_or(0)
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    /// Get or create a provider instance.
    fn get_or_create_provider(
        &self,
        provider_name: &str,
    ) -> Result<(Arc<dyn Provider>, ProviderType), ResolverError> {
        // Phase 1: Check provider cache (read lock)
        {
            let cache = self
                .providers
                .read()
                .map_err(|_| ResolverError::LockPoisoned)?;
            if let Some(cached) = cache.get(provider_name) {
                debug!(provider = %provider_name, "Provider cache hit");
                return Ok((cached.provider.clone(), cached.provider_type));
            }
        }

        // Phase 2: Resolve provider info and create provider
        let provider_info = self.config.resolve_provider(provider_name).map_err(|e| {
            ResolverError::ProviderResolution {
                provider: provider_name.to_string(),
                source: e.into(),
            }
        })?;

        info!(provider = %provider_name, "Creating provider");
        let provider = provider_factory::create_provider(&provider_info).map_err(|e| {
            ResolverError::ProviderCreation {
                provider: provider_name.to_string(),
                source: e,
            }
        })?;
        let provider_type = provider_info.provider_type;

        // Phase 3: Double-check and store in cache (write lock)
        {
            let mut cache = self
                .providers
                .write()
                .map_err(|_| ResolverError::LockPoisoned)?;

            // Another thread might have created it
            if let Some(cached) = cache.get(provider_name) {
                debug!(
                    provider = %provider_name,
                    "Provider created by another thread, using existing"
                );
                return Ok((cached.provider.clone(), cached.provider_type));
            }

            cache.insert(
                provider_name.to_string(),
                CachedProvider {
                    provider: provider.clone(),
                    provider_type,
                },
            );
        }

        Ok((provider, provider_type))
    }
}

impl std::fmt::Debug for ModelResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelResolver")
            .field("provider_cache_size", &self.provider_cache_size())
            .field("model_cache_size", &self.model_cache_size())
            .finish()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolver_error_no_model_configured() {
        let err = ResolverError::NoModelConfigured {
            role: ModelRole::Main,
        };
        assert!(err.is_no_model_configured());
        assert!(err.to_string().contains("No model configured"));
    }

    #[test]
    fn test_resolver_error_provider_resolution() {
        let err = ResolverError::ProviderResolution {
            provider: "test".to_string(),
            source: anyhow::anyhow!("not found"),
        };
        assert!(!err.is_no_model_configured());
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn test_resolver_new() {
        let config = ConfigManager::empty();
        let resolver = ModelResolver::new(Arc::new(config));
        assert_eq!(resolver.provider_cache_size(), 0);
        assert_eq!(resolver.model_cache_size(), 0);
    }

    #[test]
    fn test_resolver_empty_selections_returns_error() {
        let config = ConfigManager::empty();
        let resolver = ModelResolver::new(Arc::new(config));
        let selections = RoleSelections::default();

        let result = resolver.get_for_role(ModelRole::Main, &selections);
        assert!(result.is_err());
        assert!(result.unwrap_err().is_no_model_configured());
    }

    #[test]
    fn test_resolver_debug() {
        let config = ConfigManager::empty();
        let resolver = ModelResolver::new(Arc::new(config));
        let debug_str = format!("{:?}", resolver);
        assert!(debug_str.contains("ModelResolver"));
        assert!(debug_str.contains("provider_cache_size"));
        assert!(debug_str.contains("model_cache_size"));
    }
}
