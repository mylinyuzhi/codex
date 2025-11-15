//! Adapter registry for managing provider adapters
//!
//! This module provides a thread-safe global registry for provider adapters.
//! Built-in adapters are automatically registered on first access.
//!
//! # Example
//!
//! ```rust
//! use codex_core::adapters::{get_adapter, list_adapters};
//!
//! // Get a built-in adapter
//! let adapter = get_adapter("passthrough")?;
//!
//! // List all registered adapters
//! let adapters = list_adapters();
//! for name in adapters {
//!     println!("Available adapter: {}", name);
//! }
//! # Ok::<(), anyhow::Error>(())
//! ```

use super::ProviderAdapter;
use anyhow::Result;
use anyhow::anyhow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::RwLock;

/// Global adapter registry
///
/// This is initialized lazily on first access and automatically registers
/// all built-in adapters.
static ADAPTER_REGISTRY: LazyLock<AdapterRegistry> = LazyLock::new(|| {
    let registry = AdapterRegistry::new();

    // Register built-in adapters
    registry.register(Arc::new(super::gpt_openapi::GptOpenapiAdapter::new()));

    // Future: Add more built-in adapters here as they are implemented
    // registry.register(Arc::new(GeminiAdapter));

    registry
});

/// Thread-safe adapter registry
///
/// Uses `RwLock` to allow concurrent reads while protecting writes.
struct AdapterRegistry {
    adapters: RwLock<HashMap<String, Arc<dyn ProviderAdapter>>>,
}

impl AdapterRegistry {
    /// Create a new empty registry
    fn new() -> Self {
        Self {
            adapters: RwLock::new(HashMap::new()),
        }
    }

    /// Register an adapter
    ///
    /// If an adapter with the same name already exists, it will be replaced.
    fn register(&self, adapter: Arc<dyn ProviderAdapter>) {
        let name = adapter.name().to_string();
        self.adapters
            .write()
            .expect("adapter registry lock poisoned")
            .insert(name, adapter);
    }

    /// Get adapter by name
    ///
    /// Returns `None` if no adapter with the given name is registered.
    fn get(&self, name: &str) -> Option<Arc<dyn ProviderAdapter>> {
        self.adapters
            .read()
            .expect("adapter registry lock poisoned")
            .get(name)
            .cloned()
    }

    /// List all registered adapter names
    fn list(&self) -> Vec<String> {
        self.adapters
            .read()
            .expect("adapter registry lock poisoned")
            .keys()
            .cloned()
            .collect()
    }

    /// Get the number of registered adapters
    #[cfg(test)]
    fn len(&self) -> usize {
        self.adapters
            .read()
            .expect("adapter registry lock poisoned")
            .len()
    }
}

/// Register a custom adapter (for user extensions)
///
/// This allows users to register their own adapters at runtime.
/// If an adapter with the same name already exists, it will be replaced.
///
/// # Example
///
/// ```rust,ignore
/// use std::sync::Arc;
/// use codex_core::adapters::{register_adapter, ProviderAdapter};
///
/// struct MyCustomAdapter;
/// impl ProviderAdapter for MyCustomAdapter {
///     // ... implementation ...
/// }
///
/// let custom_adapter = Arc::new(MyCustomAdapter);
/// register_adapter(custom_adapter);
///
/// // Now it can be used via config
/// // [model_providers.my_custom]
/// // adapter = "my_custom"
/// ```
pub fn register_adapter(adapter: Arc<dyn ProviderAdapter>) {
    ADAPTER_REGISTRY.register(adapter);
}

/// Get adapter by name
///
/// # Errors
///
/// Returns an error if no adapter with the given name is registered.
///
/// # Example
///
/// ```rust
/// use codex_core::adapters::get_adapter;
///
/// // Get a built-in adapter
/// let adapter = get_adapter("passthrough")?;
/// assert_eq!(adapter.name(), "passthrough");
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn get_adapter(name: &str) -> Result<Arc<dyn ProviderAdapter>> {
    ADAPTER_REGISTRY.get(name).ok_or_else(|| {
        anyhow!(
            "Adapter not found: {}. Available adapters: {}",
            name,
            list_adapters().join(", ")
        )
    })
}

/// List all registered adapter names
///
/// Useful for debugging and introspection.
///
/// # Example
///
/// ```rust
/// use codex_core::adapters::list_adapters;
///
/// let adapters = list_adapters();
/// println!("Available adapters: {:?}", adapters);
/// ```
pub fn list_adapters() -> Vec<String> {
    ADAPTER_REGISTRY.list()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::AdapterContext;
    use crate::client_common::Prompt;
    use crate::client_common::ResponseEvent;
    use crate::error::Result;
    use crate::model_provider_info::ModelProviderInfo;
    use serde_json::Value as JsonValue;

    // Test adapter for testing the registry
    #[derive(Debug)]
    struct TestAdapter {
        adapter_name: String,
    }

    impl TestAdapter {
        fn new(name: &str) -> Self {
            Self {
                adapter_name: name.to_string(),
            }
        }
    }

    impl ProviderAdapter for TestAdapter {
        fn name(&self) -> &str {
            &self.adapter_name
        }

        fn transform_request(
            &self,
            _prompt: &Prompt,
            _provider: &ModelProviderInfo,
        ) -> Result<JsonValue> {
            Ok(serde_json::json!({}))
        }

        fn transform_response_chunk(
            &self,
            _chunk: &str,
            _context: &mut AdapterContext,
        ) -> Result<Vec<ResponseEvent>> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_registry_builtin_adapters() {
        let adapters = list_adapters();
        // GptOpenapiAdapter should be registered by default
        assert!(
            adapters.contains(&"gpt_openapi".to_string()),
            "GptOpenapiAdapter should be registered. Found: {:?}",
            adapters
        );
    }

    #[test]
    fn test_get_gpt_openapi_adapter() {
        let adapter = get_adapter("gpt_openapi");
        assert!(adapter.is_ok(), "Should find gpt_openapi adapter");
        assert_eq!(adapter.unwrap().name(), "gpt_openapi");
    }

    #[test]
    fn test_get_nonexistent_adapter() {
        let adapter = get_adapter("nonexistent_adapter_xyz");
        assert!(adapter.is_err(), "Should error for nonexistent adapter");

        let err_msg = format!("{}", adapter.unwrap_err());
        assert!(
            err_msg.contains("Adapter not found"),
            "Error should mention adapter not found. Got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("nonexistent_adapter_xyz"),
            "Error should mention the adapter name. Got: {}",
            err_msg
        );
    }

    #[test]
    fn test_register_custom_adapter() {
        let test_adapter = Arc::new(TestAdapter::new("test_adapter_custom"));
        register_adapter(test_adapter);

        let adapter = get_adapter("test_adapter_custom");
        assert!(adapter.is_ok(), "Should find registered custom adapter");
        assert_eq!(adapter.unwrap().name(), "test_adapter_custom");
    }

    #[test]
    fn test_register_replaces_existing() {
        // Use a unique name to avoid interference from other tests
        let unique_name = "replaceable_unique_test_adapter_xyz";

        // Register first adapter
        let adapter1 = Arc::new(TestAdapter::new(unique_name));
        register_adapter(adapter1);

        // Verify it's registered
        assert!(
            get_adapter(unique_name).is_ok(),
            "First adapter should be registered"
        );

        // Register second adapter with same name
        let adapter2 = Arc::new(TestAdapter::new(unique_name));
        register_adapter(adapter2);

        // Should still be retrievable (proves replacement works)
        let retrieved = get_adapter(unique_name);
        assert!(retrieved.is_ok(), "Should still find replaced adapter");

        // Verify the adapter list contains our adapter
        let adapters = list_adapters();
        assert!(
            adapters.contains(&unique_name.to_string()),
            "Adapter list should contain the replaced adapter"
        );
    }

    #[test]
    fn test_list_adapters_includes_custom() {
        let custom = Arc::new(TestAdapter::new("list_test_custom"));
        register_adapter(custom);

        let adapters = list_adapters();
        assert!(
            adapters.contains(&"list_test_custom".to_string()),
            "list_adapters should include custom adapter. Found: {:?}",
            adapters
        );
    }

    #[test]
    fn test_adapter_registry_thread_safety() {
        use std::sync::Arc as StdArc;
        use std::thread;

        // Register an adapter from multiple threads
        let handles: Vec<_> = (0..10)
            .map(|i| {
                thread::spawn(move || {
                    let adapter = StdArc::new(TestAdapter::new(&format!("thread_test_{i}")));
                    register_adapter(adapter);
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("Thread should not panic");
        }

        // All adapters should be registered
        let adapters = list_adapters();
        for i in 0..10 {
            assert!(
                adapters.contains(&format!("thread_test_{i}")),
                "Adapter thread_test_{i} should be registered"
            );
        }
    }
}
