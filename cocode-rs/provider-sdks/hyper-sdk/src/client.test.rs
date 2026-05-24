use super::*;

#[derive(Debug)]
struct MockProvider {
    name: String,
}

#[async_trait::async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn model(&self, model_id: &str) -> Result<Arc<dyn Model>, HyperError> {
        Err(HyperError::ModelNotFound(format!(
            "{}:{}",
            self.name, model_id
        )))
    }
}

#[test]
fn test_client_new() {
    let client = HyperClient::new();
    assert!(client.list_providers().is_empty());
}

#[test]
fn test_client_with_provider() {
    let client = HyperClient::new()
        .with_provider(MockProvider {
            name: "test1".to_string(),
        })
        .with_provider(MockProvider {
            name: "test2".to_string(),
        });

    assert!(client.has_provider("test1"));
    assert!(client.has_provider("test2"));
    assert!(!client.has_provider("test3"));
}

#[test]
fn test_client_register() {
    let client = HyperClient::new();
    client.register(MockProvider {
        name: "test".to_string(),
    });

    assert!(client.has_provider("test"));
}

#[test]
fn test_client_provider() {
    let client = HyperClient::new().with_provider(MockProvider {
        name: "test".to_string(),
    });

    assert!(client.provider("test").is_some());
    assert!(client.provider("nonexistent").is_none());
}

#[test]
fn test_client_require_provider() {
    let client = HyperClient::new().with_provider(MockProvider {
        name: "test".to_string(),
    });

    assert!(client.require_provider("test").is_ok());
    assert!(matches!(
        client.require_provider("nonexistent"),
        Err(HyperError::ProviderNotFound(_))
    ));
}

#[test]
fn test_client_model_not_found() {
    let client = HyperClient::new().with_provider(MockProvider {
        name: "test".to_string(),
    });

    // Provider exists but model doesn't
    let result = client.model("test", "gpt-4o");
    assert!(matches!(result, Err(HyperError::ModelNotFound(_))));

    // Provider doesn't exist
    let result = client.model("nonexistent", "gpt-4o");
    assert!(matches!(result, Err(HyperError::ProviderNotFound(_))));
}

#[test]
fn test_client_list_providers() {
    let client = HyperClient::new()
        .with_provider(MockProvider {
            name: "alpha".to_string(),
        })
        .with_provider(MockProvider {
            name: "beta".to_string(),
        });

    let providers = client.list_providers();
    assert_eq!(providers.len(), 2);
    assert!(providers.contains(&"alpha".to_string()));
    assert!(providers.contains(&"beta".to_string()));
}

#[test]
fn test_client_remove_provider() {
    let client = HyperClient::new().with_provider(MockProvider {
        name: "test".to_string(),
    });

    assert!(client.has_provider("test"));
    let removed = client.remove_provider("test");
    assert!(removed.is_some());
    assert!(!client.has_provider("test"));
}

#[test]
fn test_client_clear() {
    let client = HyperClient::new()
        .with_provider(MockProvider {
            name: "test1".to_string(),
        })
        .with_provider(MockProvider {
            name: "test2".to_string(),
        });

    assert_eq!(client.list_providers().len(), 2);
    client.clear();
    assert!(client.list_providers().is_empty());
}

#[test]
fn test_client_conversation() {
    // Can't create conversation for nonexistent provider
    let client = HyperClient::new();
    let result = client.conversation("openai", "gpt-4o");
    assert!(matches!(result, Err(HyperError::ProviderNotFound(_))));
}
