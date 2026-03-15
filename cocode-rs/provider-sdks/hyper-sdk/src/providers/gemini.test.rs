use super::*;

#[test]
fn test_builder() {
    let result = GeminiProvider::builder()
        .api_key("test-key")
        .base_url("https://custom.google.com")
        .timeout_secs(120)
        .build();

    assert!(result.is_ok());
    let provider = result.unwrap();
    assert_eq!(provider.name(), "gemini");
    assert_eq!(provider.api_key(), "test-key");
}

#[test]
fn test_builder_missing_key() {
    let result = GeminiProvider::builder().build();
    assert!(result.is_err());
}
