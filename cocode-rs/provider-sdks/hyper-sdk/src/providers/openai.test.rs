use super::*;

#[test]
fn test_builder() {
    let result = OpenAIProvider::builder()
        .api_key("sk-test-key")
        .base_url("https://custom.openai.com")
        .organization_id("org-123")
        .timeout_secs(120)
        .build();

    assert!(result.is_ok());
    let provider = result.unwrap();
    assert_eq!(provider.name(), "openai");
    assert_eq!(provider.api_key(), "sk-test-key");
    assert_eq!(provider.base_url(), "https://custom.openai.com");
}

#[test]
fn test_builder_missing_key() {
    let result = OpenAIProvider::builder().build();
    assert!(result.is_err());
}
