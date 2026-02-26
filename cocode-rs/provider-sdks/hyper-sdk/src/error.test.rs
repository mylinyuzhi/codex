use super::*;

#[test]
fn test_error_display() {
    let err = HyperError::ProviderNotFound("openai".to_string());
    assert_eq!(err.to_string(), "provider not found: openai");

    let err = HyperError::UnsupportedCapability("Vision".to_string());
    assert_eq!(err.to_string(), "unsupported capability: Vision");

    let err = HyperError::ProviderError {
        code: "invalid_api_key".to_string(),
        message: "The API key is invalid".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "provider error: invalid_api_key: The API key is invalid"
    );
}

#[test]
fn test_is_retryable() {
    assert!(
        HyperError::Retryable {
            message: "rate limited".to_string(),
            delay: None,
        }
        .is_retryable()
    );
    assert!(HyperError::RateLimitExceeded("limit reached".to_string()).is_retryable());
    assert!(HyperError::NetworkError("connection refused".to_string()).is_retryable());

    assert!(!HyperError::AuthenticationFailed("invalid key".to_string()).is_retryable());
}

#[test]
fn test_retry_delay() {
    let err = HyperError::Retryable {
        message: "try again".to_string(),
        delay: Some(Duration::from_secs(5)),
    };
    assert_eq!(err.retry_delay(), Some(Duration::from_secs(5)));

    let err_no_delay = HyperError::Retryable {
        message: "try again".to_string(),
        delay: None,
    };
    assert_eq!(err_no_delay.retry_delay(), None);

    let other_err = HyperError::NetworkError("timeout".to_string());
    assert_eq!(other_err.retry_delay(), None);
}

#[test]
fn test_parse_retry_after() {
    // Seconds
    assert_eq!(
        parse_retry_after("try again in 5s"),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        parse_retry_after("Try Again In 10s"),
        Some(Duration::from_secs(10))
    );
    assert_eq!(
        parse_retry_after("try again in 2.5 seconds"),
        Some(Duration::from_secs_f64(2.5))
    );
    assert_eq!(
        parse_retry_after("try again in 1 second"),
        Some(Duration::from_secs(1))
    );

    // Milliseconds
    assert_eq!(
        parse_retry_after("try again in 500ms"),
        Some(Duration::from_millis(500))
    );

    // No match
    assert_eq!(parse_retry_after("some error message"), None);
    assert_eq!(parse_retry_after("rate limit exceeded"), None);
    assert_eq!(parse_retry_after(""), None);
}

#[test]
fn test_new_error_display() {
    let err = HyperError::StreamIdleTimeout(Duration::from_secs(60));
    assert!(err.to_string().contains("60"));

    let err = HyperError::PreviousResponseNotFound("resp_123".to_string());
    assert_eq!(err.to_string(), "previous response not found: resp_123");

    let err = HyperError::Retryable {
        message: "rate limited".to_string(),
        delay: Some(Duration::from_secs(5)),
    };
    assert_eq!(err.to_string(), "retryable error: rate limited");
}

// =========================================================================
// Comprehensive error scenario tests
// =========================================================================

#[test]
fn test_all_error_variants_display() {
    // Test that all error variants produce valid display strings
    let errors: Vec<HyperError> = vec![
        HyperError::ProviderNotFound("openai".into()),
        HyperError::ModelNotFound("gpt-5".into()),
        HyperError::UnsupportedCapability("Vision".to_string()),
        HyperError::AuthenticationFailed("invalid key".into()),
        HyperError::RateLimitExceeded("429".into()),
        HyperError::ContextWindowExceeded("too long".into()),
        HyperError::InvalidRequest("bad params".into()),
        HyperError::NetworkError("timeout".into()),
        HyperError::ProviderError {
            code: "500".into(),
            message: "internal error".into(),
        },
        HyperError::ParseError("invalid json".into()),
        HyperError::StreamError("stream closed".into()),
        HyperError::ConfigError("missing field".into()),
        HyperError::Internal("bug".into()),
        HyperError::Retryable {
            message: "retry".into(),
            delay: Some(Duration::from_secs(1)),
        },
        HyperError::PreviousResponseNotFound("resp_123".into()),
        HyperError::StreamIdleTimeout(Duration::from_secs(60)),
    ];

    for err in errors {
        let display = err.to_string();
        assert!(!display.is_empty(), "Error should have display: {err:?}");
    }
}

#[test]
fn test_retryable_classification_exhaustive() {
    // Retryable errors
    let retryable = [
        HyperError::Retryable {
            message: "temp".into(),
            delay: None,
        },
        HyperError::Retryable {
            message: "temp".into(),
            delay: Some(Duration::from_secs(1)),
        },
        HyperError::RateLimitExceeded("rate".into()),
        HyperError::NetworkError("net".into()),
    ];
    for err in retryable {
        assert!(err.is_retryable(), "Should be retryable: {err:?}");
    }

    // Non-retryable errors
    let non_retryable = [
        HyperError::ProviderNotFound("openai".into()),
        HyperError::ModelNotFound("gpt-5".into()),
        HyperError::UnsupportedCapability("Vision".to_string()),
        HyperError::AuthenticationFailed("auth".into()),
        HyperError::ContextWindowExceeded("ctx".into()),
        HyperError::InvalidRequest("req".into()),
        HyperError::ProviderError {
            code: "err".into(),
            message: "msg".into(),
        },
        HyperError::ParseError("parse".into()),
        HyperError::StreamError("stream".into()),
        HyperError::ConfigError("cfg".into()),
        HyperError::Internal("int".into()),
        HyperError::PreviousResponseNotFound("resp".into()),
        HyperError::StreamIdleTimeout(Duration::from_secs(60)),
    ];
    for err in non_retryable {
        assert!(!err.is_retryable(), "Should NOT be retryable: {err:?}");
    }
}

#[test]
fn test_retry_delay_only_from_retryable() {
    // Only Retryable variant with delay should return delay
    let with_delay = HyperError::Retryable {
        message: "retry".into(),
        delay: Some(Duration::from_millis(500)),
    };
    assert_eq!(with_delay.retry_delay(), Some(Duration::from_millis(500)));

    let without_delay = HyperError::Retryable {
        message: "retry".into(),
        delay: None,
    };
    assert_eq!(without_delay.retry_delay(), None);

    // All other errors should return None
    let other_errors: Vec<HyperError> = vec![
        HyperError::RateLimitExceeded("rate".into()),
        HyperError::NetworkError("net".into()),
    ];
    for err in other_errors {
        assert_eq!(
            err.retry_delay(),
            None,
            "Non-Retryable should return None: {err:?}"
        );
    }
}

#[test]
fn test_parse_retry_after_edge_cases() {
    // Valid formats
    assert_eq!(
        parse_retry_after("try again in 0s"),
        Some(Duration::from_secs(0))
    );
    assert_eq!(
        parse_retry_after("try again in 0.5s"),
        Some(Duration::from_secs_f64(0.5))
    );
    assert_eq!(
        parse_retry_after("TRY AGAIN IN 5S"),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        parse_retry_after("  try again in 5s  "),
        Some(Duration::from_secs(5))
    );

    // Invalid formats
    assert_eq!(parse_retry_after("try again in -5s"), None); // Negative
    assert_eq!(parse_retry_after("try again in 5h"), None); // Hours not supported
    assert_eq!(parse_retry_after("try again in 5m"), None); // Minutes not supported
    assert_eq!(parse_retry_after("retry in 5s"), None); // Different prefix
    assert_eq!(parse_retry_after("try again in s"), None); // No number
}

#[test]
fn test_error_from_reqwest() {
    // We can't easily create reqwest errors, but we can test the From implementation exists
    // by checking the error types are compatible
    fn assert_from<T: From<reqwest::Error>>() {}
    assert_from::<HyperError>();
}

#[test]
fn test_error_from_serde_json() {
    let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
    let hyper_err: HyperError = json_err.into();
    assert!(matches!(hyper_err, HyperError::ParseError(_)));
    assert!(hyper_err.to_string().contains("parse error"));
}

#[test]
fn test_rate_limit_is_retryable() {
    // All rate limits are retryable (aligned with Python SDKs)
    let rate = HyperError::RateLimitExceeded("too many requests".into());
    assert!(rate.is_retryable());
}

#[test]
fn test_context_window_exceeded_not_retryable() {
    // Context window errors are not retryable - need to reduce input
    let err = HyperError::ContextWindowExceeded("max 128k tokens".into());
    assert!(!err.is_retryable());
}

#[test]
fn test_stream_idle_timeout_not_retryable() {
    // Idle timeout is a local timeout, not a transient server error
    let err = HyperError::StreamIdleTimeout(Duration::from_secs(60));
    assert!(!err.is_retryable());
    assert!(err.to_string().contains("60"));
}

#[test]
fn test_previous_response_not_found() {
    let err = HyperError::PreviousResponseNotFound("resp_abc123".into());
    assert!(!err.is_retryable());
    assert!(err.to_string().contains("resp_abc123"));
}

#[test]
fn test_provider_error_with_special_characters() {
    let err = HyperError::ProviderError {
        code: "error_code_123".into(),
        message: "Message with \"quotes\" and 'apostrophes' and\nnewlines".into(),
    };
    let display = err.to_string();
    assert!(display.contains("error_code_123"));
    assert!(display.contains("quotes"));
}

// =========================================================================
// H1: Secret scrubbing tests
// =========================================================================

#[test]
fn test_scrub_secret_patterns_sk_prefix() {
    assert_eq!(
        scrub_secret_patterns("API key sk-abc123xyz is invalid"),
        "API key [REDACTED] is invalid"
    );
}

#[test]
fn test_scrub_secret_patterns_github_tokens() {
    assert_eq!(
        scrub_secret_patterns("Token ghp_abc123 failed"),
        "Token [REDACTED] failed"
    );
    assert_eq!(
        scrub_secret_patterns("Token gho_xyz789 failed"),
        "Token [REDACTED] failed"
    );
    assert_eq!(
        scrub_secret_patterns("Token ghu_abc123 failed"),
        "Token [REDACTED] failed"
    );
    assert_eq!(
        scrub_secret_patterns("Token github_pat_abc123 failed"),
        "Token [REDACTED] failed"
    );
}

#[test]
fn test_scrub_secret_patterns_slack_tokens() {
    assert_eq!(
        scrub_secret_patterns("Slack xoxb-123-456 error"),
        "Slack [REDACTED] error"
    );
    assert_eq!(
        scrub_secret_patterns("Slack xoxp-123-456 error"),
        "Slack [REDACTED] error"
    );
}

#[test]
fn test_scrub_secret_patterns_bearer() {
    assert_eq!(
        scrub_secret_patterns("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9 failed"),
        "Authorization: [REDACTED] failed"
    );
}

#[test]
fn test_scrub_secret_patterns_token_header() {
    assert_eq!(
        scrub_secret_patterns("Authorization: token abc123def456 failed"),
        "Authorization: [REDACTED] failed"
    );
}

#[test]
fn test_scrub_secret_patterns_no_match() {
    let input = "No secrets in this message";
    assert_eq!(scrub_secret_patterns(input), input);
}

#[test]
fn test_scrub_secret_patterns_multiple_tokens() {
    let result = scrub_secret_patterns("Keys sk-key1 and sk-key2 both failed");
    assert_eq!(result, "Keys [REDACTED] and [REDACTED] both failed");
}

#[test]
fn test_scrub_secret_patterns_empty() {
    assert_eq!(scrub_secret_patterns(""), "");
}

#[test]
fn test_sanitize_api_error_scrubs_and_truncates() {
    let msg = sanitize_api_error("Key sk-secret123 failed with error", 20);
    assert!(!msg.contains("sk-secret123"));
    assert!(msg.contains("[REDACTED]"));
}

#[test]
fn test_sanitize_api_error_no_truncation_needed() {
    let msg = sanitize_api_error("short", 100);
    assert_eq!(msg, "short");
}

#[test]
fn test_sanitize_api_error_truncation() {
    let long_msg = "a".repeat(200);
    let result = sanitize_api_error(&long_msg, 50);
    assert_eq!(result.len(), 53); // 50 + "..."
    assert!(result.ends_with("..."));
}

#[test]
fn test_sanitize_api_error_multibyte_utf8() {
    // CJK characters are 3 bytes each in UTF-8
    let msg = sanitize_api_error("错误信息：密钥无效", 5);
    assert!(msg.starts_with("错误信息："));
    assert!(msg.ends_with("..."));
    // Should not panic on multi-byte boundary
}

#[test]
fn test_sanitize_api_error_multibyte_with_scrub() {
    // Multi-byte chars + secret that gets scrubbed
    let msg = sanitize_api_error("错误密钥sk-abc123", 5);
    assert!(!msg.contains("sk-abc123"));
    assert!(msg.ends_with("..."));
}

#[test]
fn test_sanitize_api_error_emoji() {
    // Emoji are 4 bytes each in UTF-8 — must not panic
    let msg = sanitize_api_error("🔑🔐🔒🔓🗝️ secret", 3);
    assert!(msg.starts_with("🔑🔐🔒"));
    assert!(msg.ends_with("..."));
}

// =========================================================================
// M1: Improved retry-after parsing tests
// =========================================================================

#[test]
fn test_parse_retry_after_header_format() {
    // HTTP header format: "Retry-After: 5"
    assert_eq!(
        parse_retry_after("Retry-After: 5"),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        parse_retry_after("Retry-After: 10"),
        Some(Duration::from_secs(10))
    );
    assert_eq!(
        parse_retry_after("Retry-After: 2.5"),
        Some(Duration::from_secs_f64(2.5))
    );
}

#[test]
fn test_parse_retry_after_header_space_format() {
    // Space variant: "Retry-After 5"
    assert_eq!(
        parse_retry_after("Retry-After 5"),
        Some(Duration::from_secs(5))
    );
}

#[test]
fn test_parse_retry_after_json_field_format() {
    // JSON field format: "retry_after: 10"
    assert_eq!(
        parse_retry_after("retry_after: 10"),
        Some(Duration::from_secs(10))
    );
    assert_eq!(
        parse_retry_after("retry_after 5"),
        Some(Duration::from_secs(5))
    );
}

#[test]
fn test_parse_retry_after_case_insensitive() {
    assert_eq!(
        parse_retry_after("RETRY-AFTER: 5"),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        parse_retry_after("RETRY_AFTER: 5"),
        Some(Duration::from_secs(5))
    );
}

#[test]
fn test_parse_retry_after_capped_at_30s() {
    // Values > 30s should be capped
    assert_eq!(
        parse_retry_after("try again in 60s"),
        Some(Duration::from_secs(30))
    );
    assert_eq!(
        parse_retry_after("Retry-After: 120"),
        Some(Duration::from_secs(30))
    );
}

#[test]
fn test_parse_retry_after_within_message() {
    // Embedded in a longer error message
    assert_eq!(
        parse_retry_after("Rate limited. Retry-After: 5. Please wait."),
        Some(Duration::from_secs(5))
    );
}
