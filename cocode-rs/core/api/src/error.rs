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

    /// Underlying SDK error.
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

    /// Check if this is an overload or rate-limit error.
    ///
    /// Used to decide whether model fallback should be attempted.
    /// Matches Claude Code's `iF6(isOverloadedError)` which only triggers
    /// fallback on 529/overloaded or 429/rate-limited — NOT on transient
    /// network errors or stream errors.
    pub fn is_overload_or_rate_limit(&self) -> bool {
        matches!(
            self,
            ApiError::Overloaded { .. } | ApiError::RateLimited { .. }
        )
    }

    /// Check if this is a stream-related error that should trigger fallback.
    pub fn is_stream_error(&self) -> bool {
        matches!(
            self,
            ApiError::Stream { .. } | ApiError::StreamIdleTimeout { .. }
        )
    }

    /// Parse token information from a context overflow error message.
    ///
    /// Returns `Some` if the error is a `ContextOverflow` and token info
    /// can be extracted from the message. Used by the API client to
    /// calculate the exact available output space during overflow recovery.
    pub fn overflow_info(&self) -> Option<ContextOverflowInfo> {
        match self {
            ApiError::ContextOverflow { message, .. } => {
                let info = parse_overflow_info(message);
                if info.has_recovery_info() {
                    Some(info)
                } else {
                    None
                }
            }
            _ => None,
        }
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

/// Classify an AISdkError from vercel-ai into a specific ApiError variant.
///
/// First inspects the error cause chain for structured `APICallError` fields
/// (status_code, is_retryable, retry_after). Falls back to message-based
/// keyword heuristics only for untyped errors.
pub fn classify_sdk_error(err: &crate::AISdkError) -> ApiError {
    // Step 1: Try to extract structured error info from the cause chain.
    if let Some(cause) = &err.cause
        && let Some(provider_err) = cause.downcast_ref::<vercel_ai_provider::ProviderError>()
        && let vercel_ai_provider::ProviderError::ApiCall(api_call) = provider_err
    {
        return classify_api_call_error(api_call);
    }

    // Step 2: Fall back to message-based heuristics.
    classify_by_message(&err.message)
}

/// Classify using structured `APICallError` fields.
fn classify_api_call_error(err: &vercel_ai_provider::APICallError) -> ApiError {
    use api_error::*;
    let msg = &err.message;
    let lower = msg.to_ascii_lowercase();

    // Context overflow is typically 400 with specific message patterns
    if is_context_overflow_message(&lower) {
        return ContextOverflowSnafu {
            message: msg.clone(),
        }
        .build();
    }

    let retry_after_ms = err
        .retry_after
        .map(|d| d.as_millis() as i64)
        .unwrap_or(1000);

    match err.status_code {
        Some(401 | 403) => AuthenticationSnafu {
            message: msg.clone(),
        }
        .build(),
        // P14: OpenAI models sometimes transiently return 404 during deployment.
        // Treating as network error makes it retryable. Non-OpenAI 404s are genuinely
        // not-found and retrying them just fails again harmlessly.
        Some(404) => NetworkSnafu {
            message: msg.clone(),
        }
        .build(),
        // P13: HTTP 413 "Request Entity Too Large" from proxies indicates context overflow.
        Some(413) => ContextOverflowSnafu {
            message: msg.clone(),
        }
        .build(),
        Some(429) => RateLimitedSnafu {
            message: msg.clone(),
            retry_after_ms,
        }
        .build(),
        Some(500 | 502 | 503 | 529) => OverloadedSnafu {
            message: msg.clone(),
            retry_after_ms,
        }
        .build(),
        _ => {
            // P17: Try extracting a better message from response_body before
            // falling to heuristic classification on the original message.
            let effective_msg =
                extract_message_from_response_body(err).unwrap_or_else(|| msg.clone());
            classify_by_message(&effective_msg)
        }
    }
}

/// Message-based heuristic classification (fallback for untyped errors).
///
/// Public so that downstream crates (e.g., `core/loop`) can classify
/// mid-stream error messages into structured `ApiError` variants.
pub fn classify_by_message(msg: &str) -> ApiError {
    use api_error::*;
    let lower = msg.to_ascii_lowercase();

    // Stream idle timeout
    if lower.contains("stream idle timeout") {
        let secs = lower
            .split("after ")
            .nth(1)
            .and_then(|s| s.split('s').next())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(60);
        return StreamIdleTimeoutSnafu { timeout_secs: secs }.build();
    }

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
        "401",
    ];
    if AUTH_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return AuthenticationSnafu {
            message: msg.to_string(),
        }
        .build();
    }

    // Context window overflow keywords
    if is_context_overflow_message(&lower) {
        return ContextOverflowSnafu {
            message: msg.to_string(),
        }
        .build();
    }

    // Rate limit keywords
    const RATE_LIMIT_KEYWORDS: &[&str] = &[
        "rate limit",
        "rate_limit",
        "too many requests",
        "throttled",
        "429",
    ];
    if RATE_LIMIT_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return RateLimitedSnafu {
            message: msg.to_string(),
            retry_after_ms: 1000_i64,
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
            message: msg.to_string(),
        }
        .build();
    }

    // Stream error keywords
    if lower.contains("stream error") || lower.contains("stream closed") {
        return StreamSnafu {
            message: msg.to_string(),
        }
        .build();
    }

    // Network error keywords
    const NETWORK_KEYWORDS: &[&str] = &[
        "connection",
        "timeout",
        "dns",
        "network",
        "reset",
        "refused",
    ];
    if NETWORK_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return NetworkSnafu {
            message: msg.to_string(),
        }
        .build();
    }

    // Overloaded / server error keywords (includes 500, 502 for heuristic fallback)
    const OVERLOADED_KEYWORDS: &[&str] = &["overloaded", "503", "529", "500", "502", "bad gateway"];
    if OVERLOADED_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return OverloadedSnafu {
            message: msg.to_string(),
            retry_after_ms: 1000_i64,
        }
        .build();
    }

    // Default: SDK error
    SdkSnafu {
        message: msg.to_string(),
    }
    .build()
}

/// Check if a lowercased message indicates context overflow.
fn is_context_overflow_message(lower: &str) -> bool {
    const CONTEXT_KEYWORDS: &[&str] = &[
        // Core patterns (original)
        "context length",
        "context window",
        "token limit",
        "input too long",
        "too many tokens",
        "context_length_exceeded",
        "maximum context",
        "max_tokens",
        "tokens exceeded",
        // P13: Additional provider-specific patterns
        "prompt is too long",
        "maximum prompt length",
        "reduce the length of the messages",
        "request entity too large",
        "exceeds the available context size",
        "exceeds the limit of",
    ];
    if CONTEXT_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return true;
    }

    // Compound pattern: Google Gemini "input token count ... exceeds the maximum"
    if lower.contains("input token count") && lower.contains("exceeds the maximum") {
        return true;
    }

    false
}

/// Extract a useful error message from an API response body.
///
/// Providers return error details in various formats:
/// - JSON: `{ "error": { "message": "..." } }` or `{ "error": "..." }` or `{ "message": "..." }`
/// - HTML: gateway pages (502, 503, etc.)
///
/// Returns `None` if no useful message can be extracted.
fn extract_message_from_response_body(err: &vercel_ai_provider::APICallError) -> Option<String> {
    let body = err.response_body.as_deref()?;

    // Try JSON extraction
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        // { "error": { "message": "..." } }
        if let Some(msg) = json
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return Some(msg.to_string());
        }
        // { "error": "..." }
        if let Some(msg) = json.get("error").and_then(|e| e.as_str()) {
            return Some(msg.to_string());
        }
        // { "message": "..." }
        if let Some(msg) = json.get("message").and_then(|m| m.as_str()) {
            return Some(msg.to_string());
        }
    }

    // HTML gateway pages
    let trimmed = body.trim_start();
    if trimmed.starts_with("<html")
        || trimmed.starts_with("<!DOCTYPE")
        || trimmed.starts_with("<!doctype")
    {
        if let Some(status) = err.status_code {
            return Some(format!("HTTP {status} gateway error"));
        }
        return Some("Gateway error (HTML response)".to_string());
    }

    None
}

impl From<crate::AISdkError> for ApiError {
    fn from(err: crate::AISdkError) -> Self {
        classify_sdk_error(&err)
    }
}

// =============================================================================
// Context Overflow Info Parsing
// =============================================================================

/// Token information extracted from a context overflow error message.
///
/// Providers embed token counts in error messages using different formats.
/// This struct captures whatever values can be parsed, with `None` for
/// values that couldn't be extracted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextOverflowInfo {
    /// Number of input tokens reported by the provider.
    pub input_tokens: Option<i64>,
    /// Maximum tokens value from the request.
    pub max_tokens: Option<i64>,
    /// Total context window limit for the model.
    pub context_limit: Option<i64>,
}

impl ContextOverflowInfo {
    /// Check if enough information is available for smart recovery.
    ///
    /// Returns `true` if at least `input_tokens` and `context_limit` are known,
    /// which allows calculating the exact available space.
    pub fn has_recovery_info(&self) -> bool {
        self.input_tokens.is_some() && self.context_limit.is_some()
    }
}

/// Parse token information from a context overflow error message.
///
/// Handles known provider patterns:
/// - **Anthropic**: `"...exceed context limit: {input} + {max} > {limit}"`
/// - **OpenAI**: `"maximum context length is {limit} tokens...resulted in {input} tokens"`
/// - **Gemini**: `"input token count of {input} exceeds the maximum...for model"`
///
/// Falls back to generic number extraction when no pattern matches.
pub fn parse_overflow_info(message: &str) -> ContextOverflowInfo {
    // Try Anthropic pattern: "...context limit: {input} + {max} > {limit}"
    if let Some(info) = parse_anthropic_overflow(message) {
        return info;
    }
    // Try OpenAI pattern: "maximum context length is {limit}...resulted in {input} tokens"
    if let Some(info) = parse_openai_overflow(message) {
        return info;
    }
    // Try Gemini pattern: "input token count of {input} exceeds"
    if let Some(info) = parse_gemini_overflow(message) {
        return info;
    }
    ContextOverflowInfo::default()
}

/// Anthropic: `"...context limit: 50000 + 4096 > 200000"`
fn parse_anthropic_overflow(message: &str) -> Option<ContextOverflowInfo> {
    // Look for the pattern with ": {n} + {n} > {n}"
    let marker = "context limit:";
    let idx = message.to_ascii_lowercase().find(marker)?;
    let after = &message[idx + marker.len()..];
    let nums = extract_numbers(after);
    if nums.len() >= 3 {
        Some(ContextOverflowInfo {
            input_tokens: Some(nums[0]),
            max_tokens: Some(nums[1]),
            context_limit: Some(nums[2]),
        })
    } else {
        None
    }
}

/// OpenAI: `"maximum context length is {limit} tokens. However, your messages resulted in {input} tokens"`
fn parse_openai_overflow(message: &str) -> Option<ContextOverflowInfo> {
    let lower = message.to_ascii_lowercase();
    if !lower.contains("maximum context length") {
        return None;
    }
    let nums = extract_numbers(message);
    if nums.len() >= 2 {
        Some(ContextOverflowInfo {
            context_limit: Some(nums[0]),
            input_tokens: Some(nums[1]),
            max_tokens: None,
        })
    } else if nums.len() == 1 {
        Some(ContextOverflowInfo {
            context_limit: Some(nums[0]),
            input_tokens: None,
            max_tokens: None,
        })
    } else {
        None
    }
}

/// Gemini: `"input token count of {input} exceeds the maximum"`
fn parse_gemini_overflow(message: &str) -> Option<ContextOverflowInfo> {
    let lower = message.to_ascii_lowercase();
    if !lower.contains("input token count") {
        return None;
    }
    let nums = extract_numbers(message);
    match nums.len() {
        0 => None,
        1 => Some(ContextOverflowInfo {
            input_tokens: Some(nums[0]),
            max_tokens: None,
            context_limit: None,
        }),
        _ => Some(ContextOverflowInfo {
            input_tokens: Some(nums[0]),
            max_tokens: None,
            context_limit: Some(nums[1]),
        }),
    }
}

/// Extract all integer sequences from a string.
fn extract_numbers(s: &str) -> Vec<i64> {
    let mut nums = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut num_str = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    num_str.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if let Ok(n) = num_str.parse::<i64>() {
                nums.push(n);
            }
        } else {
            chars.next();
        }
    }
    nums
}

/// Result type for API operations.
pub type Result<T> = std::result::Result<T, ApiError>;

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
