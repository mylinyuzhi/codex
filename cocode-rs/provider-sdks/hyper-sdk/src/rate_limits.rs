//! Rate limit tracking for API responses.
//!
//! This module provides types for capturing and inspecting rate limit
//! information from HTTP response headers. Different providers use
//! different header formats, and this module normalizes them into
//! a common snapshot format.
//!
//! # Supported Providers
//!
//! - **OpenAI**: `x-ratelimit-remaining-requests`, `x-ratelimit-remaining-tokens`, etc.
//! - **Anthropic**: `anthropic-ratelimit-requests-remaining`, etc.
//! - **Generic**: Standard `Retry-After` header
//!
//! # Example
//!
//! ```ignore
//! use hyper_sdk::rate_limits::RateLimitSnapshot;
//! use reqwest::header::HeaderMap;
//!
//! let snapshot = RateLimitSnapshot::from_headers(&headers);
//! if let Some(s) = snapshot {
//!     if s.is_approaching_limit() {
//!         // Consider throttling requests
//!     }
//! }
//! ```

use http::HeaderMap;
use std::time::Duration;

/// Snapshot of rate limit state from HTTP response headers.
///
/// This captures the current rate limit status as reported by the provider.
/// Not all fields may be present depending on the provider.
#[derive(Debug, Clone, Default)]
pub struct RateLimitSnapshot {
    /// Remaining requests in the current window.
    pub remaining_requests: Option<i64>,
    /// Remaining tokens in the current window.
    pub remaining_tokens: Option<i64>,
    /// Seconds until the rate limit resets.
    pub reset_seconds: Option<f64>,
    /// Time to wait before retrying (from Retry-After header).
    pub retry_after: Option<Duration>,
}

impl RateLimitSnapshot {
    /// Parse rate limit information from HTTP response headers.
    ///
    /// Attempts to extract rate limit data from common header formats:
    /// - OpenAI style: `x-ratelimit-*`
    /// - Anthropic style: `anthropic-ratelimit-*`
    /// - Standard: `Retry-After`
    ///
    /// Returns `None` if no rate limit headers are found.
    pub fn from_headers(headers: &HeaderMap) -> Option<Self> {
        let remaining_requests = parse_i64_header(headers, "x-ratelimit-remaining-requests")
            .or_else(|| parse_i64_header(headers, "anthropic-ratelimit-requests-remaining"));

        let remaining_tokens = parse_i64_header(headers, "x-ratelimit-remaining-tokens")
            .or_else(|| parse_i64_header(headers, "anthropic-ratelimit-tokens-remaining"));

        let reset_seconds = parse_f64_header(headers, "x-ratelimit-reset-requests")
            .or_else(|| parse_reset_header(headers, "anthropic-ratelimit-requests-reset"));

        let retry_after = parse_retry_after_header(headers);

        // Only return Some if we found at least one piece of data
        if remaining_requests.is_some()
            || remaining_tokens.is_some()
            || reset_seconds.is_some()
            || retry_after.is_some()
        {
            Some(Self {
                remaining_requests,
                remaining_tokens,
                reset_seconds,
                retry_after,
            })
        } else {
            None
        }
    }

    /// Check if approaching rate limit (less than 10% remaining).
    ///
    /// Returns `true` if either remaining requests or tokens is below 10% of typical limits.
    /// Uses conservative thresholds:
    /// - Requests: < 10 remaining
    /// - Tokens: < 10000 remaining
    pub fn is_approaching_limit(&self) -> bool {
        if let Some(requests) = self.remaining_requests {
            if requests < 10 {
                return true;
            }
        }
        if let Some(tokens) = self.remaining_tokens {
            if tokens < 10000 {
                return true;
            }
        }
        false
    }

    /// Check if rate limit is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.remaining_requests == Some(0) || self.remaining_tokens == Some(0)
    }

    /// Get suggested wait duration before retrying.
    ///
    /// Returns the Retry-After value if present, otherwise estimates
    /// from reset_seconds if available.
    pub fn suggested_wait(&self) -> Option<Duration> {
        self.retry_after
            .or_else(|| self.reset_seconds.map(Duration::from_secs_f64))
    }
}

/// Parse an i64 from a header value.
fn parse_i64_header(headers: &HeaderMap, name: &str) -> Option<i64> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}

/// Parse an f64 from a header value.
fn parse_f64_header(headers: &HeaderMap, name: &str) -> Option<f64> {
    headers
        .get(name)?
        .to_str()
        .ok()?
        .parse()
        .ok()
        .filter(|v: &f64| v.is_finite())
}

/// Parse reset time from various formats.
///
/// Handles both numeric seconds and duration formats like "1m30s".
fn parse_reset_header(headers: &HeaderMap, name: &str) -> Option<f64> {
    let value = headers.get(name)?.to_str().ok()?;

    // Try parsing as plain number first
    if let Ok(seconds) = value.parse::<f64>() {
        return Some(seconds);
    }

    // Try parsing as duration format (e.g., "1m30s", "2h", "30s")
    parse_duration_string(value).map(|d| d.as_secs_f64())
}

/// Parse a duration string like "1m30s" or "2h".
fn parse_duration_string(s: &str) -> Option<Duration> {
    let mut total_secs: f64 = 0.0;
    let mut current_num = String::new();

    for c in s.chars() {
        if c.is_ascii_digit() || c == '.' {
            current_num.push(c);
        } else {
            let num: f64 = current_num.parse().ok()?;
            current_num.clear();

            match c.to_ascii_lowercase() {
                'h' => total_secs += num * 3600.0,
                'm' => total_secs += num * 60.0,
                's' => total_secs += num,
                _ => return None,
            }
        }
    }

    if total_secs > 0.0 {
        Some(Duration::from_secs_f64(total_secs))
    } else {
        None
    }
}

/// Parse the standard Retry-After header.
fn parse_retry_after_header(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get("retry-after")?.to_str().ok()?;

    // Try parsing as seconds
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    // Could also parse HTTP-date format, but that's more complex
    // and rarely used in practice for AI APIs
    None
}

#[cfg(test)]
#[path = "rate_limits.test.rs"]
mod tests;
