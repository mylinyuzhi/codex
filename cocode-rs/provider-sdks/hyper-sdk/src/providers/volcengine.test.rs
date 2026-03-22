use super::*;

#[test]
fn test_builder() {
    let result = VolcengineProvider::builder()
        .api_key("ark-test-key")
        .base_url("https://custom.ark.com")
        .timeout_secs(120)
        .build();

    assert!(result.is_ok());
    let provider = result.unwrap();
    assert_eq!(provider.name(), "volcengine");
    assert_eq!(provider.api_key(), "ark-test-key");
}

#[test]
fn test_builder_missing_key() {
    let result = VolcengineProvider::builder().build();
    assert!(result.is_err());
}

#[test]
fn test_volcengine_options_boxing() {
    let opts = VolcengineOptions::new()
        .with_thinking_budget(4096)
        .with_previous_response_id("resp_123")
        .boxed();

    let downcasted = downcast_options::<VolcengineOptions>(&opts);
    assert!(downcasted.is_some());
    assert_eq!(downcasted.unwrap().thinking_budget_tokens, Some(4096));
}
