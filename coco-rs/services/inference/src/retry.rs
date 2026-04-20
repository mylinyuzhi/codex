use std::time::Duration;

use crate::errors::InferenceError;

/// Retry configuration for API calls.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries.
    pub max_retries: i32,
    /// Base delay for exponential backoff (milliseconds).
    pub base_delay_ms: i64,
    /// Maximum delay cap (milliseconds).
    pub max_delay_ms: i64,
    /// Jitter factor (0.0 to 1.0).
    pub jitter_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1000,
            max_delay_ms: 60_000,
            jitter_factor: 0.25,
        }
    }
}

/// Lift `coco_config::ApiRetryConfig` (settings-sourced, already
/// finalized / clamped by `ApiConfig::resolve`) into the inference
/// crate's `RetryConfig`. Fields are 1:1 — the two structs mirror each
/// other so callers can pipe `runtime_config.api.retry.clone().into()`
/// directly into `ApiClient::new`.
impl From<coco_config::ApiRetryConfig> for RetryConfig {
    fn from(cfg: coco_config::ApiRetryConfig) -> Self {
        Self {
            max_retries: cfg.max_retries,
            base_delay_ms: cfg.base_delay_ms,
            max_delay_ms: cfg.max_delay_ms,
            jitter_factor: cfg.jitter_factor,
        }
    }
}

impl RetryConfig {
    /// Calculate delay for a given attempt number.
    /// Uses server-specified retry-after if available, else exponential backoff.
    pub fn delay_for_attempt(&self, attempt: i32, error: &InferenceError) -> Duration {
        // Use server-specified retry-after if available
        if let Some(retry_after) = error.retry_after_ms() {
            return Duration::from_millis(retry_after as u64);
        }

        // Exponential backoff: base * 2^attempt, capped at max_delay
        let delay = self
            .base_delay_ms
            .saturating_mul(2_i64.saturating_pow(attempt as u32));
        let delay = delay.min(self.max_delay_ms);

        // Add jitter (deterministic for reproducibility)
        let jitter = (delay as f64 * self.jitter_factor) as i64;
        let jittered = delay.saturating_add(jitter);

        Duration::from_millis(jittered as u64)
    }

    /// Whether a retry should be attempted for this error at this attempt count.
    pub fn should_retry(&self, attempt: i32, error: &InferenceError) -> bool {
        attempt < self.max_retries && error.is_retryable()
    }

    /// Check if this error is an auth failure that needs credential refresh.
    ///
    /// TS: withRetry.ts — 401 errors trigger credential cache clearing.
    pub fn is_auth_error(error: &InferenceError) -> bool {
        matches!(
            error,
            InferenceError::AuthenticationFailed { .. }
                | InferenceError::ProviderError { status: 401, .. }
                | InferenceError::ProviderError { status: 403, .. }
        )
    }

    /// Check if this error is a rate limit that needs cooldown.
    ///
    /// TS: withRetry.ts — 429 errors trigger rate limit cooldown.
    pub fn is_rate_limit(error: &InferenceError) -> bool {
        matches!(error, InferenceError::RateLimited { .. })
    }

    /// Check if this error indicates the prompt is too long.
    pub fn is_prompt_too_long(error: &InferenceError) -> bool {
        match error {
            InferenceError::ProviderError { message, .. } => {
                message.contains("prompt is too long")
                    || message.contains("prompt_too_long")
                    || message.contains("context_length_exceeded")
            }
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "retry.test.rs"]
mod tests;
