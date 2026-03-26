use pretty_assertions::assert_eq;

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
fn test_is_overload_or_rate_limit() {
    assert!(
        OverloadedSnafu {
            message: "server overloaded",
            retry_after_ms: 1000i64
        }
        .build()
        .is_overload_or_rate_limit()
    );
    assert!(
        RateLimitedSnafu {
            message: "rate limited",
            retry_after_ms: 1000i64
        }
        .build()
        .is_overload_or_rate_limit()
    );
    // Other retryable errors must NOT trigger model fallback
    assert!(
        !NetworkSnafu { message: "timeout" }
            .build()
            .is_overload_or_rate_limit()
    );
    assert!(
        !StreamSnafu {
            message: "stream error"
        }
        .build()
        .is_overload_or_rate_limit()
    );
    assert!(
        !StreamIdleTimeoutSnafu {
            timeout_secs: 30i64
        }
        .build()
        .is_overload_or_rate_limit()
    );
    assert!(
        !AuthenticationSnafu { message: "bad key" }
            .build()
            .is_overload_or_rate_limit()
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
fn test_from_sdk_error_context_overflow() {
    let sdk_err = crate::AISdkError::new("Context too long, context length exceeded");
    let api_err: ApiError = sdk_err.into();
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
fn test_classify_sdk_error_auth() {
    let sdk_err = crate::AISdkError::new("invalid api key provided");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Authentication { .. }));
}

#[test]
fn test_classify_sdk_error_model_not_found() {
    let sdk_err = crate::AISdkError::new("model not found: gpt-5-turbo");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::InvalidRequest { .. }));
}

#[test]
fn test_classify_sdk_error_context_overflow() {
    let sdk_err = crate::AISdkError::new("maximum context length exceeded");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_classify_sdk_error_rate_limit() {
    let sdk_err = crate::AISdkError::new("rate limit exceeded, try again in 5s");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::RateLimited { .. }));
}

#[test]
fn test_classify_sdk_error_generic_server_error() {
    // "internal server error" now maps to Overloaded (P8: added "500" detection)
    let sdk_err = crate::AISdkError::new("500 internal server error");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Overloaded { .. }));
    assert!(api_err.is_retryable());
}

#[test]
fn test_classify_sdk_error_unknown_falls_to_sdk() {
    // Truly unknown errors still fall to SDK
    let sdk_err = crate::AISdkError::new("something completely unexpected happened");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Sdk { .. }));
}

#[test]
fn test_classify_sdk_error_maximum_context() {
    let sdk_err = crate::AISdkError::new("maximum context length exceeded");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_classify_sdk_error_max_tokens() {
    let sdk_err = crate::AISdkError::new("max_tokens must be less than context window");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_classify_sdk_error_tokens_exceeded() {
    let sdk_err = crate::AISdkError::new("128000 tokens exceeded for model");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

// =========================================================================
// P0: AISdkError variants that map through classify_sdk_error
// =========================================================================

#[test]
fn test_classify_sdk_error_provider_not_found_keyword() {
    // "does not exist" matches MODEL_KEYWORDS
    let sdk_err = crate::AISdkError::new("Provider not found: does not exist my-provider");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::InvalidRequest { .. }));
    assert_eq!(api_err.status_code(), StatusCode::InvalidArguments);
    assert!(!api_err.is_retryable());
}

#[test]
fn test_classify_sdk_error_model_not_found_keyword() {
    let sdk_err = crate::AISdkError::new("Model not found: gpt-99");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::InvalidRequest { .. }));
    assert_eq!(api_err.status_code(), StatusCode::InvalidArguments);
    assert!(!api_err.is_retryable());
}

// =========================================================================
// P1: Overloaded error maps correctly
// =========================================================================

#[test]
fn test_classify_sdk_error_overloaded() {
    let sdk_err = crate::AISdkError::new("server overloaded, 529");
    let api_err: ApiError = sdk_err.into();
    assert!(
        matches!(api_err, ApiError::Overloaded { .. }),
        "Overloaded keyword should map to Overloaded"
    );
    assert!(api_err.is_retryable());
    assert_eq!(api_err.retry_after(), Some(Duration::from_millis(1000)));
    assert_eq!(api_err.status_code(), StatusCode::ServiceUnavailable);
}

#[test]
fn test_classify_sdk_error_503() {
    let sdk_err = crate::AISdkError::new("503 service unavailable");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Overloaded { .. }));
    assert_eq!(
        api_err.retry_after(),
        Some(Duration::from_millis(1000)),
        "default delay should be 1000ms"
    );
}

#[test]
fn test_classify_sdk_error_network() {
    let sdk_err = crate::AISdkError::new("connection refused");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Network { .. }));
}

// =========================================================================
// W1: Secret scrubbing - AISdkError messages pass through classify
// =========================================================================

#[test]
fn test_classify_sdk_error_auth_message_preserved() {
    let sdk_err = crate::AISdkError::new("invalid api key for account");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Authentication { .. }));
    // The message is preserved through classification
    assert!(api_err.to_string().contains("invalid api key for account"));
}

#[test]
fn test_classify_sdk_error_stream_error() {
    let sdk_err = crate::AISdkError::new("stream error: connection reset");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Stream { .. }));
}

// =========================================================================
// P5: Cause-chain inspection for structured APICallError fields
// =========================================================================

fn make_api_call_error(
    message: &str,
    status_code: Option<u16>,
    retry_after: Option<Duration>,
) -> crate::AISdkError {
    let api_call = vercel_ai_provider::APICallError {
        message: message.to_string(),
        url: "https://api.example.com/v1/messages".to_string(),
        status_code,
        response_body: None,
        cause: None,
        is_retryable: status_code.is_some_and(|s| s == 429 || s >= 500),
        retry_after,
        data: None,
        request_body_values: None,
        response_headers: None,
    };
    let provider_err = vercel_ai_provider::ProviderError::ApiCall(api_call);
    crate::AISdkError::from(provider_err)
}

#[test]
fn test_classify_cause_chain_429_with_retry_after() {
    let sdk_err = make_api_call_error(
        "rate limit exceeded",
        Some(429),
        Some(Duration::from_secs(5)),
    );
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::RateLimited { .. }));
    assert!(api_err.is_retryable());
    // Should use the actual retry_after from APICallError, not hardcoded 1000ms
    assert_eq!(api_err.retry_after(), Some(Duration::from_secs(5)));
}

#[test]
fn test_classify_cause_chain_429_without_retry_after() {
    let sdk_err = make_api_call_error("too many requests", Some(429), None);
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::RateLimited { .. }));
    // Falls back to default 1000ms when no retry_after in APICallError
    assert_eq!(api_err.retry_after(), Some(Duration::from_millis(1000)));
}

#[test]
fn test_classify_cause_chain_401() {
    let sdk_err = make_api_call_error("invalid api key", Some(401), None);
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Authentication { .. }));
    assert!(!api_err.is_retryable());
}

#[test]
fn test_classify_cause_chain_500() {
    let sdk_err = make_api_call_error(
        "internal server error",
        Some(500),
        Some(Duration::from_secs(2)),
    );
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Overloaded { .. }));
    assert!(api_err.is_retryable());
    assert_eq!(api_err.retry_after(), Some(Duration::from_secs(2)));
}

#[test]
fn test_classify_cause_chain_502() {
    let sdk_err = make_api_call_error("bad gateway", Some(502), None);
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Overloaded { .. }));
    assert!(api_err.is_retryable());
}

#[test]
fn test_classify_cause_chain_503_with_retry_after() {
    let sdk_err = make_api_call_error(
        "service unavailable",
        Some(503),
        Some(Duration::from_secs(10)),
    );
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Overloaded { .. }));
    // Should use actual retry_after from APICallError
    assert_eq!(api_err.retry_after(), Some(Duration::from_secs(10)));
}

#[test]
fn test_classify_cause_chain_context_overflow_400() {
    let sdk_err = make_api_call_error("context length exceeded for model", Some(400), None);
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
    assert!(!api_err.is_retryable());
}

#[test]
fn test_classify_cause_chain_unknown_status_falls_to_heuristic() {
    // 418 is not specially handled, falls to message heuristic
    let sdk_err = make_api_call_error("model not found: gpt-99", Some(418), None);
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::InvalidRequest { .. }));
}

// =========================================================================
// P8: 500/502 server errors now detected via heuristic fallback
// =========================================================================

#[test]
fn test_classify_sdk_error_bad_gateway_heuristic() {
    let sdk_err = crate::AISdkError::new("502 bad gateway");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Overloaded { .. }));
    assert!(api_err.is_retryable());
}

// =========================================================================
// P13: Expanded context overflow detection patterns
// =========================================================================

#[test]
fn test_context_overflow_prompt_is_too_long() {
    let sdk_err = crate::AISdkError::new("prompt is too long for this model");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_context_overflow_maximum_prompt_length() {
    let sdk_err = crate::AISdkError::new("maximum prompt length exceeded");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_context_overflow_reduce_length_of_messages() {
    let sdk_err = crate::AISdkError::new("Please reduce the length of the messages");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_context_overflow_request_entity_too_large() {
    let sdk_err = crate::AISdkError::new("request entity too large");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_context_overflow_exceeds_available_context_size() {
    let sdk_err = crate::AISdkError::new("exceeds the available context size");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_context_overflow_exceeds_the_limit_of() {
    let sdk_err = crate::AISdkError::new("exceeds the limit of 128000 tokens");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_context_overflow_gemini_compound_pattern() {
    let sdk_err =
        crate::AISdkError::new("input token count of 200000 exceeds the maximum allowed for model");
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
}

#[test]
fn test_context_overflow_http_413() {
    let sdk_err = make_api_call_error("Request entity too large", Some(413), None);
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::ContextOverflow { .. }));
    assert!(!api_err.is_retryable());
}

// =========================================================================
// P14: OpenAI 404 treated as retryable network error
// =========================================================================

#[test]
fn test_classify_cause_chain_404_is_retryable() {
    let sdk_err = make_api_call_error("not found", Some(404), None);
    let api_err: ApiError = sdk_err.into();
    assert!(
        matches!(api_err, ApiError::Network { .. }),
        "404 should map to Network (retryable)"
    );
    assert!(api_err.is_retryable());
}

// =========================================================================
// P17: Response body extraction for error classification
// =========================================================================

fn make_api_call_error_with_body(
    message: &str,
    status_code: Option<u16>,
    response_body: Option<&str>,
) -> crate::AISdkError {
    let api_call = vercel_ai_provider::APICallError {
        message: message.to_string(),
        url: "https://api.example.com/v1/messages".to_string(),
        status_code,
        response_body: response_body.map(ToString::to_string),
        cause: None,
        is_retryable: false,
        retry_after: None,
        data: None,
        request_body_values: None,
        response_headers: None,
    };
    let provider_err = vercel_ai_provider::ProviderError::ApiCall(api_call);
    crate::AISdkError::from(provider_err)
}

#[test]
fn test_response_body_json_nested_error() {
    let sdk_err = make_api_call_error_with_body(
        "unknown error",
        Some(400),
        Some(r#"{"error": {"message": "context length exceeded"}}"#),
    );
    let api_err: ApiError = sdk_err.into();
    assert!(
        matches!(api_err, ApiError::ContextOverflow { .. }),
        "Should extract message from JSON response body and classify as overflow"
    );
}

#[test]
fn test_response_body_json_flat_error() {
    let sdk_err = make_api_call_error_with_body(
        "unknown error",
        Some(400),
        Some(r#"{"error": "rate limit exceeded"}"#),
    );
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::RateLimited { .. }));
}

#[test]
fn test_response_body_json_message_field() {
    let sdk_err = make_api_call_error_with_body(
        "unknown error",
        Some(400),
        Some(r#"{"message": "invalid api key"}"#),
    );
    let api_err: ApiError = sdk_err.into();
    assert!(matches!(api_err, ApiError::Authentication { .. }));
}

#[test]
fn test_response_body_html_gateway() {
    let sdk_err = make_api_call_error_with_body(
        "unknown error",
        Some(502),
        Some("<html><body>Bad Gateway</body></html>"),
    );
    let api_err: ApiError = sdk_err.into();
    // 502 is matched by status code before body extraction
    assert!(matches!(api_err, ApiError::Overloaded { .. }));
}

#[test]
fn test_response_body_html_gateway_unknown_status() {
    let sdk_err = make_api_call_error_with_body(
        "unknown error",
        Some(520),
        Some("<html><body>Gateway Error</body></html>"),
    );
    let api_err: ApiError = sdk_err.into();
    // 520 is not specially handled, falls to body extraction → "HTTP 520 gateway error"
    // "520" is not a recognized keyword, so it falls to Sdk catch-all
    assert!(matches!(api_err, ApiError::Sdk { .. }));
}

#[test]
fn test_response_body_html_gateway_with_500() {
    let sdk_err = make_api_call_error_with_body(
        "unknown error",
        Some(500),
        Some("<html><body>Internal Server Error</body></html>"),
    );
    let api_err: ApiError = sdk_err.into();
    // 500 is matched by status code before body extraction
    assert!(matches!(api_err, ApiError::Overloaded { .. }));
}

#[test]
fn test_response_body_no_body_falls_to_message() {
    let sdk_err = make_api_call_error_with_body("connection refused", Some(400), None);
    let api_err: ApiError = sdk_err.into();
    // No body, falls to classify_by_message("connection refused") → Network
    assert!(matches!(api_err, ApiError::Network { .. }));
}

// =========================================================================
// Context Overflow Info Parsing
// =========================================================================

use super::ContextOverflowInfo;
use super::parse_overflow_info;

#[test]
fn test_parse_overflow_anthropic() {
    let msg = "input length and `max_tokens` exceed context limit: 50000 + 4096 > 200000";
    let info = parse_overflow_info(msg);
    assert_eq!(
        info,
        ContextOverflowInfo {
            input_tokens: Some(50000),
            max_tokens: Some(4096),
            context_limit: Some(200000),
        }
    );
    assert!(info.has_recovery_info());
}

#[test]
fn test_parse_overflow_openai() {
    let msg = "This model's maximum context length is 128000 tokens. However, your messages resulted in 140000 tokens.";
    let info = parse_overflow_info(msg);
    assert_eq!(
        info,
        ContextOverflowInfo {
            context_limit: Some(128000),
            input_tokens: Some(140000),
            max_tokens: None,
        }
    );
    assert!(info.has_recovery_info());
}

#[test]
fn test_parse_overflow_gemini() {
    let msg = "input token count of 200000 exceeds the maximum allowed for model gemini-2.5-pro";
    let info = parse_overflow_info(msg);
    assert_eq!(info.input_tokens, Some(200000));
    // Gemini may or may not have a second number
}

#[test]
fn test_parse_overflow_gemini_with_limit() {
    let msg = "input token count of 200000 exceeds the maximum of 128000 for model";
    let info = parse_overflow_info(msg);
    assert_eq!(info.input_tokens, Some(200000));
    assert_eq!(info.context_limit, Some(128000));
}

#[test]
fn test_parse_overflow_unparseable() {
    let msg = "context length exceeded";
    let info = parse_overflow_info(msg);
    assert_eq!(info, ContextOverflowInfo::default());
    assert!(!info.has_recovery_info());
}

#[test]
fn test_overflow_info_from_api_error() {
    let err: ApiError = api_error::ContextOverflowSnafu {
        message: "input length and `max_tokens` exceed context limit: 80000 + 8192 > 200000",
    }
    .build();
    let info = err.overflow_info().expect("should parse");
    assert_eq!(info.input_tokens, Some(80000));
    assert_eq!(info.context_limit, Some(200000));
}

#[test]
fn test_overflow_info_non_overflow_error() {
    let err: ApiError = api_error::NetworkSnafu { message: "timeout" }.build();
    assert!(err.overflow_info().is_none());
}
