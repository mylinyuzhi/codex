use super::*;
use crate::error::HyperError;
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
