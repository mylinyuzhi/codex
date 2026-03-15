//! Prepare retry configuration for API calls.
//!
//! This module provides utilities for configuring retry behavior
//! for transient failures.

use std::time::Duration;

/// Retry configuration for API calls.
#[derive(Debug, Clone)]
pub struct RetrySettings {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Initial delay before first retry.
    pub initial_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Multiplier for exponential backoff.
    pub backoff_multiplier: f64,
    /// HTTP status codes that should trigger a retry.
    pub retryable_status_codes: Vec<u16>,
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            retryable_status_codes: vec![429, 500, 502, 503, 504],
        }
    }
}

impl RetrySettings {
    /// Create new retry settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum number of retries.
    pub fn with_max_retries(mut self, max: u32) -> Self {
        self.max_retries = max;
        self
    }

    /// Set the initial delay.
    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    /// Set the maximum delay.
    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Set the backoff multiplier.
    pub fn with_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.backoff_multiplier = multiplier;
        self
    }

    /// Add a retryable status code.
    pub fn with_retryable_status(mut self, code: u16) -> Self {
        if !self.retryable_status_codes.contains(&code) {
            self.retryable_status_codes.push(code);
        }
        self
    }

    /// Calculate the delay for a given attempt.
    ///
    /// # Arguments
    ///
    /// * `attempt` - The attempt number (0-indexed).
    ///
    /// # Returns
    ///
    /// The delay before the next retry.
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let delay_ms =
            self.initial_delay.as_millis() as f64 * self.backoff_multiplier.powi(attempt as i32);

        let delay = Duration::from_millis(delay_ms as u64);
        delay.min(self.max_delay)
    }

    /// Check if a status code is retryable.
    pub fn is_retryable_status(&self, status: u16) -> bool {
        self.retryable_status_codes.contains(&status)
    }

    /// Check if retries are exhausted.
    pub fn is_exhausted(&self, attempts: u32) -> bool {
        attempts >= self.max_retries
    }
}

/// Prepare retry settings from options.
///
/// # Arguments
///
/// * `max_retries` - Optional maximum retries.
/// * `initial_delay_ms` - Optional initial delay in milliseconds.
///
/// # Returns
///
/// Configured `RetrySettings`.
pub fn prepare_retries(max_retries: Option<u32>, initial_delay_ms: Option<u64>) -> RetrySettings {
    let mut settings = RetrySettings::new();

    if let Some(max) = max_retries {
        settings = settings.with_max_retries(max);
    }

    if let Some(delay) = initial_delay_ms {
        settings = settings.with_initial_delay(Duration::from_millis(delay));
    }

    settings
}

/// Prepare retry settings for a specific provider.
///
/// Different providers may have different retry recommendations.
///
/// # Arguments
///
/// * `provider` - The provider name.
///
/// # Returns
///
/// Provider-specific `RetrySettings`.
pub fn prepare_provider_retries(provider: &str) -> RetrySettings {
    match provider.to_lowercase().as_str() {
        "anthropic" => RetrySettings::new()
            .with_max_retries(2)
            .with_initial_delay(Duration::from_millis(500))
            .with_retryable_status(529), // Anthropic overload
        "openai" => RetrySettings::new()
            .with_max_retries(3)
            .with_initial_delay(Duration::from_millis(200)),
        "google" | "google-genai" => RetrySettings::new()
            .with_max_retries(3)
            .with_initial_delay(Duration::from_millis(1000)),
        _ => RetrySettings::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_settings_default() {
        let settings = RetrySettings::new();

        assert_eq!(settings.max_retries, 3);
        assert_eq!(settings.initial_delay, Duration::from_millis(100));
        assert_eq!(settings.max_delay, Duration::from_secs(60));
        assert_eq!(settings.backoff_multiplier, 2.0);
    }

    #[test]
    fn test_retry_settings_builder() {
        let settings = RetrySettings::new()
            .with_max_retries(5)
            .with_initial_delay(Duration::from_millis(200))
            .with_max_delay(Duration::from_secs(30))
            .with_backoff_multiplier(1.5);

        assert_eq!(settings.max_retries, 5);
        assert_eq!(settings.initial_delay, Duration::from_millis(200));
        assert_eq!(settings.max_delay, Duration::from_secs(30));
        assert_eq!(settings.backoff_multiplier, 1.5);
    }

    #[test]
    fn test_calculate_delay() {
        let settings = RetrySettings::new()
            .with_initial_delay(Duration::from_millis(100))
            .with_backoff_multiplier(2.0);

        assert_eq!(settings.calculate_delay(0), Duration::from_millis(100));
        assert_eq!(settings.calculate_delay(1), Duration::from_millis(200));
        assert_eq!(settings.calculate_delay(2), Duration::from_millis(400));
    }

    #[test]
    fn test_calculate_delay_max() {
        let settings = RetrySettings::new()
            .with_initial_delay(Duration::from_millis(1000))
            .with_max_delay(Duration::from_millis(5000))
            .with_backoff_multiplier(10.0);

        // Should cap at max_delay
        assert_eq!(settings.calculate_delay(2), Duration::from_millis(5000));
    }

    #[test]
    fn test_is_retryable_status() {
        let settings = RetrySettings::new();

        assert!(settings.is_retryable_status(429));
        assert!(settings.is_retryable_status(500));
        assert!(settings.is_retryable_status(503));
        assert!(!settings.is_retryable_status(400));
        assert!(!settings.is_retryable_status(401));
    }

    #[test]
    fn test_is_exhausted() {
        let settings = RetrySettings::new().with_max_retries(3);

        assert!(!settings.is_exhausted(0));
        assert!(!settings.is_exhausted(2));
        assert!(settings.is_exhausted(3));
        assert!(settings.is_exhausted(4));
    }

    #[test]
    fn test_prepare_retries() {
        let settings = prepare_retries(Some(5), Some(500));

        assert_eq!(settings.max_retries, 5);
        assert_eq!(settings.initial_delay, Duration::from_millis(500));
    }

    #[test]
    fn test_prepare_provider_retries_anthropic() {
        let settings = prepare_provider_retries("anthropic");

        assert_eq!(settings.max_retries, 2);
        assert!(settings.is_retryable_status(529));
    }

    #[test]
    fn test_prepare_provider_retries_openai() {
        let settings = prepare_provider_retries("openai");

        assert_eq!(settings.max_retries, 3);
    }
}
