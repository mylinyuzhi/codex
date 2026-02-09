use super::*;
use crate::messages::Message;

#[test]
fn test_session_config_builder() {
    let config = SessionConfig::new()
        .temperature(0.7)
        .max_tokens(4096)
        .top_p(0.9);

    assert_eq!(config.temperature, Some(0.7));
    assert_eq!(config.max_tokens, Some(4096));
    assert_eq!(config.top_p, Some(0.9));
}

#[test]
fn test_merge_fills_empty_fields() {
    let config = SessionConfig::new().temperature(0.7).max_tokens(4096);

    let mut request = GenerateRequest::new(vec![Message::user("Hello")]);
    assert!(request.temperature.is_none());
    assert!(request.max_tokens.is_none());

    config.merge_into(&mut request);

    assert_eq!(request.temperature, Some(0.7));
    assert_eq!(request.max_tokens, Some(4096));
}

#[test]
fn test_merge_preserves_request_values() {
    let config = SessionConfig::new().temperature(0.7).max_tokens(4096);

    let mut request = GenerateRequest::new(vec![Message::user("Hello")])
        .temperature(0.3)
        .max_tokens(1000);

    config.merge_into(&mut request);

    // Request values should be preserved
    assert_eq!(request.temperature, Some(0.3));
    assert_eq!(request.max_tokens, Some(1000));
}

#[test]
fn test_merge_partial() {
    let config = SessionConfig::new().temperature(0.7).max_tokens(4096);

    let mut request = GenerateRequest::new(vec![Message::user("Hello")]).temperature(0.3);

    config.merge_into(&mut request);

    // Temperature from request, max_tokens from config
    assert_eq!(request.temperature, Some(0.3));
    assert_eq!(request.max_tokens, Some(4096));
}

#[test]
fn test_with_tools() {
    let tools = vec![ToolDefinition::new(
        "test",
        serde_json::json!({"type": "object"}),
    )];

    let config = SessionConfig::new().tools(tools.clone());

    let mut request = GenerateRequest::new(vec![Message::user("Hello")]);
    config.merge_into(&mut request);

    assert!(request.tools.is_some());
    assert_eq!(request.tools.as_ref().unwrap().len(), 1);
}

#[test]
fn test_is_empty() {
    let empty = SessionConfig::new();
    assert!(empty.is_empty());

    let with_temp = SessionConfig::new().temperature(0.5);
    assert!(!with_temp.is_empty());
}
