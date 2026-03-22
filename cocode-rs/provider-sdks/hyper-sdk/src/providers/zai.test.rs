use super::*;

#[test]
fn test_builder() {
    let result = ZaiProvider::builder()
        .api_key("zai-test-key")
        .base_url("https://custom.zai.com")
        .timeout_secs(120)
        .build();

    assert!(result.is_ok());
    let provider = result.unwrap();
    assert_eq!(provider.name(), "zhipuai");
    assert_eq!(provider.api_key(), "zai-test-key");
}

#[test]
fn test_builder_zhipuai() {
    let result = ZaiProvider::builder()
        .api_key("zhipuai-test-key")
        .use_zhipuai(true)
        .build();

    assert!(result.is_ok());
    let provider = result.unwrap();
    assert_eq!(provider.name(), "zhipuai");
    assert!(provider.is_zhipuai());
}

#[test]
fn test_builder_missing_key() {
    let result = ZaiProvider::builder().build();
    assert!(result.is_err());
}

#[test]
fn test_zai_options_boxing() {
    let opts = ZaiOptions::new()
        .with_thinking_budget(8192)
        .with_request_id("req_123")
        .boxed();

    let downcasted = downcast_options::<ZaiOptions>(&opts);
    assert!(downcasted.is_some());
    assert_eq!(downcasted.unwrap().thinking_budget_tokens, Some(8192));
}
