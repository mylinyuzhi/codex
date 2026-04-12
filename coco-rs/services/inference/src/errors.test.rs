use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_classify_401_as_auth() {
    let err = InferenceError::from_http_status(401, "Unauthorized", None);
    assert!(matches!(err, InferenceError::AuthenticationFailed { .. }));
    assert_eq!(err.error_class(), "auth");
    assert!(!err.is_retryable());
}

#[test]
fn test_classify_429_as_rate_limit() {
    let err = InferenceError::from_http_status(429, "Too Many Requests", Some(5000));
    assert!(matches!(err, InferenceError::RateLimited { .. }));
    assert_eq!(err.retry_after_ms(), Some(5000));
    assert!(err.is_retryable());
}

#[test]
fn test_classify_503_as_overloaded() {
    let err = InferenceError::from_http_status(503, "Service Unavailable", Some(30000));
    assert!(matches!(err, InferenceError::Overloaded { .. }));
    assert!(err.is_retryable());
}

#[test]
fn test_classify_400_context_overflow() {
    let err =
        InferenceError::from_http_status(400, r#"{"error": "context_length_exceeded"}"#, None);
    assert!(matches!(err, InferenceError::ContextWindowExceeded { .. }));
    assert!(!err.is_retryable());
}

#[test]
fn test_classify_400_normal() {
    let err = InferenceError::from_http_status(400, "Bad Request", None);
    assert!(matches!(err, InferenceError::InvalidRequest { .. }));
}

#[test]
fn test_classify_500_as_provider_error() {
    let err = InferenceError::from_http_status(500, "Internal Server Error", None);
    assert!(matches!(
        err,
        InferenceError::ProviderError { status: 500, .. }
    ));
    assert!(!err.is_retryable());
}

#[test]
fn test_network_error_is_retryable() {
    let err = InferenceError::NetworkError {
        message: "connection reset".into(),
    };
    assert!(err.is_retryable());
    assert_eq!(err.error_class(), "network");
}

#[test]
fn test_cancelled_not_retryable() {
    let err = InferenceError::Cancelled;
    assert!(!err.is_retryable());
    assert_eq!(err.error_class(), "cancelled");
}

#[test]
fn test_body_truncation() {
    let long_body = "x".repeat(1000);
    let err = InferenceError::from_http_status(500, &long_body, None);
    if let InferenceError::ProviderError { message, .. } = &err {
        assert!(message.len() <= 504); // 500 + "..."
        assert!(message.ends_with("..."));
    }
}
