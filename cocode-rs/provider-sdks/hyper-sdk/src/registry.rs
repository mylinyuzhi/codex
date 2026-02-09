//! Global provider registry.

use crate::provider::Provider;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use tracing::debug;

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

#[cfg(test)]
#[path = "registry.test.rs"]
mod tests;
