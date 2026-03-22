use super::*;

#[test]
fn test_client_requires_api_key() {
    let result = Client::new(ClientConfig::default());
    assert!(matches!(result, Err(AnthropicError::Configuration(_))));
}

#[test]
fn test_client_with_api_key() {
    let result = Client::with_api_key("test-key");
    assert!(result.is_ok());
}

#[test]
fn test_parse_api_error_structured() {
    let body = r#"{"error":{"type":"invalid_request_error","message":"Invalid model"}}"#;
    let error = parse_api_error(400, body, None);
    assert!(matches!(error, AnthropicError::BadRequest(_)));
}

#[test]
fn test_parse_api_error_rate_limit() {
    let body = r#"{"error":{"type":"rate_limit_error","message":"Rate limited"}}"#;
    let error = parse_api_error(429, body, None);
    assert!(matches!(error, AnthropicError::RateLimited { .. }));
}
