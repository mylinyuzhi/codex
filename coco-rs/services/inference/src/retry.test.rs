use std::time::Duration;

use super::*;

#[test]
fn test_exponential_backoff() {
    let config = RetryConfig {
        max_retries: 5,
        base_delay_ms: 1000,
        max_delay_ms: 60_000,
        jitter_factor: 0.0, // no jitter for deterministic tests
    };
    let err = InferenceError::NetworkError {
        message: "timeout".into(),
    };

    assert_eq!(
        config.delay_for_attempt(0, &err),
        Duration::from_millis(1000)
    ); // 1000 * 2^0
    assert_eq!(
        config.delay_for_attempt(1, &err),
        Duration::from_millis(2000)
    ); // 1000 * 2^1
    assert_eq!(
        config.delay_for_attempt(2, &err),
        Duration::from_millis(4000)
    ); // 1000 * 2^2
    assert_eq!(
        config.delay_for_attempt(3, &err),
        Duration::from_millis(8000)
    ); // 1000 * 2^3
}

#[test]
fn test_backoff_capped_at_max() {
    let config = RetryConfig {
        max_retries: 10,
        base_delay_ms: 1000,
        max_delay_ms: 5000,
        jitter_factor: 0.0,
    };
    let err = InferenceError::NetworkError {
        message: "timeout".into(),
    };

    // 1000 * 2^5 = 32000, but capped at 5000
    assert_eq!(
        config.delay_for_attempt(5, &err),
        Duration::from_millis(5000)
    );
}

#[test]
fn test_server_retry_after_takes_priority() {
    let config = RetryConfig::default();
    let err = InferenceError::RateLimited {
        retry_after_ms: Some(15000),
        message: "slow down".into(),
    };

    // Should use server's retry-after, not calculated backoff
    assert_eq!(
        config.delay_for_attempt(0, &err),
        Duration::from_millis(15000)
    );
}

#[test]
fn test_should_retry_within_limit() {
    let config = RetryConfig {
        max_retries: 3,
        ..Default::default()
    };
    let retryable = InferenceError::NetworkError {
        message: "err".into(),
    };
    let non_retryable = InferenceError::AuthenticationFailed {
        message: "err".into(),
    };

    assert!(config.should_retry(0, &retryable));
    assert!(config.should_retry(2, &retryable));
    assert!(!config.should_retry(3, &retryable)); // at limit
    assert!(!config.should_retry(0, &non_retryable)); // not retryable
}

#[test]
fn test_jitter_adds_delay() {
    let config = RetryConfig {
        max_retries: 3,
        base_delay_ms: 1000,
        max_delay_ms: 60_000,
        jitter_factor: 0.5,
    };
    let err = InferenceError::NetworkError {
        message: "err".into(),
    };

    // With 0.5 jitter on 1000ms base: delay = 1000 + 500 = 1500
    assert_eq!(
        config.delay_for_attempt(0, &err),
        Duration::from_millis(1500)
    );
}
