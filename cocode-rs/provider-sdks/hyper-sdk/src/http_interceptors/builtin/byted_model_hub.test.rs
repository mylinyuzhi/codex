use super::*;

fn create_test_request() -> HttpRequest {
    HttpRequest::post("https://ark.cn-beijing.volces.com/api/v3/chat")
}

#[test]
fn test_intercept_adds_session_header() {
    let interceptor = BytedModelHubInterceptor::new();
    let mut request = create_test_request();
    let ctx = HttpInterceptorContext {
        conversation_id: Some("test-session-123".to_string()),
        model: Some("deepseek-v3".to_string()),
        provider_name: Some("byted-model-hub".to_string()),
        request_id: None,
        metadata: Default::default(),
    };

    interceptor.intercept(&mut request, &ctx);

    let extra_header = request
        .headers
        .get("extra")
        .expect("extra header should exist");
    let extra_str = extra_header.to_str().unwrap();
    let extra_json: serde_json::Value = serde_json::from_str(extra_str).unwrap();

    assert_eq!(extra_json["session_id"], "test-session-123");
}

#[test]
fn test_intercept_no_session_id() {
    let interceptor = BytedModelHubInterceptor::new();
    let mut request = create_test_request();
    let ctx = HttpInterceptorContext {
        conversation_id: None,
        model: Some("deepseek-v3".to_string()),
        provider_name: Some("byted-model-hub".to_string()),
        request_id: None,
        metadata: Default::default(),
    };

    interceptor.intercept(&mut request, &ctx);

    // Should not add header when conversation_id is None
    assert!(request.headers.get("extra").is_none());
}

#[test]
fn test_interceptor_name() {
    let interceptor = BytedModelHubInterceptor::new();
    assert_eq!(interceptor.name(), "byted_model_hub");
}

#[test]
fn test_interceptor_priority() {
    let interceptor = BytedModelHubInterceptor::new();
    assert_eq!(interceptor.priority(), 50);
}

#[test]
fn test_interceptor_default() {
    let interceptor = BytedModelHubInterceptor::default();
    assert_eq!(interceptor.name(), "byted_model_hub");
}
