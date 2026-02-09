use super::*;

#[test]
fn test_http_request_builder() {
    let request = HttpRequest::post("https://api.example.com/v1/chat")
        .with_header("Authorization", "Bearer token")
        .with_body(serde_json::json!({"message": "hello"}));

    assert_eq!(request.method, http::Method::POST);
    assert_eq!(request.url, "https://api.example.com/v1/chat");
    assert!(request.headers.contains_key("Authorization"));
    assert!(request.body.is_some());
}

#[test]
fn test_http_interceptor_context_builder() {
    let ctx = HttpInterceptorContext::with_provider("openai", "gpt-4o")
        .conversation_id("conv_123")
        .request_id("req_456");

    assert_eq!(ctx.provider_name, Some("openai".to_string()));
    assert_eq!(ctx.model, Some("gpt-4o".to_string()));
    assert_eq!(ctx.conversation_id, Some("conv_123".to_string()));
    assert_eq!(ctx.request_id, Some("req_456".to_string()));
}

#[test]
fn test_http_interceptor_context_metadata() {
    let mut ctx = HttpInterceptorContext::new();
    ctx.set_metadata("key", serde_json::json!("value"));

    assert_eq!(ctx.get_metadata("key"), Some(&serde_json::json!("value")));
    assert_eq!(ctx.get_metadata("nonexistent"), None);
}
