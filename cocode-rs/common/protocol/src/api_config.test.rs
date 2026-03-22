use std::time::Duration;

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_api_retry_config_defaults() {
    assert_eq!(
        ApiRetryConfig::default(),
        ApiRetryConfig {
            max_retries: 5,
            base_delay_ms: 1000,
            max_delay_ms: 60000,
            multiplier: 2.0,
            jitter: 0.2,
        }
    );
}

#[test]
fn test_api_retry_config_serde() {
    let json = r#"{"max_retries": 3, "base_delay_ms": 500}"#;
    let config: ApiRetryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        config,
        ApiRetryConfig {
            max_retries: 3,
            base_delay_ms: 500,
            // Unspecified fields get defaults
            max_delay_ms: 60000,
            multiplier: 2.0,
            jitter: 0.2,
        }
    );
}

#[test]
fn test_api_fallback_config_defaults() {
    assert_eq!(
        ApiFallbackConfig::default(),
        ApiFallbackConfig {
            enable_stream_fallback: true,
            enable_overflow_recovery: true,
            fallback_max_tokens: Some(21333),
            min_output_tokens: 3000,
            max_overflow_attempts: 3,
            floor_output_tokens: 3000,
            buffer_tokens: 1000,
        }
    );
}

#[test]
fn test_api_fallback_config_serde() {
    let json = r#"{"floor_output_tokens": 2000, "buffer_tokens": 500}"#;
    let config: ApiFallbackConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.floor_output_tokens, 2000);
    assert_eq!(config.buffer_tokens, 500);
    // Unspecified fields get defaults
    assert!(config.enable_stream_fallback);
    assert_eq!(config.min_output_tokens, 3000);
}

#[test]
fn test_api_retry_config_no_retry() {
    let config = ApiRetryConfig::no_retry();
    assert_eq!(config.max_retries, 0);
    // Other fields still have defaults
    assert_eq!(config.base_delay_ms, DEFAULT_BASE_DELAY_MS);
}

#[test]
fn test_api_retry_config_builders() {
    assert_eq!(
        ApiRetryConfig::default()
            .with_max_retries(10)
            .with_base_delay(Duration::from_millis(500))
            .with_max_delay(Duration::from_secs(30))
            .with_multiplier(3.0)
            .with_jitter(0.5),
        ApiRetryConfig {
            max_retries: 10,
            base_delay_ms: 500,
            max_delay_ms: 30000,
            multiplier: 3.0,
            jitter: 0.5,
        }
    );
}

#[test]
fn test_api_retry_config_jitter_clamped() {
    let config = ApiRetryConfig::default().with_jitter(1.5);
    assert_eq!(config.jitter, 1.0);

    let config = ApiRetryConfig::default().with_jitter(-0.5);
    assert_eq!(config.jitter, 0.0);
}

#[test]
fn test_api_fallback_config_disabled() {
    let config = ApiFallbackConfig::disabled();
    assert!(!config.enable_stream_fallback);
    assert!(!config.enable_overflow_recovery);
    assert_eq!(config.fallback_max_tokens, None);
    assert_eq!(config.max_overflow_attempts, 0);
}

#[test]
fn test_api_fallback_config_builders() {
    assert_eq!(
        ApiFallbackConfig::default()
            .with_stream_fallback(false)
            .with_fallback_max_tokens(Some(10000))
            .with_overflow_recovery(false)
            .with_min_output_tokens(1000)
            .with_max_overflow_attempts(5),
        ApiFallbackConfig {
            enable_stream_fallback: false,
            fallback_max_tokens: Some(10000),
            enable_overflow_recovery: false,
            min_output_tokens: 1000,
            max_overflow_attempts: 5,
            // Unchanged from default
            floor_output_tokens: 3000,
            buffer_tokens: 1000,
        }
    );
}

#[test]
fn test_max_consecutive_overload_errors_constant() {
    assert_eq!(DEFAULT_MAX_CONSECUTIVE_OVERLOAD_ERRORS, 3);
}
