use super::*;

#[test]
fn test_retryable_errors() {
    assert!(ArkError::RateLimited { retry_after: None }.is_retryable());
    assert!(ArkError::InternalServerError.is_retryable());
    assert!(
        ArkError::Api {
            status: 500,
            message: "error".to_string(),
            request_id: None
        }
        .is_retryable()
    );
    assert!(
        ArkError::Api {
            status: 429,
            message: "error".to_string(),
            request_id: None
        }
        .is_retryable()
    );
}

#[test]
fn test_non_retryable_errors() {
    assert!(!ArkError::Configuration("test".to_string()).is_retryable());
    assert!(!ArkError::Validation("test".to_string()).is_retryable());
    assert!(!ArkError::Authentication("test".to_string()).is_retryable());
    assert!(!ArkError::BadRequest("test".to_string()).is_retryable());
    assert!(
        !ArkError::Api {
            status: 400,
            message: "error".to_string(),
            request_id: None
        }
        .is_retryable()
    );
}
