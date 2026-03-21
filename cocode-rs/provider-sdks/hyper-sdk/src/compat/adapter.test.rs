use super::*;
use crate::hooks::LoggingHook;
use crate::hooks::ResponseIdHook;
use crate::providers::OpenAIProvider;

#[test]
fn test_hyper_adapter_creation() {
    let provider = OpenAIProvider::builder()
        .api_key("test-key")
        .build()
        .unwrap();

    let adapter = HyperAdapter::new(Arc::new(provider)).with_default_model("gpt-4o");

    assert_eq!(adapter.name(), "openai");
    assert_eq!(adapter.default_model_id(), Some("gpt-4o"));
    assert!(adapter.supports_previous_response_id());
}

#[test]
fn test_hyper_adapter_with_hooks() {
    let provider = OpenAIProvider::builder()
        .api_key("test-key")
        .build()
        .unwrap();

    let mut adapter = HyperAdapter::new(Arc::new(provider));
    adapter.add_request_hook(Arc::new(ResponseIdHook));
    adapter.add_request_hook(Arc::new(LoggingHook::info()));

    assert!(adapter.hooks().has_request_hooks());
    assert_eq!(adapter.hooks().request_hook_count(), 2);
}

#[test]
fn test_hyper_adapter_clone() {
    let provider = OpenAIProvider::builder()
        .api_key("test-key")
        .build()
        .unwrap();

    let mut adapter = HyperAdapter::new(Arc::new(provider)).with_default_model("gpt-4o");
    adapter.add_request_hook(Arc::new(ResponseIdHook));

    let cloned = adapter.clone();
    assert_eq!(cloned.name(), "openai");
    assert_eq!(cloned.default_model_id(), Some("gpt-4o"));
    assert!(cloned.hooks().has_request_hooks());
}

#[test]
fn test_supports_previous_response_id() {
    let openai = OpenAIProvider::builder()
        .api_key("test-key")
        .build()
        .unwrap();
    let adapter = HyperAdapter::new(Arc::new(openai));
    assert!(adapter.supports_previous_response_id());
}
