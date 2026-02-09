use super::*;

#[test]
fn test_plain_error() {
    let err = PlainError::new("test error", StatusCode::InvalidArguments);
    assert_eq!(err.status_code(), StatusCode::InvalidArguments);
    assert_eq!(err.to_string(), "test error");
    assert!(!err.is_retryable());
}

#[test]
fn test_plain_error_retryable() {
    let err = PlainError::new("network error", StatusCode::NetworkError);
    assert!(err.is_retryable());
}

#[test]
fn test_output_msg_hides_internal() {
    let err = PlainError::new("sensitive details", StatusCode::Internal);
    assert_eq!(err.output_msg(), "Internal error: 1001");
}

#[test]
fn test_output_msg_shows_user_errors() {
    let err = PlainError::new("Invalid parameter: foo", StatusCode::InvalidArguments);
    assert_eq!(err.output_msg(), "Invalid parameter: foo");
}

#[test]
fn test_boxed_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let boxed = boxed(io_err, StatusCode::FileNotFound);

    assert_eq!(boxed.status_code(), StatusCode::FileNotFound);
    assert!(boxed.source().is_some());
}
