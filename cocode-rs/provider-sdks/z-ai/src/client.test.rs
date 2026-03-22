use super::*;

#[test]
fn test_zai_client_requires_api_key() {
    let result = ZaiClient::new(ClientConfig::zai(""));
    assert!(result.is_err());
}

#[test]
fn test_zhipuai_client_requires_api_key() {
    let result = ZhipuAiClient::new(ClientConfig::zhipuai(""));
    assert!(result.is_err());
}

#[test]
fn test_zai_client_with_api_key() {
    let result = ZaiClient::with_api_key("test-key");
    assert!(result.is_ok());
}

#[test]
fn test_zhipuai_client_with_api_key() {
    let result = ZhipuAiClient::with_api_key("test-key");
    assert!(result.is_ok());
}
