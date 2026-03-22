use super::*;

#[test]
fn test_builder() {
    let result = AnthropicProvider::builder()
        .api_key("sk-ant-test-key")
        .base_url("https://custom.anthropic.com")
        .timeout_secs(120)
        .build();

    assert!(result.is_ok());
    let provider = result.unwrap();
    assert_eq!(provider.name(), "anthropic");
    assert_eq!(provider.api_key(), "sk-ant-test-key");
}

#[test]
fn test_builder_missing_key() {
    let result = AnthropicProvider::builder().build();
    assert!(result.is_err());
}
