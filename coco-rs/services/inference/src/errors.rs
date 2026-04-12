use std::fmt;

/// Inference error categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceError {
    /// Authentication failure (invalid key, expired token).
    AuthenticationFailed { message: String },
    /// Rate limited by provider (429).
    RateLimited {
        retry_after_ms: Option<i64>,
        message: String,
    },
    /// Context window exceeded.
    ContextWindowExceeded { max_tokens: i64, requested: i64 },
    /// Provider returned an error response.
    ProviderError { status: i32, message: String },
    /// Network error (connection, timeout).
    NetworkError { message: String },
    /// Stream interrupted.
    StreamInterrupted { message: String },
    /// Request cancelled.
    Cancelled,
    /// Overloaded (503).
    Overloaded { retry_after_ms: Option<i64> },
    /// Invalid request (400).
    InvalidRequest { message: String },
}

impl InferenceError {
    /// Whether this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. }
                | Self::NetworkError { .. }
                | Self::StreamInterrupted { .. }
                | Self::Overloaded { .. }
        )
    }

    /// Suggested retry delay in milliseconds.
    pub fn retry_after_ms(&self) -> Option<i64> {
        match self {
            Self::RateLimited { retry_after_ms, .. } | Self::Overloaded { retry_after_ms } => {
                *retry_after_ms
            }
            _ => None,
        }
    }

    /// Classify from an HTTP status code and body.
    pub fn from_http_status(status: i32, body: &str, retry_after: Option<i64>) -> Self {
        match status {
            400 => {
                // Check for context window overflow in body
                if body.contains("context_length_exceeded")
                    || body.contains("max_tokens")
                    || body.contains("too many tokens")
                {
                    Self::ContextWindowExceeded {
                        max_tokens: 0,
                        requested: 0,
                    }
                } else {
                    Self::InvalidRequest {
                        message: truncate_body(body),
                    }
                }
            }
            401 | 403 => Self::AuthenticationFailed {
                message: truncate_body(body),
            },
            429 => Self::RateLimited {
                retry_after_ms: retry_after,
                message: truncate_body(body),
            },
            500 | 502 => Self::ProviderError {
                status,
                message: truncate_body(body),
            },
            503 => Self::Overloaded {
                retry_after_ms: retry_after,
            },
            529 => Self::Overloaded {
                retry_after_ms: retry_after,
            },
            _ => Self::ProviderError {
                status,
                message: truncate_body(body),
            },
        }
    }

    /// Error class for telemetry.
    pub fn error_class(&self) -> &'static str {
        match self {
            Self::AuthenticationFailed { .. } => "auth",
            Self::RateLimited { .. } => "rate_limit",
            Self::ContextWindowExceeded { .. } => "context_overflow",
            Self::ProviderError { .. } => "provider_error",
            Self::NetworkError { .. } => "network",
            Self::StreamInterrupted { .. } => "stream_interrupted",
            Self::Cancelled => "cancelled",
            Self::Overloaded { .. } => "overloaded",
            Self::InvalidRequest { .. } => "invalid_request",
        }
    }
}

fn truncate_body(body: &str) -> String {
    if body.len() > 500 {
        format!("{}...", &body[..500])
    } else {
        body.to_string()
    }
}

impl fmt::Display for InferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationFailed { message } => {
                write!(f, "authentication failed: {message}")
            }
            Self::RateLimited { message, .. } => write!(f, "rate limited: {message}"),
            Self::ContextWindowExceeded {
                max_tokens,
                requested,
            } => {
                write!(f, "context window exceeded: {requested} > {max_tokens}")
            }
            Self::ProviderError { status, message } => {
                write!(f, "provider error ({status}): {message}")
            }
            Self::NetworkError { message } => write!(f, "network error: {message}"),
            Self::StreamInterrupted { message } => write!(f, "stream interrupted: {message}"),
            Self::Cancelled => write!(f, "request cancelled"),
            Self::Overloaded { .. } => write!(f, "provider overloaded"),
            Self::InvalidRequest { message } => write!(f, "invalid request: {message}"),
        }
    }
}

impl std::error::Error for InferenceError {}

#[cfg(test)]
#[path = "errors.test.rs"]
mod tests;
