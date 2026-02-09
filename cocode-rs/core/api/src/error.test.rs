use super::api_error::*;
use super::*;

#[test]
fn test_error_retryable() {
    assert!(NetworkSnafu { message: "test" }.build().is_retryable());
    assert!(
        RateLimitedSnafu {
            message: "test",
            retry_after_ms: 1000i64
        }
        .build()
        .is_retryable()
    );
    assert!(OverloadedSnafu { message: "test" }.build().is_retryable());
    assert!(
        !AuthenticationSnafu { message: "test" }
            .build()
            .is_retryable()
    );
    assert!(
        !InvalidRequestSnafu { message: "test" }
            .build()
            .is_retryable()
    );
}

#[test]
fn test_retry_delay() {
    let err: ApiError = RateLimitedSnafu {
        message: "test",
        retry_after_ms: 5000i64,
    }
    .build();
    assert_eq!(err.retry_delay(), Some(Duration::from_millis(5000)));

    let err: ApiError = NetworkSnafu { message: "test" }.build();
    assert_eq!(err.retry_delay(), None);
}

#[test]
fn test_status_codes() {
    assert_eq!(
        NetworkSnafu { message: "test" }.build().status_code(),
        StatusCode::NetworkError
    );
    assert_eq!(
        AuthenticationSnafu { message: "test" }
            .build()
            .status_code(),
        StatusCode::AuthenticationFailed
    );
    assert_eq!(
        RateLimitedSnafu {
            message: "test",
            retry_after_ms: 1000i64
        }
        .build()
        .status_code(),
        StatusCode::RateLimited
    );
}

#[test]
fn test_context_overflow() {
    let err: ApiError = ContextOverflowSnafu {
        message: "max context exceeded",
    }
    .build();
    assert!(err.is_context_overflow());
    assert!(!err.is_retryable());
    assert_eq!(err.status_code(), StatusCode::InvalidArguments);
}

#[test]
fn test_is_stream_error() {
    let stream_err: ApiError = StreamSnafu {
        message: "stream failed",
    }
    .build();
    assert!(stream_err.is_stream_error());

    let timeout_err: ApiError = StreamIdleTimeoutSnafu {
        timeout_secs: 30i64,
    }
    .build();
    assert!(timeout_err.is_stream_error());

    let network_err: ApiError = NetworkSnafu {
        message: "network error",
    }
    .build();
    assert!(!network_err.is_stream_error());

    let rate_err: ApiError = RateLimitedSnafu {
        message: "rate limited",
        retry_after_ms: 1000i64,
    }
    .build();
    assert!(!rate_err.is_stream_error());

    let overflow_err: ApiError = ContextOverflowSnafu {
        message: "overflow",
    }
    .build();
    assert!(!overflow_err.is_stream_error());
}

#[test]
fn test_from_hyper_error_context_overflow() {
    let hyper_err = hyper_sdk::HyperError::ContextWindowExceeded("Context too long".to_string());
    let api_err: ApiError = hyper_err.into();
    assert!(api_err.is_context_overflow());
}
