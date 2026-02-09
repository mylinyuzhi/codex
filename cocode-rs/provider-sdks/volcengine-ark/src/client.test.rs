use super::*;

#[test]
fn test_client_requires_api_key() {
    let result = Client::new(ClientConfig::default());
    assert!(matches!(result, Err(ArkError::Configuration(_))));
}

#[test]
fn test_client_with_api_key() {
    let result = Client::with_api_key("test-key");
    assert!(result.is_ok());
}

#[test]
fn test_parse_api_error_structured() {
    let body = r#"{"error":{"code":"invalid_request_error","message":"Invalid model"}}"#;
    let error = parse_api_error(400, body, None);
    assert!(matches!(error, ArkError::BadRequest(_)));
}

#[test]
fn test_parse_api_error_rate_limit() {
    let body = r#"{"error":{"code":"rate_limit_error","message":"Rate limited"}}"#;
    let error = parse_api_error(429, body, None);
    assert!(matches!(error, ArkError::RateLimited { .. }));
}

#[test]
fn test_parse_api_error_context_exceeded() {
    let body = r#"{"error":{"code":"context_length_exceeded","message":"Context too long"}}"#;
    let error = parse_api_error(400, body, None);
    assert!(matches!(error, ArkError::ContextWindowExceeded));
}
