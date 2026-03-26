use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_retry_config_defaults() {
    let config = ApiRetryConfig::default();
    assert_eq!(config.max_retries, 5);
    assert_eq!(config.base_delay_ms, 1000);
    assert_eq!(config.max_delay_ms, 60000);
    assert_eq!(config.multiplier, 2.0);
    assert_eq!(config.jitter, 0.2);
}

#[test]
fn test_retry_config_builder() {
    let config = ApiRetryConfig::default()
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

    let mut ctx = RetryContext::new(ApiRetryConfig::default().with_max_retries(3));

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
fn test_delay_calculation_exponential() {
    use crate::error::api_error::*;

    // Stream errors use exponential backoff and have no retry_after
    let mut ctx = RetryContext::new(
        ApiRetryConfig::default()
            .with_base_delay(Duration::from_millis(100))
            .with_multiplier(2.0)
            .with_jitter(0.0),
    );

    let error: ApiError = StreamSnafu { message: "test" }.build();

    ctx.current_attempt = 1;
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(100));

    ctx.current_attempt = 2;
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(200));

    ctx.current_attempt = 3;
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(400));
}

#[test]
fn test_delay_calculation_linear_for_network() {
    use crate::error::api_error::*;

    // Network errors use linear backoff: base * attempt
    let mut ctx = RetryContext::new(
        ApiRetryConfig::default()
            .with_base_delay(Duration::from_millis(1000))
            .with_jitter(0.0),
    );

    let error: ApiError = NetworkSnafu { message: "test" }.build();

    ctx.current_attempt = 1;
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(1000));

    ctx.current_attempt = 2;
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(2000));

    ctx.current_attempt = 3;
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(3000));
}

#[test]
fn test_delay_respects_max() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(
        ApiRetryConfig::default()
            .with_base_delay(Duration::from_secs(10))
            .with_max_delay(Duration::from_secs(5))
            .with_jitter(0.0),
    );

    ctx.current_attempt = 1;
    // Stream error (exponential) — 10s base exceeds 5s max → capped
    let error: ApiError = StreamSnafu { message: "test" }.build();
    assert_eq!(ctx.calculate_delay(&error), Duration::from_secs(5));
}

#[test]
fn test_delay_honors_retry_after() {
    use crate::error::api_error::*;

    let mut ctx =
        RetryContext::new(ApiRetryConfig::default().with_base_delay(Duration::from_secs(10)));
    ctx.current_attempt = 1;

    let error: ApiError = RateLimitedSnafu {
        message: "test",
        retry_after_ms: 2000i64,
    }
    .build();
    assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(2000));
}

#[test]
fn test_delay_retry_after_exceeding_max_delay_not_capped() {
    use crate::error::api_error::*;

    // Provider says "retry after 120s" but max_delay_ms is 60s.
    // The provider hint is authoritative and must NOT be capped.
    let mut ctx = RetryContext::new(
        ApiRetryConfig::default()
            .with_max_delay(Duration::from_secs(60))
            .with_jitter(0.0),
    );
    ctx.current_attempt = 1;

    let error: ApiError = RateLimitedSnafu {
        message: "test",
        retry_after_ms: 120_000i64,
    }
    .build();
    assert_eq!(ctx.calculate_delay(&error), Duration::from_secs(120));
}

#[test]
fn test_retry_decision() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(ApiRetryConfig::default().with_max_retries(3));

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
    assert_eq!(ctx.diagnostics().len(), 2);

    ctx.reset();
    assert_eq!(ctx.current_attempt(), 0);
    assert!(ctx.last_error().is_none());
    assert!(ctx.diagnostics().is_empty());
}

// =========================================================================
// H3: Failure diagnostics trail tests
// =========================================================================

#[test]
fn test_diagnostics_trail_accumulates() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(ApiRetryConfig::default().with_max_retries(3));
    let error: ApiError = NetworkSnafu {
        message: "connection refused",
    }
    .build();

    ctx.should_retry(&error);
    ctx.should_retry(&error);
    ctx.should_retry(&error);

    let diagnostics = ctx.diagnostics();
    assert_eq!(diagnostics.len(), 3);
    assert!(diagnostics[0].contains("attempt 1/3"));
    assert!(diagnostics[1].contains("attempt 2/3"));
    assert!(diagnostics[2].contains("attempt 3/3"));
    assert!(diagnostics[0].contains("connection refused"));
}

#[test]
fn test_diagnostics_trail_with_provider_context() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(ApiRetryConfig::default().with_max_retries(2))
        .with_provider_context("openai");
    let error: ApiError = NetworkSnafu { message: "timeout" }.build();

    ctx.should_retry(&error);
    let diagnostics = ctx.diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].contains("[openai]"));
    assert!(diagnostics[0].contains("attempt 1/2"));
}

#[test]
fn test_exhausted_error_includes_diagnostics() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(ApiRetryConfig::default().with_max_retries(2));
    let error: ApiError = NetworkSnafu {
        message: "connection failed",
    }
    .build();

    ctx.should_retry(&error);
    ctx.should_retry(&error);
    ctx.should_retry(&error); // exceeds max

    let exhausted = ctx.exhausted_error();
    assert!(matches!(exhausted, ApiError::RetriesExhausted { .. }));
    let diags = exhausted.diagnostics();
    assert_eq!(diags.len(), 3);
}

#[test]
fn test_diagnostics_empty_initially() {
    let ctx = RetryContext::with_defaults();
    assert!(ctx.diagnostics().is_empty());
}

#[test]
fn test_diagnostics_non_retryable_still_recorded() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::with_defaults();
    let error: ApiError = AuthenticationSnafu {
        message: "invalid key",
    }
    .build();

    // Even though should_retry returns false, the failure is still recorded
    let retryable = ctx.should_retry(&error);
    assert!(!retryable);
    assert_eq!(ctx.diagnostics().len(), 1);
    assert!(ctx.diagnostics()[0].contains("invalid key"));
}

// =========================================================================
// P23: Jitter tests
// =========================================================================

#[test]
fn test_jitter_config_builder() {
    let config = ApiRetryConfig::default().with_jitter(0.3);
    assert_eq!(config.jitter, 0.3);
}

#[test]
fn test_jitter_clamped() {
    let config = ApiRetryConfig::default().with_jitter(1.5);
    assert_eq!(config.jitter, 1.0);

    let config = ApiRetryConfig::default().with_jitter(-0.5);
    assert_eq!(config.jitter, 0.0);
}

#[test]
fn test_jitter_delay_within_expected_range() {
    use crate::error::api_error::*;

    let base_ms = 1000;
    let jitter = 0.2;
    let mut ctx = RetryContext::new(
        ApiRetryConfig::default()
            .with_base_delay(Duration::from_millis(base_ms))
            .with_multiplier(1.0)
            .with_jitter(jitter),
    );
    ctx.current_attempt = 1;

    let error: ApiError = NetworkSnafu { message: "test" }.build();
    let min_ms = (base_ms as f64 * (1.0 - jitter)) as u64;
    let max_ms = (base_ms as f64 * (1.0 + jitter)) as u64;

    // Run multiple samples — all should fall within the expected range
    for _ in 0..50 {
        let delay = ctx.calculate_delay(&error);
        let ms = delay.as_millis() as u64;
        assert!(
            ms >= min_ms && ms <= max_ms,
            "delay {ms}ms outside [{min_ms}, {max_ms}]",
        );
    }
}

#[test]
fn test_zero_jitter_is_deterministic() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(
        ApiRetryConfig::default()
            .with_base_delay(Duration::from_millis(500))
            .with_multiplier(2.0)
            .with_jitter(0.0),
    );
    ctx.current_attempt = 2;

    // Linear: 500 * 2 = 1000
    let error: ApiError = NetworkSnafu { message: "test" }.build();
    for _ in 0..10 {
        assert_eq!(ctx.calculate_delay(&error), Duration::from_millis(1000));
    }
}

// =========================================================================
// Overload counter tests
// =========================================================================

#[test]
fn test_consecutive_overload_counter() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(ApiRetryConfig::default());

    // Overloaded errors increment counter
    let overload: ApiError = OverloadedSnafu {
        message: "overloaded",
        retry_after_ms: 1000i64,
    }
    .build();
    ctx.should_retry(&overload);
    assert_eq!(ctx.consecutive_overload_errors(), 1);
    ctx.should_retry(&overload);
    assert_eq!(ctx.consecutive_overload_errors(), 2);

    // Non-overload error resets counter
    let network: ApiError = NetworkSnafu { message: "timeout" }.build();
    ctx.should_retry(&network);
    assert_eq!(ctx.consecutive_overload_errors(), 0);

    // Back to overload
    ctx.should_retry(&overload);
    assert_eq!(ctx.consecutive_overload_errors(), 1);
}

#[test]
fn test_reset_clears_overload_counter() {
    use crate::error::api_error::*;

    let mut ctx = RetryContext::new(ApiRetryConfig::default());
    let overload: ApiError = OverloadedSnafu {
        message: "overloaded",
        retry_after_ms: 0i64,
    }
    .build();
    ctx.should_retry(&overload);
    ctx.should_retry(&overload);
    assert_eq!(ctx.consecutive_overload_errors(), 2);

    ctx.reset();
    assert_eq!(ctx.consecutive_overload_errors(), 0);
}
