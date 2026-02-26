//! Error types for the API layer.

use cocode_error::ErrorExt;
use cocode_error::Location;
use cocode_error::StatusCode;
use cocode_error::stack_trace_debug;
use snafu::Snafu;
use std::time::Duration;

/// API layer errors.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
pub enum ApiError {
    /// Network error during API call.
    #[snafu(display("Network error: {message}"))]
    Network {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Authentication error.
    #[snafu(display("Authentication failed: {message}"))]
    Authentication {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Rate limit exceeded.
    #[snafu(display("Rate limited: {message}, retry after {retry_after_ms}ms"))]
    RateLimited {
        message: String,
        retry_after_ms: i64,
        #[snafu(implicit)]
        location: Location,
    },

    /// Model overloaded or server error with retry hint.
    #[snafu(display("Model overloaded: {message} (retry after {retry_after_ms}ms)"))]
    Overloaded {
        message: String,
        retry_after_ms: i64,
        #[snafu(implicit)]
        location: Location,
    },

    /// Stream error during streaming response.
    #[snafu(display("Stream error: {message}"))]
    Stream {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Stream idle timeout.
    #[snafu(display("Stream idle timeout after {timeout_secs}s"))]
    StreamIdleTimeout {
        timeout_secs: i64,
        #[snafu(implicit)]
        location: Location,
    },

    /// Invalid request.
    #[snafu(display("Invalid request: {message}"))]
    InvalidRequest {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Provider error.
    #[snafu(display("Provider error: {message}"))]
    Provider {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// All retries exhausted.
    #[snafu(display("Retries exhausted after {attempts} attempts: {message}"))]
    RetriesExhausted {
        attempts: i32,
        message: String,
        /// Full trail of failure details from each retry attempt.
        diagnostics: Vec<String>,
        #[snafu(implicit)]
        location: Location,
    },

    /// Underlying hyper-sdk error.
    #[snafu(display("SDK error: {message}"))]
    Sdk {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Context window exceeded.
    #[snafu(display("Context overflow: {message}"))]
    ContextOverflow {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl ApiError {
    /// Check if this is a context overflow error.
    pub fn is_context_overflow(&self) -> bool {
        matches!(self, ApiError::ContextOverflow { .. })
    }

    /// Check if this error indicates no model is configured.
    pub fn is_no_model_configured(&self) -> bool {
        matches!(self, ApiError::InvalidRequest { message, .. } if message.starts_with("No model configured"))
    }

    /// Check if this is a stream-related error that should trigger fallback.
    ///
    /// Returns true for errors where falling back to non-streaming mode might help.
    pub fn is_stream_error(&self) -> bool {
        matches!(
            self,
            ApiError::Stream { .. } | ApiError::StreamIdleTimeout { .. }
        )
    }

    /// Get the diagnostics trail from a `RetriesExhausted` error.
    pub fn diagnostics(&self) -> &[String] {
        match self {
            ApiError::RetriesExhausted { diagnostics, .. } => diagnostics,
            _ => &[],
        }
    }
}

impl ErrorExt for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Network { .. } => StatusCode::NetworkError,
            ApiError::Authentication { .. } => StatusCode::AuthenticationFailed,
            ApiError::RateLimited { .. } => StatusCode::RateLimited,
            ApiError::Overloaded { .. } => StatusCode::ServiceUnavailable,
            ApiError::Stream { .. } => StatusCode::StreamError,
            ApiError::StreamIdleTimeout { .. } => StatusCode::Timeout,
            ApiError::InvalidRequest { .. } => StatusCode::InvalidArguments,
            ApiError::Provider { .. } => StatusCode::ProviderError,
            ApiError::RetriesExhausted { .. } => StatusCode::NetworkError,
            ApiError::Sdk { .. } => StatusCode::Internal,
            ApiError::ContextOverflow { .. } => StatusCode::ContextWindowExceeded,
        }
    }

    fn is_retryable(&self) -> bool {
        matches!(
            self,
            ApiError::Network { .. }
                | ApiError::Overloaded { .. }
                | ApiError::Stream { .. }
                | ApiError::StreamIdleTimeout { .. }
                | ApiError::RateLimited { .. }
        )
    }

    fn retry_after(&self) -> Option<Duration> {
        match self {
            ApiError::RateLimited { retry_after_ms, .. }
            | ApiError::Overloaded { retry_after_ms, .. } => {
                Some(Duration::from_millis(*retry_after_ms as u64))
            }
            _ => None,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Classify a provider error by scanning the code and message for known patterns.
///
/// This heuristic reclassifies generic `ProviderError` into more specific error types
/// (auth failure, model-not-found, context overflow, rate limit) based on keywords.
fn classify_provider_error(code: &str, message: &str) -> ApiError {
    use api_error::*;
    let lower = message.to_ascii_lowercase();
    let lower_code = code.to_ascii_lowercase();

    // Auth failure keywords
    const AUTH_KEYWORDS: &[&str] = &[
        "invalid api key",
        "invalid_api_key",
        "incorrect api key",
        "authentication",
        "unauthorized",
        "api key not found",
        "invalid x-api-key",
        "invalid authorization",
        "permission denied",
        "access denied",
    ];
    if lower_code == "401" || AUTH_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return AuthenticationSnafu {
            message: message.to_string(),
        }
        .build();
    }

    // Model not found keywords
    const MODEL_KEYWORDS: &[&str] = &[
        "model not found",
        "model_not_found",
        "no such model",
        "does not exist",
        "unknown model",
        "invalid model",
        "model is not accessible",
    ];
    if MODEL_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return InvalidRequestSnafu {
            message: message.to_string(),
        }
        .build();
    }

    // Context window overflow keywords
    const CONTEXT_KEYWORDS: &[&str] = &[
        "context length",
        "context window",
        "token limit",
        "input too long",
        "too many tokens",
        "context_length_exceeded",
        "maximum context",
        "max_tokens",
        "tokens exceeded",
    ];
    if CONTEXT_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return ContextOverflowSnafu {
            message: message.to_string(),
        }
        .build();
    }

    // Rate limit keywords
    const RATE_LIMIT_KEYWORDS: &[&str] = &[
        "rate limit",
        "rate_limit",
        "too many requests",
        "throttled",
        "try again",
    ];
    if lower_code == "429" || RATE_LIMIT_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        let retry_ms = hyper_sdk::error::parse_retry_after(message)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(1000);
        return RateLimitedSnafu {
            message: message.to_string(),
            retry_after_ms: retry_ms,
        }
        .build();
    }

    // Default: keep as Provider error
    ProviderSnafu {
        message: message.to_string(),
    }
    .build()
}

impl From<hyper_sdk::HyperError> for ApiError {
    fn from(err: hyper_sdk::HyperError) -> Self {
        use api_error::*;
        use hyper_sdk::HyperError;
        use hyper_sdk::scrub_secret_patterns as scrub;

        match err {
            HyperError::NetworkError(msg) => NetworkSnafu {
                message: scrub(&msg),
            }
            .build(),
            HyperError::AuthenticationFailed(msg) => AuthenticationSnafu {
                message: scrub(&msg),
            }
            .build(),
            HyperError::RateLimitExceeded(msg) => {
                let retry_ms = hyper_sdk::error::parse_retry_after(&msg)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(1000);
                RateLimitedSnafu {
                    message: scrub(&msg),
                    retry_after_ms: retry_ms,
                }
                .build()
            }
            HyperError::Retryable { message, delay } => {
                let ms = delay.map(|d| d.as_millis() as i64).unwrap_or(1000);
                OverloadedSnafu {
                    message: scrub(&message),
                    retry_after_ms: ms,
                }
                .build()
            }
            HyperError::ContextWindowExceeded(msg) => ContextOverflowSnafu {
                message: scrub(&msg),
            }
            .build(),
            HyperError::StreamError(msg) => StreamSnafu {
                message: scrub(&msg),
            }
            .build(),
            HyperError::StreamIdleTimeout(timeout) => StreamIdleTimeoutSnafu {
                timeout_secs: timeout.as_secs() as i64,
            }
            .build(),
            HyperError::InvalidRequest(msg) => InvalidRequestSnafu {
                message: scrub(&msg),
            }
            .build(),
            HyperError::ProviderError { code, message } => {
                classify_provider_error(&code, &scrub(&message))
            }
            HyperError::ProviderNotFound(msg) => InvalidRequestSnafu {
                message: format!("Provider not found: {}", scrub(&msg)),
            }
            .build(),
            HyperError::ModelNotFound(msg) => InvalidRequestSnafu {
                message: format!("Model not found: {}", scrub(&msg)),
            }
            .build(),
            HyperError::ConfigError(msg) => SdkSnafu {
                message: format!("Config error: {}", scrub(&msg)),
            }
            .build(),
            other => SdkSnafu {
                message: scrub(&other.to_string()),
            }
            .build(),
        }
    }
}

/// Result type for API operations.
pub type Result<T> = std::result::Result<T, ApiError>;

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
