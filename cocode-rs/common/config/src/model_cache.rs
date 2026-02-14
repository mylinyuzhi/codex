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
            if let Ok(info) = resolve_fn(&spec.provider, &spec.model) {
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
mod tests {
    use super::*;

    #[test]
    fn test_model_cache_new() {
        let cache = ModelCache::new();
        assert!(cache.cache.is_empty());
    }

    #[test]
    fn test_model_cache_insert_and_get() {
        let mut cache = ModelCache::new();
        let spec = ModelSpec::new("openai", "gpt-4");
        let info = ModelInfo {
            slug: "gpt-4".to_string(),
            display_name: Some("GPT-4".to_string()),
            context_window: Some(8192),
            ..Default::default()
        };

        cache.insert(spec.clone(), info);
        assert_eq!(cache.cache.len(), 1);
        assert!(cache.get(&spec).is_some());
        assert_eq!(cache.get(&spec).unwrap().slug, "gpt-4");
    }

    #[test]
    fn test_model_cache_multiple_entries() {
        let mut cache = ModelCache::new();
        let spec1 = ModelSpec::new("openai", "gpt-4");
        let spec2 = ModelSpec::new("openai", "gpt-3.5");
        let spec3 = ModelSpec::new("anthropic", "claude-3");

        let info = ModelInfo {
            slug: "test".to_string(),
            ..Default::default()
        };

        cache.insert(spec1.clone(), info.clone());
        cache.insert(spec2.clone(), info.clone());
        cache.insert(spec3.clone(), info);

        assert_eq!(cache.cache.len(), 3);
        assert!(cache.get(&spec1).is_some());
        assert!(cache.get(&spec2).is_some());
        assert!(cache.get(&spec3).is_some());
    }

    #[test]
    fn test_model_cache_clear() {
        let mut cache = ModelCache::new();
        let spec = ModelSpec::new("openai", "gpt-4");
        let info = ModelInfo {
            slug: "gpt-4".to_string(),
            ..Default::default()
        };

        cache.insert(spec, info);
        assert_eq!(cache.cache.len(), 1);

        cache.cache.clear();
        assert!(cache.cache.is_empty());
    }

    #[test]
    fn test_model_cache_into_inner() {
        let mut cache = ModelCache::new();
        let spec = ModelSpec::new("openai", "gpt-4");
        let info = ModelInfo {
            slug: "gpt-4".to_string(),
            ..Default::default()
        };

        cache.insert(spec, info);
        let inner = cache.into_inner();
        assert_eq!(inner.len(), 1);
    }
}
