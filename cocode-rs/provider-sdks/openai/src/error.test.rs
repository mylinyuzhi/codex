use super::*;

#[test]
fn test_retryable_errors() {
    assert!(OpenAIError::RateLimited { retry_after: None }.is_retryable());
    assert!(OpenAIError::InternalServerError.is_retryable());
    assert!(
        OpenAIError::Api {
            status: 500,
            message: "error".to_string(),
            request_id: None
        }
        .is_retryable()
    );
    assert!(
        OpenAIError::Api {
            status: 429,
            message: "error".to_string(),
            request_id: None
        }
        .is_retryable()
    );
}

#[test]
fn test_non_retryable_errors() {
    assert!(!OpenAIError::Configuration("test".to_string()).is_retryable());
    assert!(!OpenAIError::Validation("test".to_string()).is_retryable());
    assert!(!OpenAIError::Authentication("test".to_string()).is_retryable());
    assert!(!OpenAIError::BadRequest("test".to_string()).is_retryable());
    assert!(
        !OpenAIError::Api {
            status: 400,
            message: "error".to_string(),
            request_id: None
        }
        .is_retryable()
    );
}
