//! Model caching and resolution optimization.
//!
//! This module provides efficient caching of resolved model information to avoid
//! redundant API calls and resolution logic. ModelCache maintains a HashMap of
//! ModelSpec → ModelInfo mappings with explicit lifecycle management.

use crate::error::ConfigError;
use cocode_protocol::ModelInfo;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelSpec;
use std::collections::HashMap;

/// Cache for resolved model information.
///
/// ModelCache maintains a HashMap of resolved models to avoid redundant lookups.
/// It's created fresh for each `build_config()` call to reflect current state,
/// but avoids resolving the same (provider, model) pair multiple times within
/// a single build operation.
///
/// # Example
///
/// ```ignore
/// let mut cache = ModelCache::new();
/// if let Some(info) = cache.get(&spec) {
///     println!("Cached: {}", info.slug);
/// } else {
///     // Resolve and insert
///     let info = resolver.resolve_model_info(&spec.provider, &spec.model)?;
///     cache.insert(spec, info);
/// }
/// ```
#[derive(Debug)]
pub struct ModelCache {
    /// Map of ModelSpec to resolved ModelInfo.
    cache: HashMap<ModelSpec, ModelInfo>,
}

impl ModelCache {
    /// Create a new empty model cache.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Get a cached model info by spec.
    pub fn get(&self, spec: &ModelSpec) -> Option<&ModelInfo> {
        self.cache.get(spec)
    }

    /// Insert a resolved model into the cache.
    pub fn insert(&mut self, spec: ModelSpec, info: ModelInfo) {
        self.cache.insert(spec, info);
    }

    /// Consume the cache and extract the inner HashMap.
    pub fn into_inner(self) -> HashMap<ModelSpec, ModelInfo> {
        self.cache
    }

    /// Build cache and resolved models for the given roles.
    ///
    /// This method handles the two-phase process:
    /// 1. Collects all unique models from roles and providers
    /// 2. Resolves each unique model ONCE and caches the result
    ///
    /// Returns:
    /// - HashMap of ModelSpec → ModelInfo (the cache)
    /// - HashMap of ModelRole → ModelInfo (for role-based lookups)
    ///
    /// # Arguments
    ///
    /// * `roles` - The role-to-model mappings to resolve
    /// * `list_providers_fn` - Closure to get all provider summaries
    /// * `list_model_slugs_fn` - Closure to get model slugs for a provider
    /// * `resolve_fn` - Closure to resolve a model by (provider, model) pair
    ///
    /// This approach allows ModelCache to be independent of ConfigManager
    /// while still providing flexible resolution logic.
    pub fn build_for_roles<F1, F2, F3>(
        &mut self,
        roles: &cocode_protocol::ModelRoles,
        list_providers_fn: F1,
        list_model_slugs_fn: F2,
        resolve_fn: F3,
    ) -> Result<HashMap<ModelRole, ModelInfo>, ConfigError>
    where
        F1: Fn() -> Vec<String>,
        F2: Fn(&str) -> Result<Vec<String>, ConfigError>,
        F3: Fn(&str, &str) -> Result<ModelInfo, ConfigError>,
    {
        // PHASE 1: Collect all unique models from roles AND providers
        let mut model_specs = std::collections::HashSet::new();

        // 1a. From configured roles
        for role in ModelRole::all() {
            if let Some(spec) = roles.get(*role) {
                model_specs.insert(spec.clone());
            }
        }

        // 1b. From all providers (to build ProviderInfo.models)
        for provider_name in list_providers_fn() {
            if let Ok(slugs) = list_model_slugs_fn(&provider_name) {
                for slug in slugs {
                    model_specs.insert(ModelSpec::new(&provider_name, &slug));
                }
            }
        }

        // PHASE 2: Resolve each unique model ONCE into cache
        for spec in model_specs {
            if let Ok(info) = resolve_fn(&spec.provider, &spec.slug) {
                self.insert(spec, info);
            }
        }

        // PHASE 3: Build resolved_models from cache (for role lookups)
        let mut resolved_models = HashMap::new();
        for role in ModelRole::all() {
            if let Some(spec) = roles.get(*role)
                && let Some(info) = self.get(spec)
            {
                resolved_models.insert(*role, info.clone());
            }
        }

        Ok(resolved_models)
    }
}

impl Default for ModelCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "model_cache.test.rs"]
mod tests;
