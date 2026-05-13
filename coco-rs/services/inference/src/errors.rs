use coco_error::ErrorExt;
use coco_error::Location;
use coco_error::StatusCode;
use coco_error::stack_trace_debug;
use snafu::Snafu;
use std::time::Duration;

// Re-export the snafu-generated context selectors so callers can use
// `crate::errors::ProviderErrorSnafu { ... }.build()` ergonomically.
// `#[snafu(module)]` placed selectors into the `inference_error` sub-module.
pub use inference_error::*;

/// Inference error categories.
///
/// Variant fields are positional-compatible with prior hand-rolled enum
/// (every variant adds a `location` captured by `#[snafu(implicit)]` for
/// virtual-stack debug rendering). Pattern matches in callers should use
/// `..` rest patterns.
#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub), module)]
pub enum InferenceError {
    /// Authentication failure (invalid key, expired token).
    #[snafu(display("authentication failed: {message}"))]
    AuthenticationFailed {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Rate limited by provider (429).
    #[snafu(display("rate limited: {message}"))]
    RateLimited {
        retry_after_ms: Option<i64>,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Context window exceeded.
    #[snafu(display("context window exceeded: {requested} > {max_tokens}"))]
    ContextWindowExceeded {
        max_tokens: i64,
        requested: i64,
        #[snafu(implicit)]
        location: Location,
    },

    /// Provider returned an error response.
    #[snafu(display("provider error ({status}): {message}"))]
    ProviderError {
        status: i32,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Network error (connection, timeout).
    #[snafu(display("network error: {message}"))]
    NetworkError {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Stream interrupted.
    #[snafu(display("stream interrupted: {message}"))]
    StreamInterrupted {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Request cancelled.
    #[snafu(display("request cancelled"))]
    Cancelled {
        #[snafu(implicit)]
        location: Location,
    },

    /// Overloaded (503 / 529).
    #[snafu(display("provider overloaded"))]
    Overloaded {
        retry_after_ms: Option<i64>,
        #[snafu(implicit)]
        location: Location,
    },

    /// Invalid request (400).
    #[snafu(display("invalid request: {message}"))]
    InvalidRequest {
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Model spec references a provider not registered in `RuntimeConfig`.
    #[snafu(display(
        "model spec references unknown provider `{provider}`; \
         add it to ~/.coco/providers.json or settings.providers"
    ))]
    UnknownProvider {
        provider: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// Wrapped failure from a provider-specific builder — anthropic /
    /// google / openai-compatible `language_model`, or
    /// `parse_provider_options`.
    #[snafu(display("{provider} provider `{provider_name}`: {message}"))]
    ProviderBuildFailed {
        provider: &'static str,
        provider_name: String,
        message: String,
        #[snafu(implicit)]
        location: Location,
    },

    /// `RoleClientCache::resolve` could not find a `ModelSpec` for the
    /// requested role. `RuntimeConfig::resolve_model_roles` is supposed
    /// to insert Main's spec as the fallback for every unconfigured
    /// role, so this surfaces only when the runtime config was built
    /// by a path that bypassed the normal layering.
    #[snafu(display(
        "model role `{role}` not configured in RuntimeConfig.model_roles \
         (no Main fallback either)"
    ))]
    ModelRoleUnresolved {
        role: String,
        #[snafu(implicit)]
        location: Location,
    },
}

impl InferenceError {
    /// Suggested retry delay in milliseconds.
    pub fn retry_after_ms(&self) -> Option<i64> {
        match self {
            Self::RateLimited { retry_after_ms, .. } | Self::Overloaded { retry_after_ms, .. } => {
                *retry_after_ms
            }
            _ => None,
        }
    }

    /// Classify from an HTTP status code and body.
    pub fn from_http_status(status: i32, body: &str, retry_after: Option<i64>) -> Self {
        match status {
            400 => {
                if body.contains("context_length_exceeded")
                    || body.contains("max_tokens")
                    || body.contains("too many tokens")
                {
                    inference_error::ContextWindowExceededSnafu {
                        max_tokens: 0_i64,
                        requested: 0_i64,
                    }
                    .build()
                } else {
                    inference_error::InvalidRequestSnafu {
                        message: truncate_body(body),
                    }
                    .build()
                }
            }
            401 | 403 => inference_error::AuthenticationFailedSnafu {
                message: truncate_body(body),
            }
            .build(),
            429 => inference_error::RateLimitedSnafu {
                retry_after_ms: retry_after,
                message: truncate_body(body),
            }
            .build(),
            500 | 502 => inference_error::ProviderSnafu {
                status,
                message: truncate_body(body),
            }
            .build(),
            503 | 529 => inference_error::OverloadedSnafu {
                retry_after_ms: retry_after,
            }
            .build(),
            _ => inference_error::ProviderSnafu {
                status,
                message: truncate_body(body),
            }
            .build(),
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
            Self::Cancelled { .. } => "cancelled",
            Self::Overloaded { .. } => "overloaded",
            Self::InvalidRequest { .. } => "invalid_request",
            Self::UnknownProvider { .. } => "unknown_provider",
            Self::ProviderBuildFailed { .. } => "provider_build_failed",
            Self::ModelRoleUnresolved { .. } => "model_role_unresolved",
        }
    }
}

impl ErrorExt for InferenceError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::AuthenticationFailed { .. } => StatusCode::AuthenticationFailed,
            Self::RateLimited { .. } => StatusCode::RateLimited,
            Self::ContextWindowExceeded { .. } => StatusCode::ContextWindowExceeded,
            Self::ProviderError { .. } => StatusCode::ProviderError,
            Self::NetworkError { .. } => StatusCode::NetworkError,
            Self::StreamInterrupted { .. } => StatusCode::StreamError,
            Self::Cancelled { .. } => StatusCode::Cancelled,
            Self::Overloaded { .. } => StatusCode::ServiceUnavailable,
            Self::InvalidRequest { .. } => StatusCode::InvalidRequest,
            Self::UnknownProvider { .. } => StatusCode::ProviderNotFound,
            Self::ProviderBuildFailed { .. } => StatusCode::ProviderError,
            Self::ModelRoleUnresolved { .. } => StatusCode::InvalidConfig,
        }
    }

    fn retry_after(&self) -> Option<Duration> {
        self.retry_after_ms()
            .map(|ms| Duration::from_millis(ms as u64))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Backwards-compatible: pre-migration callers used `error.is_retryable()`
/// directly. `ErrorExt::is_retryable()` (default impl delegates to
/// `status_code().is_retryable()`) provides this; this inherent method
/// keeps `error.is_retryable()` ergonomic without `use coco_error::ErrorExt`
/// at every call site.
impl InferenceError {
    pub fn is_retryable(&self) -> bool {
        ErrorExt::is_retryable(self)
    }
}

fn truncate_body(body: &str) -> String {
    if body.len() > 500 {
        format!("{}...", &body[..500])
    } else {
        body.to_string()
    }
}

#[cfg(test)]
#[path = "errors.test.rs"]
mod tests;
