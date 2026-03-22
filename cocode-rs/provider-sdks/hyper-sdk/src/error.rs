//! Error types for hyper-sdk.
//!
//! # Error Chain Design
//!
//! hyper-sdk converts provider-specific errors into a unified `HyperError` type.
//! Errors like `NetworkError` and `ParseError` store stringified messages rather
//! than wrapping source errors directly. This is intentional for several reasons:
//!
//! 1. **Provider Independence**: Each provider SDK has different error types
//!    (reqwest::Error, serde_json::Error, etc.). Storing strings allows uniform
//!    handling without leaking provider-specific types.
//!
//! 2. **API Stability**: Wrapping source errors would expose internal dependencies,
//!    making semver-compatible changes harder.
//!
//! 3. **Serialization**: String errors serialize cleanly for logging and debugging.
//!
//! The `From` implementations preserve error context by including the source error's
//! Display output, which typically contains the full error chain information.

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
    #[error("unsupported capability: {0}")]
    UnsupportedCapability(String),

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
    ///
    /// The string contains the source error's display output, preserving error chain info.
    /// See module-level documentation for why we use strings instead of wrapping sources.
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
    ///
    /// The string contains the source error's display output, preserving error chain info.
    /// See module-level documentation for why we use strings instead of wrapping sources.
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

/// Maximum retry-after value (30 seconds). Values above this are capped.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);

/// Parse retry-after delay from an error message.
///
/// Looks for multiple common patterns:
/// - `"try again in Xs"` / `"try again in Xms"` / `"try again in X seconds"`
/// - `"Retry-After: X"` / `"Retry-After X"` (HTTP header format, seconds)
/// - `"retry_after: X"` / `"retry_after X"` (JSON field format, seconds)
///
/// Parsed values are capped at 30 seconds.
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
/// assert_eq!(parse_retry_after("Retry-After: 5"), Some(Duration::from_secs(5)));
/// assert_eq!(parse_retry_after("retry_after: 10"), Some(Duration::from_secs(10)));
/// assert_eq!(parse_retry_after("some error"), None);
/// ```
pub fn parse_retry_after(message: &str) -> Option<Duration> {
    // Try "try again in Xs" pattern first
    if let Some(duration) = parse_try_again_pattern(message) {
        return Some(duration.min(MAX_RETRY_AFTER));
    }

    // Try "Retry-After" header / JSON field patterns
    if let Some(duration) = parse_retry_after_field(message) {
        return Some(duration.min(MAX_RETRY_AFTER));
    }

    None
}

fn parse_try_again_pattern(message: &str) -> Option<Duration> {
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

fn parse_retry_after_field(message: &str) -> Option<Duration> {
    let re = retry_after_field_regex();
    let captures = re.captures(message)?;

    let value: f64 = captures.get(1)?.as_str().parse().ok()?;
    Some(Duration::from_secs_f64(value))
}

#[allow(clippy::expect_used)]
fn retry_after_regex() -> &'static regex_lite::Regex {
    static RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(r"(?i)try again in\s*(\d+(?:\.\d+)?)\s*(s|ms|seconds?)")
            .expect("invalid regex")
    })
}

#[allow(clippy::expect_used)]
fn retry_after_field_regex() -> &'static regex_lite::Regex {
    static RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        // Matches: "Retry-After: 5", "Retry-After 5", "retry_after: 10", "retry_after 10"
        regex_lite::Regex::new(r"(?i)retry[-_]after[:\s]\s*(\d+(?:\.\d+)?)").expect("invalid regex")
    })
}

/// Known secret prefixes to scrub from error messages.
const SECRET_PREFIXES: &[&str] = &[
    "sk-",
    "xoxb-",
    "xoxp-",
    "ghp_",
    "gho_",
    "ghu_",
    "github_pat_",
];

/// Bearer/token header patterns to scrub.
const SECRET_HEADER_PREFIXES: &[&str] = &["Bearer ", "token "];

/// Scrub secret patterns from a string.
///
/// Replaces any token that starts with known secret prefixes (`sk-`, `ghp_`, etc.)
/// or authentication headers (`Bearer`, `token`) with `[REDACTED]`.
///
/// # Examples
///
/// ```
/// use hyper_sdk::error::scrub_secret_patterns;
///
/// assert_eq!(
///     scrub_secret_patterns("API key sk-abc123xyz is invalid"),
///     "API key [REDACTED] is invalid"
/// );
/// assert_eq!(
///     scrub_secret_patterns("No secrets here"),
///     "No secrets here"
/// );
/// ```
pub fn scrub_secret_patterns(input: &str) -> String {
    let mut result = input.to_string();

    // Scrub Bearer/token headers: "Bearer <token>" → "Bearer [REDACTED]"
    for prefix in SECRET_HEADER_PREFIXES {
        while let Some(start) = result.find(prefix) {
            let token_start = start + prefix.len();
            let token_end = result[token_start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',')
                .map_or(result.len(), |pos| token_start + pos);
            if token_end > token_start {
                result.replace_range(start..token_end, "[REDACTED]");
            } else {
                break;
            }
        }
    }

    // Scrub secret key prefixes: "sk-abc123" → "[REDACTED]"
    for prefix in SECRET_PREFIXES {
        while let Some(start) = result.find(prefix) {
            let token_end = result[start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',')
                .map_or(result.len(), |pos| start + pos);
            if token_end > start {
                result.replace_range(start..token_end, "[REDACTED]");
            } else {
                break;
            }
        }
    }

    result
}

/// Sanitize an API error message by scrubbing secrets and truncating.
///
/// Combines [`scrub_secret_patterns`] with length truncation.
///
/// # Examples
///
/// ```
/// use hyper_sdk::error::sanitize_api_error;
///
/// let msg = sanitize_api_error("Key sk-secret123 failed", 20);
/// assert!(msg.len() <= 23); // 20 + "..." suffix
/// assert!(msg.contains("[REDACTED]"));
/// ```
pub fn sanitize_api_error(input: &str, max_chars: usize) -> String {
    let scrubbed = scrub_secret_patterns(input);
    if scrubbed.chars().count() <= max_chars {
        scrubbed
    } else {
        let byte_end = scrubbed
            .char_indices()
            .nth(max_chars)
            .map_or(scrubbed.len(), |(idx, _)| idx);
        let mut truncated = scrubbed[..byte_end].to_string();
        truncated.push_str("...");
        truncated
    }
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
