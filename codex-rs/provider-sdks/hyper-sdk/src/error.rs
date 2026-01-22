//! Error types for hyper-sdk.

use crate::capability::Capability;
use std::time::Duration;
use thiserror::Error;

/// Result type alias using HyperError.
pub type Result<T> = std::result::Result<T, HyperError>;

/// Errors that can occur when using hyper-sdk.
#[derive(Debug, Error)]
pub enum HyperError {
    /// Provider not found in registry.
    #[error("provider not found: {0}")]
    ProviderNotFound(String),

    /// Model not found or not supported by provider.
    #[error("model not found: {0}")]
    ModelNotFound(String),

    /// Requested capability is not supported by the model.
    #[error("unsupported capability: {0:?}")]
    UnsupportedCapability(Capability),

    /// Authentication failed (invalid or missing API key).
    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Rate limit exceeded.
    #[error("rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    /// Context window exceeded.
    #[error("context window exceeded: {0}")]
    ContextWindowExceeded(String),

    /// Invalid request parameters.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// Network or HTTP error.
    #[error("network error: {0}")]
    NetworkError(String),

    /// Provider returned an error response.
    #[error("provider error: {code}: {message}")]
    ProviderError {
        /// Error code from the provider.
        code: String,
        /// Error message from the provider.
        message: String,
    },

    /// Failed to parse response from provider.
    #[error("parse error: {0}")]
    ParseError(String),

    /// Streaming error.
    #[error("stream error: {0}")]
    StreamError(String),

    /// Configuration error.
    #[error("configuration error: {0}")]
    ConfigError(String),

    /// Internal SDK error.
    #[error("internal error: {0}")]
    Internal(String),

    /// Retryable error with optional suggested delay.
    #[error("retryable error: {message}")]
    Retryable {
        /// Error message.
        message: String,
        /// Suggested delay before retry (parsed from error message).
        delay: Option<Duration>,
    },

    /// Previous response not found (session continuity).
    #[error("previous response not found: {0}")]
    PreviousResponseNotFound(String),

    /// Quota exceeded (different from rate limit, requires billing change).
    #[error("quota exceeded: {0}")]
    QuotaExceeded(String),

    /// Stream idle timeout (no events received within timeout period).
    #[error("stream idle timeout after {0:?}")]
    StreamIdleTimeout(Duration),
}

impl HyperError {
    /// Check if this error is retryable.
    ///
    /// Returns `true` for transient errors that may succeed on retry:
    /// - `Retryable` variant (explicitly marked as retryable)
    /// - `RateLimitExceeded` (temporary rate limiting)
    /// - `NetworkError` (connection issues, timeouts)
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            HyperError::Retryable { .. }
                | HyperError::RateLimitExceeded(_)
                | HyperError::NetworkError(_)
        )
    }

    /// Get the suggested retry delay, if available.
    ///
    /// Only returns a value for `Retryable` errors that include a parsed delay.
    pub fn retry_delay(&self) -> Option<Duration> {
        match self {
            HyperError::Retryable { delay, .. } => *delay,
            _ => None,
        }
    }
}

impl From<reqwest::Error> for HyperError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            HyperError::NetworkError(format!("request timeout: {err}"))
        } else if err.is_connect() {
            HyperError::NetworkError(format!("connection failed: {err}"))
        } else {
            HyperError::NetworkError(err.to_string())
        }
    }
}

impl From<serde_json::Error> for HyperError {
    fn from(err: serde_json::Error) -> Self {
        HyperError::ParseError(err.to_string())
    }
}

/// Parse retry-after delay from an error message.
///
/// Looks for patterns like "try again in Xs" or "try again in Xms" in the message.
/// This is commonly used by rate-limited APIs to suggest when to retry.
///
/// # Examples
///
/// ```
/// use hyper_sdk::error::parse_retry_after;
/// use std::time::Duration;
///
/// assert_eq!(parse_retry_after("try again in 5s"), Some(Duration::from_secs(5)));
/// assert_eq!(parse_retry_after("try again in 500ms"), Some(Duration::from_millis(500)));
/// assert_eq!(parse_retry_after("try again in 2.5 seconds"), Some(Duration::from_secs_f64(2.5)));
/// assert_eq!(parse_retry_after("some error"), None);
/// ```
pub fn parse_retry_after(message: &str) -> Option<Duration> {
    let re = retry_after_regex();
    let captures = re.captures(message)?;

    let value = captures.get(1)?;
    let unit = captures.get(2)?;

    let value: f64 = value.as_str().parse().ok()?;
    let unit = unit.as_str().to_ascii_lowercase();

    if unit == "s" || unit.starts_with("second") {
        Some(Duration::from_secs_f64(value))
    } else if unit == "ms" {
        Some(Duration::from_millis(value as u64))
    } else {
        None
    }
}

fn retry_after_regex() -> &'static regex_lite::Regex {
    static RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(r"(?i)try again in\s*(\d+(?:\.\d+)?)\s*(s|ms|seconds?)")
            .expect("invalid regex")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = HyperError::ProviderNotFound("openai".to_string());
        assert_eq!(err.to_string(), "provider not found: openai");

        let err = HyperError::UnsupportedCapability(Capability::Vision);
        assert_eq!(err.to_string(), "unsupported capability: Vision");

        let err = HyperError::ProviderError {
            code: "invalid_api_key".to_string(),
            message: "The API key is invalid".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "provider error: invalid_api_key: The API key is invalid"
        );
    }

    #[test]
    fn test_is_retryable() {
        assert!(
            HyperError::Retryable {
                message: "rate limited".to_string(),
                delay: None,
            }
            .is_retryable()
        );
        assert!(HyperError::RateLimitExceeded("limit reached".to_string()).is_retryable());
        assert!(HyperError::NetworkError("connection refused".to_string()).is_retryable());

        assert!(!HyperError::AuthenticationFailed("invalid key".to_string()).is_retryable());
        assert!(!HyperError::QuotaExceeded("quota exceeded".to_string()).is_retryable());
    }

    #[test]
    fn test_retry_delay() {
        let err = HyperError::Retryable {
            message: "try again".to_string(),
            delay: Some(Duration::from_secs(5)),
        };
        assert_eq!(err.retry_delay(), Some(Duration::from_secs(5)));

        let err_no_delay = HyperError::Retryable {
            message: "try again".to_string(),
            delay: None,
        };
        assert_eq!(err_no_delay.retry_delay(), None);

        let other_err = HyperError::NetworkError("timeout".to_string());
        assert_eq!(other_err.retry_delay(), None);
    }

    #[test]
    fn test_parse_retry_after() {
        // Seconds
        assert_eq!(
            parse_retry_after("try again in 5s"),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            parse_retry_after("Try Again In 10s"),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            parse_retry_after("try again in 2.5 seconds"),
            Some(Duration::from_secs_f64(2.5))
        );
        assert_eq!(
            parse_retry_after("try again in 1 second"),
            Some(Duration::from_secs(1))
        );

        // Milliseconds
        assert_eq!(
            parse_retry_after("try again in 500ms"),
            Some(Duration::from_millis(500))
        );

        // No match
        assert_eq!(parse_retry_after("some error message"), None);
        assert_eq!(parse_retry_after("rate limit exceeded"), None);
        assert_eq!(parse_retry_after(""), None);
    }

    #[test]
    fn test_new_error_display() {
        let err = HyperError::StreamIdleTimeout(Duration::from_secs(60));
        assert!(err.to_string().contains("60"));

        let err = HyperError::QuotaExceeded("monthly limit".to_string());
        assert_eq!(err.to_string(), "quota exceeded: monthly limit");

        let err = HyperError::PreviousResponseNotFound("resp_123".to_string());
        assert_eq!(err.to_string(), "previous response not found: resp_123");

        let err = HyperError::Retryable {
            message: "rate limited".to_string(),
            delay: Some(Duration::from_secs(5)),
        };
        assert_eq!(err.to_string(), "retryable error: rate limited");
    }
}
