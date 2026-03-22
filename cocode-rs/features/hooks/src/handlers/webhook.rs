//! Webhook handler: sends hook context to an HTTP endpoint.
//!
//! The webhook receives the full `HookContext` as JSON via HTTP POST and returns
//! a JSON response. The response format supports both:
//!
//! 1. `HookResult` (legacy format with `action` tag):
//!    ```json
//!    { "action": "continue" }
//!    { "action": "reject", "reason": "..." }
//!    { "action": "modify_input", "new_input": {...} }
//!    ```
//!
//! 2. `HookOutput` (Claude Code v2.1.7 format):
//!    ```json
//!    { "continue_execution": true }
//!    { "continue_execution": false, "stop_reason": "..." }
//!    { "continue_execution": true, "updated_input": {...} }
//!    ```
//!
//! ## Request Headers
//!
//! - `Content-Type: application/json`
//! - `User-Agent: cocode-hooks/1.0`
//! - `X-Hook-Event: <event_type>` (e.g., "pre_tool_use")
//! - `X-Hook-Tool-Name: <tool_name>` (if applicable)
//! - `X-Hook-Session-Id: <session_id>`
//!
//! ## Error Handling
//!
//! On any error (network, timeout, invalid response), the handler returns
//! `Continue` to allow execution to proceed. Errors are logged at warn level.

use std::collections::HashMap;
use std::time::Duration;

use tracing::debug;
use tracing::warn;

use super::command::HookOutput;
use crate::context::HookContext;
use crate::result::HookResult;

/// Default timeout for webhook requests (10 seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Handles hooks that call external webhooks via HTTP POST.
pub struct WebhookHandler;

impl WebhookHandler {
    /// Sends the `HookContext` as JSON to the given URL and parses the response.
    ///
    /// The request includes headers with event metadata for routing/filtering.
    /// On any error, returns `Continue` to avoid blocking execution.
    pub async fn execute(url: &str, context: &HookContext) -> (HookResult, bool) {
        Self::execute_with_timeout(url, context, DEFAULT_TIMEOUT_SECS).await
    }

    /// Execute webhook with custom headers and optional per-webhook timeout.
    pub async fn execute_with_options(
        url: &str,
        context: &HookContext,
        timeout_secs: Option<u64>,
        custom_headers: &HashMap<String, String>,
    ) -> (HookResult, bool) {
        let effective_timeout = timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
        Self::execute_internal(url, context, effective_timeout, custom_headers).await
    }

    /// Execute webhook with custom timeout (for testing).
    pub async fn execute_with_timeout(
        url: &str,
        context: &HookContext,
        timeout_secs: u64,
    ) -> (HookResult, bool) {
        Self::execute_internal(url, context, timeout_secs, &HashMap::new()).await
    }

    /// Internal execution with all options.
    async fn execute_internal(
        url: &str,
        context: &HookContext,
        timeout_secs: u64,
        custom_headers: &HashMap<String, String>,
    ) -> (HookResult, bool) {
        debug!(url, event_type = %context.event_type, "Executing webhook hook");

        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                warn!(url, error = %e, "Failed to create HTTP client for webhook");
                return (HookResult::Continue, false);
            }
        };

        let mut request = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "cocode-hooks/1.0")
            .header("X-Hook-Event", context.event_type.as_str())
            .header(
                "X-Hook-Tool-Name",
                context.tool_name.as_deref().unwrap_or(""),
            )
            .header("X-Hook-Session-Id", &context.session_id);

        // Apply custom headers
        for (key, value) in custom_headers {
            request = request.header(key.as_str(), value.as_str());
        }

        let response = match request.json(context).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(url, error = %e, "Webhook request failed");
                return (HookResult::Continue, false);
            }
        };

        let status = response.status();
        if !status.is_success() {
            warn!(
                url,
                status = %status,
                "Webhook returned non-success status"
            );
            return (HookResult::Continue, false);
        }

        let body = match response.text().await {
            Ok(b) => b,
            Err(e) => {
                warn!(url, error = %e, "Failed to read webhook response body");
                return (HookResult::Continue, false);
            }
        };

        if body.trim().is_empty() {
            return (HookResult::Continue, false);
        }

        parse_webhook_response(url, body.trim())
    }
}

/// Parses webhook response, supporting both `HookResult` and `HookOutput` formats.
///
/// Returns `(result, suppress_output)`. The `suppress_output` flag is only available
/// when parsing the `HookOutput` format (which has a `suppressOutput` field).
fn parse_webhook_response(url: &str, body: &str) -> (HookResult, bool) {
    // Try parsing as HookResult first (has "action" field)
    if let Ok(result) = serde_json::from_str::<HookResult>(body) {
        return (result, false);
    }

    // Try parsing as HookOutput (Claude Code v2.1.7 format with "continue_execution" field)
    if let Ok(output) = serde_json::from_str::<HookOutput>(body) {
        let suppress = output.suppress_output;
        return (output.into_result(None), suppress);
    }

    warn!(
        url,
        body = %body,
        "Failed to parse webhook response as HookResult or HookOutput"
    );
    (HookResult::Continue, false)
}

#[cfg(test)]
#[path = "webhook.test.rs"]
mod tests;
