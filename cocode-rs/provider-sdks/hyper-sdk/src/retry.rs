//! Retry configuration and execution.
//!
//! This module provides exponential backoff retry support with configurable
//! parameters and telemetry integration.
//!
//! # Example
//!
//! ```ignore
//! use hyper_sdk::retry::{RetryConfig, RetryExecutor};
//!
//! let config = RetryConfig::default()
//!     .with_max_attempts(5)
//!     .with_initial_backoff(Duration::from_millis(200));
//!
//! let executor = RetryExecutor::new(config);
//! let result = executor.execute(|| async {
//!     make_api_call().await
//! }).await;
//! ```

use crate::error::HyperError;
use crate::telemetry::RequestTelemetry;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Retry configuration with exponential backoff.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of attempts (1 = no retry).
    pub max_attempts: i32,
    /// Initial backoff delay.
    pub initial_backoff: Duration,
    /// Maximum backoff delay.
    pub max_backoff: Duration,
    /// Backoff multiplier.
    pub backoff_multiplier: f64,
    /// Jitter ratio (0.0-1.0).
    pub jitter_ratio: f64,
    /// Honor retry-after from error.
    pub respect_retry_after: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter_ratio: 0.1,
            respect_retry_after: true,
        }
    }
}

impl RetryConfig {
    /// Create a config that disables retries (single attempt).
    pub fn no_retry() -> Self {
        Self {
            max_attempts: 1,
            ..Default::default()
        }
    }

    /// Set the maximum number of attempts.
    pub fn with_max_attempts(mut self, attempts: i32) -> Self {
        self.max_attempts = attempts;
        self
    }

    /// Set the initial backoff delay.
    pub fn with_initial_backoff(mut self, backoff: Duration) -> Self {
        self.initial_backoff = backoff;
        self
    }

    /// Set the maximum backoff delay.
    pub fn with_max_backoff(mut self, max: Duration) -> Self {
        self.max_backoff = max;
        self
    }

    /// Set the backoff multiplier.
    pub fn with_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.backoff_multiplier = multiplier;
        self
    }

    /// Set the jitter ratio (0.0 to 1.0).
    pub fn with_jitter_ratio(mut self, ratio: f64) -> Self {
        self.jitter_ratio = ratio.clamp(0.0, 1.0);
        self
    }

    /// Set whether to respect retry-after from errors.
    pub fn with_respect_retry_after(mut self, respect: bool) -> Self {
        self.respect_retry_after = respect;
        self
    }
}

/// Retry executor with telemetry integration.
#[derive(Debug)]
pub struct RetryExecutor {
    config: RetryConfig,
    telemetry: Option<Arc<dyn RequestTelemetry>>,
}

impl RetryExecutor {
    /// Create a new retry executor with the given configuration.
    pub fn new(config: RetryConfig) -> Self {
        Self {
            config,
            telemetry: None,
        }
    }

    /// Add telemetry to the executor.
    pub fn with_telemetry(mut self, telemetry: Arc<dyn RequestTelemetry>) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// Execute an operation with retries.
    ///
    /// The operation is retried according to the configuration when it returns
    /// a retryable error (as determined by `HyperError::is_retryable()`).
    pub async fn execute<F, Fut, T>(&self, mut operation: F) -> Result<T, HyperError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, HyperError>>,
    {
        let mut attempt = 1;

        loop {
            let start = std::time::Instant::now();

            match operation().await {
                Ok(result) => {
                    if let Some(ref telemetry) = self.telemetry {
                        telemetry.on_request(
                            attempt,
                            Some(http::StatusCode::OK),
                            None,
                            start.elapsed(),
                        );
                    }
                    return Ok(result);
                }
                Err(error) => {
                    let duration = start.elapsed();

                    if let Some(ref telemetry) = self.telemetry {
                        telemetry.on_request(attempt, None, Some(&error), duration);
                    }

                    if !error.is_retryable() || attempt >= self.config.max_attempts {
                        if let Some(ref telemetry) = self.telemetry {
                            telemetry.on_exhausted(attempt, &error);
                        }
                        return Err(error);
                    }

                    let delay = self.calculate_delay(attempt, &error);

                    if let Some(ref telemetry) = self.telemetry {
                        telemetry.on_retry(attempt, delay);
                    }

                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    fn calculate_delay(&self, attempt: i32, error: &HyperError) -> Duration {
        // Honor retry-after if available
        if self.config.respect_retry_after {
            if let Some(delay) = error.retry_delay() {
                return delay.min(self.config.max_backoff);
            }
        }

        // Exponential backoff
        let base = self.config.initial_backoff.as_secs_f64()
            * self.config.backoff_multiplier.powi(attempt - 1);
        let base = base.min(self.config.max_backoff.as_secs_f64());

        // Apply jitter using a simple pseudo-random approach
        let jitter = base * self.config.jitter_ratio * simple_random();
        Duration::from_secs_f64(base + jitter)
    }
}

/// Simple pseudo-random number generator for jitter.
/// Returns a value between 0.0 and 1.0.
fn simple_random() -> f64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    // Use a combination of time and counter for basic randomness
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    let count = COUNTER.fetch_add(1, Ordering::Relaxed);

    // Simple hash-like mixing
    let mixed = now.wrapping_mul(0x517cc1b727220a95).wrapping_add(count);
    let mixed = mixed ^ (mixed >> 33);
    let mixed = mixed.wrapping_mul(0xc4ceb9fe1a85ec53);
    let mixed = mixed ^ (mixed >> 33);

    // Convert to 0.0-1.0 range
    (mixed as f64) / (u64::MAX as f64)
}

#[cfg(test)]
#[path = "retry.test.rs"]
mod tests;
