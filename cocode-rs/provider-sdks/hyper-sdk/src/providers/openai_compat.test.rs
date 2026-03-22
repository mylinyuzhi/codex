use super::*;

#[test]
fn test_builder() {
    let result = OpenAICompatProvider::builder("custom")
        .api_key("test-key")
        .base_url("https://custom-llm.example.com/v1")
        .timeout_secs(120)
        .build();

    assert!(result.is_ok());
    let provider = result.unwrap();
    assert_eq!(provider.name(), "custom");
    assert_eq!(provider.api_key(), "test-key");
    assert_eq!(provider.base_url(), "https://custom-llm.example.com/v1");
}

#[test]
fn test_builder_missing_url() {
    let result = OpenAICompatProvider::builder("custom")
        .api_key("test-key")
        .build();
    assert!(result.is_err());
}

#[test]
fn test_azure_constructor() {
    let result = OpenAICompatProvider::azure(
        "https://my-resource.openai.azure.com",
        "azure-key",
        "2024-02-15-preview",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "azure");
}

#[test]
fn test_local_constructor() {
    let result = OpenAICompatProvider::local("http://localhost:1234/v1");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name(), "local");
}

#[test]
fn test_groq_constructor() {
    let result = OpenAICompatProvider::groq("groq-key");
    assert!(result.is_ok());
    let provider = result.unwrap();
    assert_eq!(provider.name(), "groq");
    assert_eq!(provider.base_url(), "https://api.groq.com/openai/v1");
}
