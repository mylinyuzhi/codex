use super::*;
use std::sync::Mutex;
use std::sync::atomic::AtomicI32;

#[tokio::test]
async fn test_retry_success_first_attempt() {
    let executor = RetryExecutor::new(RetryConfig::default());
    let attempts = AtomicI32::new(0);

    let result = executor
        .execute(|| {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, HyperError>(42) }
        })
        .await;

    assert_eq!(result.unwrap(), 42);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_retry_success_after_failures() {
    let config = RetryConfig::default()
        .with_max_attempts(5)
        .with_initial_backoff(Duration::from_millis(1));

    let executor = RetryExecutor::new(config);
    let attempts = AtomicI32::new(0);

    let result = executor
        .execute(|| {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if attempt < 3 {
                    Err(HyperError::NetworkError("connection failed".to_string()))
                } else {
                    Ok(42)
                }
            }
        })
        .await;

    assert_eq!(result.unwrap(), 42);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_retry_exhausted() {
    let config = RetryConfig::default()
        .with_max_attempts(3)
        .with_initial_backoff(Duration::from_millis(1));

    let executor = RetryExecutor::new(config);
    let attempts = AtomicI32::new(0);

    let result: Result<i32, HyperError> = executor
        .execute(|| {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err(HyperError::NetworkError("always fails".to_string())) }
        })
        .await;

    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_retry_non_retryable_error() {
    let config = RetryConfig::default()
        .with_max_attempts(5)
        .with_initial_backoff(Duration::from_millis(1));

    let executor = RetryExecutor::new(config);
    let attempts = AtomicI32::new(0);

    let result: Result<i32, HyperError> = executor
        .execute(|| {
            attempts.fetch_add(1, Ordering::SeqCst);
            async {
                // Auth errors are not retryable
                Err(HyperError::AuthenticationFailed("invalid key".to_string()))
            }
        })
        .await;

    assert!(result.is_err());
    // Should not retry auth errors
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_retry_no_retry_config() {
    let executor = RetryExecutor::new(RetryConfig::no_retry());
    let attempts = AtomicI32::new(0);

    let result: Result<i32, HyperError> = executor
        .execute(|| {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err(HyperError::NetworkError("fail".to_string())) }
        })
        .await;

    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_retry_respects_retry_after() {
    let config = RetryConfig::default()
        .with_max_attempts(2)
        .with_initial_backoff(Duration::from_secs(10)) // Long backoff
        .with_respect_retry_after(true);

    let executor = RetryExecutor::new(config);
    let attempts = AtomicI32::new(0);
    let start = std::time::Instant::now();

    let result: Result<i32, HyperError> = executor
        .execute(|| {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if attempt == 1 {
                    // Short retry-after should be used instead of long backoff
                    Err(HyperError::Retryable {
                        message: "rate limited".to_string(),
                        delay: Some(Duration::from_millis(10)),
                    })
                } else {
                    Ok(42)
                }
            }
        })
        .await;

    let elapsed = start.elapsed();
    assert_eq!(result.unwrap(), 42);
    // Should use short delay from retry-after, not 10 second backoff
    assert!(elapsed < Duration::from_secs(1));
}

#[derive(Debug)]
struct TestTelemetry {
    requests: Mutex<Vec<(i32, Option<http::StatusCode>, bool)>>,
    retries: Mutex<Vec<(i32, Duration)>>,
    exhausted: Mutex<Option<(i32, String)>>,
}

impl TestTelemetry {
    fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            retries: Mutex::new(Vec::new()),
            exhausted: Mutex::new(None),
        }
    }
}

impl RequestTelemetry for TestTelemetry {
    fn on_request(
        &self,
        attempt: i32,
        status: Option<http::StatusCode>,
        error: Option<&HyperError>,
        _duration: Duration,
    ) {
        self.requests
            .lock()
            .unwrap()
            .push((attempt, status, error.is_some()));
    }

    fn on_retry(&self, attempt: i32, delay: Duration) {
        self.retries.lock().unwrap().push((attempt, delay));
    }

    fn on_exhausted(&self, total_attempts: i32, final_error: &HyperError) {
        *self.exhausted.lock().unwrap() = Some((total_attempts, final_error.to_string()));
    }
}

#[tokio::test]
async fn test_retry_telemetry() {
    let config = RetryConfig::default()
        .with_max_attempts(3)
        .with_initial_backoff(Duration::from_millis(1));

    let telemetry = Arc::new(TestTelemetry::new());
    let executor = RetryExecutor::new(config).with_telemetry(telemetry.clone());

    let attempts = AtomicI32::new(0);
    let _: Result<i32, HyperError> = executor
        .execute(|| {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if attempt < 3 {
                    Err(HyperError::NetworkError("fail".to_string()))
                } else {
                    Ok(42)
                }
            }
        })
        .await;

    let requests = telemetry.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    // First two have errors
    assert!(requests[0].2);
    assert!(requests[1].2);
    // Third is success
    assert!(!requests[2].2);
    assert_eq!(requests[2].1, Some(http::StatusCode::OK));

    let retries = telemetry.retries.lock().unwrap();
    assert_eq!(retries.len(), 2); // Two retries before success

    // No exhausted call since we succeeded
    assert!(telemetry.exhausted.lock().unwrap().is_none());
}

#[tokio::test]
async fn test_retry_telemetry_exhausted() {
    let config = RetryConfig::default()
        .with_max_attempts(2)
        .with_initial_backoff(Duration::from_millis(1));

    let telemetry = Arc::new(TestTelemetry::new());
    let executor = RetryExecutor::new(config).with_telemetry(telemetry.clone());

    let _: Result<i32, HyperError> = executor
        .execute(|| async { Err(HyperError::NetworkError("fail".to_string())) })
        .await;

    let exhausted = telemetry.exhausted.lock().unwrap();
    assert!(exhausted.is_some());
    let (attempts, msg) = exhausted.as_ref().unwrap();
    assert_eq!(*attempts, 2);
    assert!(msg.contains("fail"));
}

#[test]
fn test_config_builder() {
    let config = RetryConfig::default()
        .with_max_attempts(5)
        .with_initial_backoff(Duration::from_millis(200))
        .with_max_backoff(Duration::from_secs(60))
        .with_backoff_multiplier(3.0)
        .with_jitter_ratio(0.2)
        .with_respect_retry_after(false);

    assert_eq!(config.max_attempts, 5);
    assert_eq!(config.initial_backoff, Duration::from_millis(200));
    assert_eq!(config.max_backoff, Duration::from_secs(60));
    assert_eq!(config.backoff_multiplier, 3.0);
    assert_eq!(config.jitter_ratio, 0.2);
    assert!(!config.respect_retry_after);
}

#[test]
fn test_jitter_ratio_clamped() {
    let config = RetryConfig::default().with_jitter_ratio(2.0);
    assert_eq!(config.jitter_ratio, 1.0);

    let config = RetryConfig::default().with_jitter_ratio(-0.5);
    assert_eq!(config.jitter_ratio, 0.0);
}

#[test]
fn test_simple_random_produces_valid_range() {
    for _ in 0..100 {
        let r = simple_random();
        assert!((0.0..=1.0).contains(&r));
    }
}
