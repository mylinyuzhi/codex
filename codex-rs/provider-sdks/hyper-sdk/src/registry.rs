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
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (another thread panicked while holding it).
    pub fn register(&self, provider: Arc<dyn Provider>) {
        let name = provider.name().to_string();
        debug!(provider = %name, "Registering provider");
        let mut providers = self
            .providers
            .write()
            .expect("provider registry lock should not be poisoned");
        providers.insert(name, provider);
    }

    /// Get a provider by name.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (another thread panicked while holding it).
    pub fn get(&self, name: &str) -> Option<Arc<dyn Provider>> {
        debug!(provider = %name, "Looking up provider");
        let providers = self
            .providers
            .read()
            .expect("provider registry lock should not be poisoned");
        providers.get(name).cloned()
    }

    /// Remove a provider by name.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (another thread panicked while holding it).
    pub fn remove(&self, name: &str) -> Option<Arc<dyn Provider>> {
        let mut providers = self
            .providers
            .write()
            .expect("provider registry lock should not be poisoned");
        providers.remove(name)
    }

    /// List all registered provider names.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (another thread panicked while holding it).
    pub fn list(&self) -> Vec<String> {
        let providers = self
            .providers
            .read()
            .expect("provider registry lock should not be poisoned");
        providers.keys().cloned().collect()
    }

    /// Check if a provider is registered.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (another thread panicked while holding it).
    pub fn has(&self, name: &str) -> bool {
        let providers = self
            .providers
            .read()
            .expect("provider registry lock should not be poisoned");
        providers.contains_key(name)
    }

    /// Clear all registered providers.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (another thread panicked while holding it).
    pub fn clear(&self) {
        let mut providers = self
            .providers
            .write()
            .expect("provider registry lock should not be poisoned");
        providers.clear();
    }
}

/// Register a provider in the global registry.
///
/// # Deprecated
///
/// Use [`HyperClient::with_provider()`](crate::HyperClient::with_provider) instead
/// for better test isolation and multi-tenancy support.
///
/// # Example
///
/// ```ignore
/// // Old way (deprecated)
/// let provider = OpenAIProvider::from_env()?;
/// register_provider(Arc::new(provider));
///
/// // New way (recommended)
/// let client = HyperClient::new()
///     .with_provider(OpenAIProvider::from_env()?);
/// ```
#[deprecated(
    since = "0.2.0",
    note = "Use HyperClient::new().with_provider() instead for better test isolation"
)]
pub fn register_provider(provider: Arc<dyn Provider>) {
    REGISTRY.register(provider);
}

/// Get a provider by name from the global registry.
///
/// Returns `None` if the provider is not found.
///
/// # Deprecated
///
/// Use [`HyperClient::provider()`](crate::HyperClient::provider) instead.
///
/// # Example
///
/// ```ignore
/// // Old way (deprecated)
/// if let Some(provider) = get_provider("openai") {
///     let model = provider.model("gpt-4o")?;
/// }
///
/// // New way (recommended)
/// let client = HyperClient::from_env()?;
/// let model = client.model("openai", "gpt-4o")?;
/// ```
#[deprecated(since = "0.2.0", note = "Use HyperClient::provider() instead")]
pub fn get_provider(name: &str) -> Option<Arc<dyn Provider>> {
    REGISTRY.get(name)
}

/// Get a provider by name, returning an error if not found.
///
/// # Deprecated
///
/// Use [`HyperClient::require_provider()`](crate::HyperClient::require_provider) instead.
#[deprecated(since = "0.2.0", note = "Use HyperClient::require_provider() instead")]
#[must_use = "this returns a Result that must be handled"]
pub fn require_provider(name: &str) -> Result<Arc<dyn Provider>, HyperError> {
    #[allow(deprecated)]
    get_provider(name).ok_or_else(|| HyperError::ProviderNotFound(name.to_string()))
}

/// Remove a provider from the global registry.
///
/// # Deprecated
///
/// Use [`HyperClient::remove_provider()`](crate::HyperClient::remove_provider) instead.
#[deprecated(since = "0.2.0", note = "Use HyperClient::remove_provider() instead")]
pub fn remove_provider(name: &str) -> Option<Arc<dyn Provider>> {
    REGISTRY.remove(name)
}

/// List all registered provider names.
///
/// # Deprecated
///
/// Use [`HyperClient::list_providers()`](crate::HyperClient::list_providers) instead.
#[deprecated(since = "0.2.0", note = "Use HyperClient::list_providers() instead")]
pub fn list_providers() -> Vec<String> {
    REGISTRY.list()
}

/// Check if a provider is registered.
///
/// # Deprecated
///
/// Use [`HyperClient::has_provider()`](crate::HyperClient::has_provider) instead.
#[deprecated(since = "0.2.0", note = "Use HyperClient::has_provider() instead")]
pub fn has_provider(name: &str) -> bool {
    REGISTRY.has(name)
}

/// Get the global registry instance.
///
/// This is useful for advanced use cases where you need direct access
/// to the registry (e.g., for testing).
///
/// # Deprecated
///
/// Use [`HyperClient::registry()`](crate::HyperClient::registry) instead.
#[deprecated(since = "0.2.0", note = "Use HyperClient::registry() instead")]
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
