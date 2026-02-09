use super::*;

#[test]
fn test_retry_config_defaults() {
    let config = RetryConfig::default();
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.base_delay_ms, 1000);
    assert_eq!(config.max_delay_ms, 30000);
    assert_eq!(config.multiplier, 2.0);
}

#[test]
fn test_retry_config_builder() {
    let config = RetryConfig::default()
        .with_max_retries(5)
        .with_base_delay(Duration::from_millis(500))
        .with_max_delay(Duration::from_secs(60))
        .with_multiplier(1.5);

    assert_eq!(config.max_retries, 5);
    assert_eq!(config.base_delay_ms, 500);
    assert_eq!(config.max_delay_ms, 60000);
    assert_eq!(config.multiplier, 1.5);
}

#[test]
fn test_should_retry() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(RetryConfig::default().with_max_retries(3));

    // First attempt
    let error: ApiError = NetworkSnafu {
        message: "connection failed",
    }
    .build();
    assert!(ctx.should_retry(&error));
    assert_eq!(ctx.current_attempt(), 1);

    // Second attempt
    assert!(ctx.should_retry(&error));
    assert_eq!(ctx.current_attempt(), 2);

    // Third attempt
    assert!(ctx.should_retry(&error));
    assert_eq!(ctx.current_attempt(), 3);

    // Fourth attempt - should fail
    assert!(!ctx.should_retry(&error));
    assert_eq!(ctx.current_attempt(), 4);
}

#[test]
fn test_non_retryable_error() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::with_defaults();

    let error: ApiError = AuthenticationSnafu {
        message: "invalid key",
    }
    .build();
    assert!(!ctx.should_retry(&error));
}

#[test]
fn test_delay_calculation() {
    use crate::error::api_error::*;

    let ctx = RetryContext::new(
        RetryConfig::default()
            .with_base_delay(Duration::from_millis(100))
            .with_multiplier(2.0),
    );

    let error: ApiError = NetworkSnafu { message: "test" }.build();

    // Note: delay calculation uses current_attempt which starts at 0
    // After first should_retry, it becomes 1
    let mut ctx = ctx;
    ctx.current_attempt = 1;
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(100));

    ctx.current_attempt = 2;
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(200));

    ctx.current_attempt = 3;
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(400));
}

#[test]
fn test_delay_respects_max() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(
        RetryConfig::default()
            .with_base_delay(Duration::from_secs(10))
            .with_max_delay(Duration::from_secs(5)),
    );

    ctx.current_attempt = 1;
    let error: ApiError = NetworkSnafu { message: "test" }.build();
    // Should be capped at max_delay
    assert_eq!(ctx.calculate_delay(&error), Duration::from_secs(5));
}

#[test]
fn test_delay_honors_retry_after() {
    use crate::error::api_error::*;

    let mut ctx =
        RetryContext::new(RetryConfig::default().with_base_delay(Duration::from_secs(10)));
    ctx.current_attempt = 1;

    let error: ApiError = RateLimitedSnafu {
        message: "test",
        retry_after_ms: 2000i64,
    }
    .build();
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(2000));
}

#[test]
fn test_retry_decision() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(RetryConfig::default().with_max_retries(3));

    // Network error - should retry
    let error: ApiError = NetworkSnafu { message: "test" }.build();
    match ctx.decide(&error) {
        RetryDecision::Retry { .. } => {}
        _ => panic!("Expected Retry"),
    }

    // Reset for next test
    ctx.reset();

    // Auth error - should give up
    let error: ApiError = AuthenticationSnafu { message: "test" }.build();
    assert_eq!(ctx.decide(&error), RetryDecision::GiveUp);
}

#[test]
fn test_reset() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::with_defaults();

    let error: ApiError = NetworkSnafu { message: "test" }.build();
    ctx.should_retry(&error);
    ctx.should_retry(&error);
    assert_eq!(ctx.current_attempt(), 2);

    ctx.reset();
    assert_eq!(ctx.current_attempt(), 0);
    assert!(ctx.last_error().is_none());
}
