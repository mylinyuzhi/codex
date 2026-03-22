//! Retry context for agent loop with exponential backoff.
//!
//! This module provides [`RetryContext`] with exponential backoff and
//! retry decisions for the vercel-ai based provider pipeline.
//!
//! The retry configuration type ([`ApiRetryConfig`]) lives in the protocol
//! crate as the single source of truth.

use crate::error::ApiError;
use cocode_error::ErrorExt;
use rand::Rng;
use std::time::Duration;

pub use cocode_protocol::ApiRetryConfig;

/// Backoff strategy determined by error type.
///
/// Not user-configurable — strategy is determined by error classification,
/// matching Claude Code's per-error-type retry behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryStrategy {
    /// `min(max_delay, base * multiplier^attempt)` — default for most errors.
    Exponential,
    /// `min(max_delay, base * attempt)` — for network errors (CC: `1000 * attempt`).
    Linear,
}

/// Retry context that tracks attempts and provides backoff calculation.
///
/// This context is used during a single request's retry cycle. It tracks
/// the number of attempts, calculates appropriate delays for retries,
/// and accumulates a diagnostics trail of all failures.
#[derive(Debug, Clone)]
pub struct RetryContext {
    config: ApiRetryConfig,
    current_attempt: i32,
    last_error: Option<String>,
    /// Accumulated failure details from each retry attempt.
    failures: Vec<String>,
    /// Optional provider context for diagnostics.
    provider_context: Option<String>,
    /// Consecutive overload errors (529/Overloaded).
    ///
    /// Reset to 0 on any non-overload error. Exposed for the loop driver
    /// to detect sustained overload and trigger fast-mode degradation.
    consecutive_overload_errors: i32,
}

impl RetryContext {
    /// Create a new retry context with the given configuration.
    pub fn new(config: ApiRetryConfig) -> Self {
        Self {
            config,
            current_attempt: 0,
            last_error: None,
            failures: Vec::new(),
            provider_context: None,
            consecutive_overload_errors: 0,
        }
    }

    /// Create a retry context with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ApiRetryConfig::default())
    }

    /// Set provider context for diagnostics (e.g., provider name).
    pub fn with_provider_context(mut self, name: &str) -> Self {
        self.provider_context = Some(name.to_string());
        self
    }

    /// Record an attempt and return if retry should be attempted.
    pub fn should_retry(&mut self, error: &ApiError) -> bool {
        self.current_attempt += 1;
        self.last_error = Some(error.to_string());

        // Track consecutive overload errors for fast-mode degradation
        if matches!(error, ApiError::Overloaded { .. }) {
            self.consecutive_overload_errors += 1;
        } else {
            self.consecutive_overload_errors = 0;
        }

        // Record failure in diagnostics trail
        let prefix = self
            .provider_context
            .as_ref()
            .map(|p| format!("[{p}] "))
            .unwrap_or_default();
        self.failures.push(format!(
            "{prefix}attempt {}/{}: {}",
            self.current_attempt, self.config.max_retries, error,
        ));

        // Check if retryable and within limits
        error.is_retryable() && self.current_attempt <= self.config.max_retries
    }

    /// Calculate the delay before the next retry.
    ///
    /// Uses per-error-type strategy (matching Claude Code):
    /// - **Network errors**: Linear backoff (`base_delay * attempt`)
    /// - **All other retryable errors**: Exponential backoff
    ///
    /// Applies random jitter (±`jitter` fraction) to prevent thundering-herd
    /// retries from concurrent requests.
    pub fn calculate_delay(&self, error: &ApiError) -> Duration {
        // Honor retry-after hint if available.
        // The provider's hint is authoritative — do not cap it with max_delay_ms.
        // Capping could cause premature retries while still rate-limited.
        if let Some(delay) = error.retry_after() {
            return delay;
        }

        let strategy = Self::strategy_for_error(error);
        let base = self.config.base_delay_ms as f64;
        let delay_ms = match strategy {
            RetryStrategy::Linear => base * self.current_attempt as f64,
            RetryStrategy::Exponential => {
                base * self.config.multiplier.powi(self.current_attempt - 1)
            }
        };
        let delay_ms = delay_ms.min(self.config.max_delay_ms as f64);

        // Apply jitter: delay * (1.0 ± jitter)
        let delay_ms = if self.config.jitter > 0.0 {
            let jitter = self.config.jitter;
            let factor = 1.0 + rand::rng().random_range(-jitter..jitter);
            (delay_ms * factor).max(0.0)
        } else {
            delay_ms
        };

        Duration::from_millis(delay_ms as u64)
    }

    /// Determine retry strategy based on error type.
    fn strategy_for_error(error: &ApiError) -> RetryStrategy {
        match error {
            ApiError::Network { .. } => RetryStrategy::Linear,
            _ => RetryStrategy::Exponential,
        }
    }

    /// Get consecutive overload error count.
    ///
    /// Used by the loop driver to detect sustained overload and trigger
    /// fast-mode degradation (matching Claude Code's `consecutive529Errors`).
    pub fn consecutive_overload_errors(&self) -> i32 {
        self.consecutive_overload_errors
    }

    /// Get the current attempt number.
    pub fn current_attempt(&self) -> i32 {
        self.current_attempt
    }

    /// Get the last error message.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Get the maximum retry attempts.
    pub fn max_retries(&self) -> i32 {
        self.config.max_retries
    }

    /// Get the accumulated diagnostics trail.
    pub fn diagnostics(&self) -> &[String] {
        &self.failures
    }

    /// Reset the context for a new request.
    pub fn reset(&mut self) {
        self.current_attempt = 0;
        self.last_error = None;
        self.failures.clear();
        self.consecutive_overload_errors = 0;
    }

    /// Check if retries are exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.current_attempt > self.config.max_retries
    }

    /// Create an exhausted error with full diagnostics trail.
    pub fn exhausted_error(&self) -> ApiError {
        crate::error::api_error::RetriesExhaustedSnafu {
            attempts: self.current_attempt,
            message: self
                .last_error
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            diagnostics: self.failures.clone(),
        }
        .build()
    }

    /// Make a retry decision based on the error.
    pub fn decide(&mut self, error: &ApiError) -> RetryDecision {
        if self.should_retry(error) {
            let delay = self.calculate_delay(error);
            RetryDecision::Retry { delay }
        } else {
            RetryDecision::GiveUp
        }
    }
}

/// Result of a retry decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision {
    /// Retry the request after the specified delay.
    Retry { delay: Duration },
    /// Give up and return the error.
    GiveUp,
}

#[cfg(test)]
#[path = "retry.test.rs"]
mod tests;
