//! Global provider registry.

use crate::error::HyperError;
use crate::provider::Provider;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::RwLock;
use tracing::debug;

/// Global provider registry.
static REGISTRY: LazyLock<ProviderRegistry> = LazyLock::new(ProviderRegistry::new);

/// Thread-safe registry for AI providers.
#[derive(Debug, Default)]
pub struct ProviderRegistry {
    providers: RwLock<HashMap<String, Arc<dyn Provider>>>,
}

impl ProviderRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
        }
    }

    /// Register a provider.
    ///
    /// If a provider with the same name already exists, it will be replaced.
    pub fn register(&self, provider: Arc<dyn Provider>) {
        let name = provider.name().to_string();
        debug!(provider = %name, "Registering provider");
        let mut providers = self.providers.write().unwrap();
        providers.insert(name, provider);
    }

    /// Get a provider by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Provider>> {
        debug!(provider = %name, "Looking up provider");
        let providers = self.providers.read().unwrap();
        providers.get(name).cloned()
    }

    /// Remove a provider by name.
    pub fn remove(&self, name: &str) -> Option<Arc<dyn Provider>> {
        let mut providers = self.providers.write().unwrap();
        providers.remove(name)
    }

    /// List all registered provider names.
    pub fn list(&self) -> Vec<String> {
        let providers = self.providers.read().unwrap();
        providers.keys().cloned().collect()
    }

    /// Check if a provider is registered.
    pub fn has(&self, name: &str) -> bool {
        let providers = self.providers.read().unwrap();
        providers.contains_key(name)
    }

    /// Clear all registered providers.
    pub fn clear(&self) {
        let mut providers = self.providers.write().unwrap();
        providers.clear();
    }
}

/// Register a provider in the global registry.
///
/// # Example
///
/// ```ignore
/// let provider = OpenAIProvider::from_env()?;
/// register_provider(Arc::new(provider));
/// ```
pub fn register_provider(provider: Arc<dyn Provider>) {
    REGISTRY.register(provider);
}

/// Get a provider by name from the global registry.
///
/// Returns `None` if the provider is not found.
///
/// # Example
///
/// ```ignore
/// if let Some(provider) = get_provider("openai") {
///     let model = provider.model("gpt-4o")?;
/// }
/// ```
pub fn get_provider(name: &str) -> Option<Arc<dyn Provider>> {
    REGISTRY.get(name)
}

/// Get a provider by name, returning an error if not found.
#[must_use = "this returns a Result that must be handled"]
pub fn require_provider(name: &str) -> Result<Arc<dyn Provider>, HyperError> {
    get_provider(name).ok_or_else(|| HyperError::ProviderNotFound(name.to_string()))
}

/// Remove a provider from the global registry.
pub fn remove_provider(name: &str) -> Option<Arc<dyn Provider>> {
    REGISTRY.remove(name)
}

/// List all registered provider names.
pub fn list_providers() -> Vec<String> {
    REGISTRY.list()
}

/// Check if a provider is registered.
pub fn has_provider(name: &str) -> bool {
    REGISTRY.has(name)
}

/// Get the global registry instance.
///
/// This is useful for advanced use cases where you need direct access
/// to the registry (e.g., for testing).
pub fn global_registry() -> &'static ProviderRegistry {
    &REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::ModelInfo;
    use crate::model::Model;
    use async_trait::async_trait;

    #[derive(Debug)]
    struct MockProvider {
        name: String,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        fn model(&self, _model_id: &str) -> Result<Arc<dyn Model>, HyperError> {
            Err(HyperError::ModelNotFound("mock".to_string()))
        }

        async fn list_models(&self) -> Result<Vec<ModelInfo>, HyperError> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_registry_basic() {
        let registry = ProviderRegistry::new();

        // Register a provider
        let provider = Arc::new(MockProvider {
            name: "test".to_string(),
        });
        registry.register(provider);

        // Get the provider
        let retrieved = registry.get("test");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "test");

        // List providers
        let names = registry.list();
        assert!(names.contains(&"test".to_string()));

        // Check has
        assert!(registry.has("test"));
        assert!(!registry.has("nonexistent"));

        // Remove provider
        let removed = registry.remove("test");
        assert!(removed.is_some());
        assert!(!registry.has("test"));
    }

    #[test]
    fn test_registry_replace() {
        let registry = ProviderRegistry::new();

        // Register provider
        registry.register(Arc::new(MockProvider {
            name: "test".to_string(),
        }));

        // Register again (should replace)
        registry.register(Arc::new(MockProvider {
            name: "test".to_string(),
        }));

        // Should still have exactly one
        assert_eq!(registry.list().len(), 1);
    }
}
