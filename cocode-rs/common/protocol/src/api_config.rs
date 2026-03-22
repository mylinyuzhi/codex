//! API client configuration constants.
//!
//! Defines configurable defaults for the API client layer: retry behavior,
//! overflow recovery thresholds, and stall detection parameters. The actual
//! `ApiClient` in `core/api` reads these values through its `ApiClientConfig`.

use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

// =============================================================================
// Retry Defaults
// =============================================================================

/// Default maximum retry attempts (matches Claude Code's `maxAttempts`).
pub const DEFAULT_MAX_RETRIES: i32 = 5;

/// Default base delay for exponential/linear backoff (ms).
pub const DEFAULT_BASE_DELAY_MS: i64 = 1000;

/// Default maximum delay cap (ms). Claude Code: `min(60000, 1000*2^attempt)`.
pub const DEFAULT_MAX_DELAY_MS: i64 = 60000;

/// Default backoff multiplier.
pub const DEFAULT_MULTIPLIER: f64 = 2.0;

/// Default jitter fraction (±20% random variation).
pub const DEFAULT_JITTER: f64 = 0.2;

// =============================================================================
// Overflow Recovery Defaults
// =============================================================================

/// Default maximum tokens for fallback (non-streaming) requests.
pub const DEFAULT_FALLBACK_MAX_TOKENS: i64 = 21333;

/// Default minimum output tokens before giving up on overflow recovery.
pub const DEFAULT_MIN_OUTPUT_TOKENS: i64 = 3000;

/// Default maximum overflow recovery attempts.
pub const DEFAULT_MAX_OVERFLOW_ATTEMPTS: i32 = 3;

/// Absolute minimum output tokens during overflow recovery.
///
/// Claude Code: `fN8 = 3000`. If the calculated available space is below
/// this floor, overflow recovery is not possible. Matches the single floor
/// constant used by Claude Code for both the recovery calculation and the
/// viability gate.
pub const DEFAULT_FLOOR_OUTPUT_TOKENS: i64 = 3000;

/// Safety buffer subtracted from available context during overflow recovery.
///
/// Claude Code: `BUFFER = 1000`. Accounts for token estimation inaccuracy.
pub const DEFAULT_BUFFER_TOKENS: i64 = 1000;

/// Maximum consecutive overload errors (529/503) before triggering
/// fast-mode degradation or model fallback.
///
/// Claude Code: `MAX_529_ERRORS_BEFORE_FALLBACK = 3`.
pub const DEFAULT_MAX_CONSECUTIVE_OVERLOAD_ERRORS: i32 = 3;

// =============================================================================
// Stall Detection Defaults
// =============================================================================

/// Default stall detection timeout (seconds).
pub const DEFAULT_STALL_TIMEOUT_SECS: i64 = 30;

// =============================================================================
// ApiRetryConfig (protocol-level)
// =============================================================================

/// Protocol-level API retry configuration.
///
/// Re-exported as `RetryConfig` in `core/api` for backward compatibility.
/// All fields have `#[serde(default)]` with constants above.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiRetryConfig {
    /// Maximum retry attempts.
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,

    /// Base delay for backoff (ms).
    #[serde(default = "default_base_delay_ms")]
    pub base_delay_ms: i64,

    /// Maximum delay cap (ms).
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: i64,

    /// Backoff multiplier.
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,

    /// Jitter fraction (0.0–1.0).
    #[serde(default = "default_jitter")]
    pub jitter: f64,
}

impl Default for ApiRetryConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            base_delay_ms: DEFAULT_BASE_DELAY_MS,
            max_delay_ms: DEFAULT_MAX_DELAY_MS,
            multiplier: DEFAULT_MULTIPLIER,
            jitter: DEFAULT_JITTER,
        }
    }
}

impl ApiRetryConfig {
    /// Create a config with no retries.
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Set jitter fraction (0.0–1.0).
    pub fn with_jitter(mut self, jitter: f64) -> Self {
        self.jitter = jitter.clamp(0.0, 1.0);
        self
    }

    /// Set maximum retry attempts.
    pub fn with_max_retries(mut self, max: i32) -> Self {
        self.max_retries = max;
        self
    }

    /// Set base delay.
    pub fn with_base_delay(mut self, delay: Duration) -> Self {
        self.base_delay_ms = delay.as_millis() as i64;
        self
    }

    /// Set maximum delay.
    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay_ms = delay.as_millis() as i64;
        self
    }

    /// Set backoff multiplier.
    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }
}

// =============================================================================
// ApiFallbackConfig (protocol-level)
// =============================================================================

/// Protocol-level API fallback configuration.
///
/// Controls overflow recovery and stream→non-stream fallback behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiFallbackConfig {
    /// Enable automatic fallback from streaming to non-streaming on stream errors.
    #[serde(default = "crate::default_true")]
    pub enable_stream_fallback: bool,

    /// Maximum tokens for fallback requests.
    #[serde(default = "default_fallback_max_tokens")]
    pub fallback_max_tokens: Option<i64>,

    /// Enable context overflow recovery (auto-reduce max_tokens).
    #[serde(default = "crate::default_true")]
    pub enable_overflow_recovery: bool,

    /// Minimum output tokens before giving up on overflow recovery.
    #[serde(default = "default_min_output_tokens")]
    pub min_output_tokens: i64,

    /// Maximum overflow recovery attempts.
    #[serde(default = "default_max_overflow_attempts")]
    pub max_overflow_attempts: i32,

    /// Absolute minimum output tokens for overflow recovery.
    #[serde(default = "default_floor_output_tokens")]
    pub floor_output_tokens: i64,

    /// Safety buffer subtracted from available context during overflow recovery.
    #[serde(default = "default_buffer_tokens")]
    pub buffer_tokens: i64,
}

impl Default for ApiFallbackConfig {
    fn default() -> Self {
        Self {
            enable_stream_fallback: true,
            fallback_max_tokens: Some(DEFAULT_FALLBACK_MAX_TOKENS),
            enable_overflow_recovery: true,
            min_output_tokens: DEFAULT_MIN_OUTPUT_TOKENS,
            max_overflow_attempts: DEFAULT_MAX_OVERFLOW_ATTEMPTS,
            floor_output_tokens: DEFAULT_FLOOR_OUTPUT_TOKENS,
            buffer_tokens: DEFAULT_BUFFER_TOKENS,
        }
    }
}

impl ApiFallbackConfig {
    /// Disable all fallback mechanisms.
    pub fn disabled() -> Self {
        Self {
            enable_stream_fallback: false,
            fallback_max_tokens: None,
            enable_overflow_recovery: false,
            min_output_tokens: DEFAULT_MIN_OUTPUT_TOKENS,
            max_overflow_attempts: 0,
            floor_output_tokens: DEFAULT_FLOOR_OUTPUT_TOKENS,
            buffer_tokens: DEFAULT_BUFFER_TOKENS,
        }
    }

    pub fn with_stream_fallback(mut self, enabled: bool) -> Self {
        self.enable_stream_fallback = enabled;
        self
    }

    pub fn with_fallback_max_tokens(mut self, max_tokens: Option<i64>) -> Self {
        self.fallback_max_tokens = max_tokens;
        self
    }

    pub fn with_overflow_recovery(mut self, enabled: bool) -> Self {
        self.enable_overflow_recovery = enabled;
        self
    }

    pub fn with_min_output_tokens(mut self, min_tokens: i64) -> Self {
        self.min_output_tokens = min_tokens;
        self
    }

    pub fn with_max_overflow_attempts(mut self, max_attempts: i32) -> Self {
        self.max_overflow_attempts = max_attempts;
        self
    }
}

// =============================================================================
// Default value functions for serde
// =============================================================================

fn default_max_retries() -> i32 {
    DEFAULT_MAX_RETRIES
}

fn default_base_delay_ms() -> i64 {
    DEFAULT_BASE_DELAY_MS
}

fn default_max_delay_ms() -> i64 {
    DEFAULT_MAX_DELAY_MS
}

fn default_multiplier() -> f64 {
    DEFAULT_MULTIPLIER
}

fn default_jitter() -> f64 {
    DEFAULT_JITTER
}

fn default_fallback_max_tokens() -> Option<i64> {
    Some(DEFAULT_FALLBACK_MAX_TOKENS)
}

fn default_min_output_tokens() -> i64 {
    DEFAULT_MIN_OUTPUT_TOKENS
}

fn default_max_overflow_attempts() -> i32 {
    DEFAULT_MAX_OVERFLOW_ATTEMPTS
}

fn default_floor_output_tokens() -> i64 {
    DEFAULT_FLOOR_OUTPUT_TOKENS
}

fn default_buffer_tokens() -> i64 {
    DEFAULT_BUFFER_TOKENS
}

#[cfg(test)]
#[path = "api_config.test.rs"]
mod tests;
