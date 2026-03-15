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
    assert!(
        OverloadedSnafu {
            message: "test",
            retry_after_ms: 1000i64
        }
        .build()
        .is_retryable()
    );
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
fn test_retry_after_duration() {
    let err: ApiError = RateLimitedSnafu {
        message: "test",
        retry_after_ms: 5000i64,
    }
    .build();
    assert_eq!(err.retry_after(), Some(Duration::from_millis(5000)));

    let err: ApiError = NetworkSnafu { message: "test" }.build();
    assert_eq!(err.retry_after(), None);

    // Overloaded also carries retry_after
    let err: ApiError = OverloadedSnafu {
        message: "server busy",
        retry_after_ms: 3000i64,
    }
    .build();
    assert_eq!(err.retry_after(), Some(Duration::from_millis(3000)));
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
    assert_eq!(err.status_code(), StatusCode::ContextWindowExceeded);
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

// All 429s (including quota) are retryable (aligned with Python SDKs)
#[test]
fn test_all_rate_limited_retryable() {
    let err: ApiError = RateLimitedSnafu {
        message: "Rate limit exceeded. Please retry after 5s",
        retry_after_ms: 5000i64,
    }
    .build();
    assert!(err.is_retryable());

    // Even quota-like messages are retryable now
    let err: ApiError = RateLimitedSnafu {
        message: "You have insufficient balance to use this model",
        retry_after_ms: 1000i64,
    }
    .build();
    assert!(err.is_retryable());
}

// =========================================================================
// M2: Heuristic error classification tests
// =========================================================================

#[test]
fn test_classify_provider_error_auth() {
    let hyper_err = hyper_sdk::HyperError::ProviderError {
        code: "401".into(),
        message: "invalid api key provided".into(),
    };
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::Authentication { .. }));
}

#[test]
fn test_classify_provider_error_model_not_found() {
    let hyper_err = hyper_sdk::HyperError::ProviderError {
        code: "404".into(),
        message: "model not found: gpt-5-turbo".into(),
    };
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::InvalidRequest { .. }));
}

#[test]
fn test_classify_provider_error_context_overflow() {
    let hyper_err = hyper_sdk::HyperError::ProviderError {
        code: "400".into(),
        message: "maximum context length exceeded".into(),
    };
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_classify_provider_error_rate_limit() {
    let hyper_err = hyper_sdk::HyperError::ProviderError {
        code: "429".into(),
        message: "rate limit exceeded, try again in 5s".into(),
    };
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::RateLimited { .. }));
}

#[test]
fn test_classify_provider_error_generic() {
    let hyper_err = hyper_sdk::HyperError::ProviderError {
        code: "500".into(),
        message: "internal server error".into(),
    };
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::Provider { .. }));
}

#[test]
fn test_classify_provider_error_maximum_context() {
    let hyper_err = hyper_sdk::HyperError::ProviderError {
        code: "400".into(),
        message: "maximum context length exceeded".into(),
    };
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_classify_provider_error_max_tokens() {
    let hyper_err = hyper_sdk::HyperError::ProviderError {
        code: "400".into(),
        message: "max_tokens must be less than context window".into(),
    };
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_classify_provider_error_tokens_exceeded() {
    let hyper_err = hyper_sdk::HyperError::ProviderError {
        code: "400".into(),
        message: "128000 tokens exceeded for model".into(),
    };
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

// =========================================================================
// P0: HyperError variants that previously fell through to catch-all
// =========================================================================

#[test]
fn test_from_hyper_error_provider_not_found() {
    let hyper_err = hyper_sdk::HyperError::ProviderNotFound("my-provider".to_string());
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::InvalidRequest { .. }));
    assert_eq!(api_err.status_code(), StatusCode::InvalidArguments);
    assert!(!api_err.is_retryable());
    assert!(api_err.to_string().contains("Provider not found"));
}

#[test]
fn test_from_hyper_error_model_not_found() {
    let hyper_err = hyper_sdk::HyperError::ModelNotFound("gpt-99".to_string());
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::InvalidRequest { .. }));
    assert_eq!(api_err.status_code(), StatusCode::InvalidArguments);
    assert!(!api_err.is_retryable());
    assert!(api_err.to_string().contains("Model not found"));
}

// =========================================================================
// P1: HyperError::Retryable → Overloaded (not RateLimited)
// =========================================================================

#[test]
fn test_from_hyper_error_retryable_maps_to_overloaded() {
    let hyper_err = hyper_sdk::HyperError::Retryable {
        message: "server overloaded".to_string(),
        delay: Some(Duration::from_millis(2000)),
    };
    let api_err: ApiError = hyper_err.into();
    assert!(
        matches!(api_err, ApiError::Overloaded { .. }),
        "Retryable should map to Overloaded, not RateLimited"
    );
    assert!(api_err.is_retryable());
    assert_eq!(api_err.retry_after(), Some(Duration::from_millis(2000)));
    assert_eq!(api_err.status_code(), StatusCode::ServiceUnavailable);
}

#[test]
fn test_from_hyper_error_retryable_default_delay() {
    let hyper_err = hyper_sdk::HyperError::Retryable {
        message: "500 internal server error".to_string(),
        delay: None,
    };
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::Overloaded { .. }));
    assert_eq!(
        api_err.retry_after(),
        Some(Duration::from_millis(1000)),
        "default delay should be 1000ms"
    );
}

#[test]
fn test_from_hyper_error_config_error() {
    let hyper_err = hyper_sdk::HyperError::ConfigError("missing base_url".to_string());
    let api_err: ApiError = hyper_err.into();
    assert!(matches!(api_err, ApiError::Sdk { .. }));
    assert!(api_err.to_string().contains("Config error"));
}

// =========================================================================
// W1: Secret scrubbing at From<HyperError> boundary
// =========================================================================

#[test]
fn test_from_hyper_error_scrubs_secrets_network() {
    let hyper_err =
        hyper_sdk::HyperError::NetworkError("failed with key sk-secret123abc".to_string());
    let api_err: ApiError = hyper_err.into();
    let msg = api_err.to_string();
    assert!(
        !msg.contains("sk-secret123abc"),
        "secret should be scrubbed"
    );
    assert!(msg.contains("[REDACTED]"));
}

#[test]
fn test_from_hyper_error_scrubs_secrets_auth() {
    let hyper_err = hyper_sdk::HyperError::AuthenticationFailed(
        "invalid key sk-mykey456 for account".to_string(),
    );
    let api_err: ApiError = hyper_err.into();
    let msg = api_err.to_string();
    assert!(!msg.contains("sk-mykey456"), "secret should be scrubbed");
    assert!(msg.contains("[REDACTED]"));
}

#[test]
fn test_from_hyper_error_scrubs_secrets_provider() {
    let hyper_err = hyper_sdk::HyperError::ProviderError {
        code: "400".into(),
        message: "bad request with Bearer eyJtoken123 attached".into(),
    };
    let api_err: ApiError = hyper_err.into();
    let msg = api_err.to_string();
    assert!(
        !msg.contains("eyJtoken123"),
        "bearer token should be scrubbed"
    );
    assert!(msg.contains("[REDACTED]"));
}
