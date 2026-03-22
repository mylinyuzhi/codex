//! WebFetch tool for fetching and processing web content.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::PermissionResult;
use cocode_protocol::ToolOutput;
use serde_json::Value;
use std::sync::LazyLock;
use std::time::Duration;

/// Maximum result size for web content (characters).
const MAX_RESULT_CHARS: i32 = 100_000;

/// Maximum line width for HTML→text conversion.
const MAX_LINE_WIDTH: usize = 120;

/// Static HTTP client for connection pooling.
///
/// Uses a generous default timeout; per-request timeouts are set from config.
#[allow(clippy::expect_used)]
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("Failed to create HTTP client")
});

/// Tool for fetching content from a URL.
///
/// Fetches the URL, converts HTML to text, and returns the content.
pub struct WebFetchTool;

impl WebFetchTool {
    /// Create a new WebFetch tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::WebFetch.as_str()
    }

    fn description(&self) -> &str {
        prompts::WEB_FETCH_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri",
                    "description": "The URL to fetch content from"
                },
                "prompt": {
                    "type": "string",
                    "description": "The prompt to run on the fetched content"
                }
            },
            "required": ["url", "prompt"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn max_result_size_chars(&self) -> i32 {
        MAX_RESULT_CHARS
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::WebFetch)
    }

    async fn check_permission(&self, input: &Value, _ctx: &ToolContext) -> PermissionResult {
        let url = match input.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return PermissionResult::Passthrough,
        };

        // Extract hostname from URL
        let hostname = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .and_then(|rest| rest.split('/').next())
            .unwrap_or(url);

        // Preapproved hosts that don't need permission
        const PREAPPROVED_HOSTS: &[&str] = &[
            "docs.rs",
            "crates.io",
            "doc.rust-lang.org",
            "docs.python.org",
            "developer.mozilla.org",
            "en.wikipedia.org",
            "stackoverflow.com",
            "github.com",
            "raw.githubusercontent.com",
        ];

        if PREAPPROVED_HOSTS
            .iter()
            .any(|h| hostname == *h || hostname.ends_with(&format!(".{h}")))
        {
            return PermissionResult::Allowed;
        }

        // All other domains → NeedsApproval
        PermissionResult::NeedsApproval {
            request: ApprovalRequest {
                request_id: format!("webfetch-{hostname}"),
                tool_name: self.name().to_string(),
                description: format!("Fetch URL: {url}"),
                risks: vec![],
                allow_remember: true,
                proposed_prefix_pattern: None,
            },
        }
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let url = input["url"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "url must be a string",
            }
            .build()
        })?;
        let prompt = input["prompt"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "prompt must be a string",
            }
            .build()
        })?;

        // Validate URL is not empty
        if url.trim().is_empty() {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "URL must not be empty",
            }
            .build());
        }

        // Validate URL scheme
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "url must start with http:// or https://",
            }
            .build());
        }

        let config = &ctx.web_fetch_config;
        let max_content_length = config.max_content_length;

        ctx.emit_progress(format!("Fetching {url}")).await;

        // Transform GitHub blob URLs to raw URLs
        let fetch_url = transform_github_url(url);

        // Fetch with timeout from config
        let response = match HTTP_CLIENT
            .get(&fetch_url)
            .header("User-Agent", &config.user_agent)
            .timeout(Duration::from_secs(config.timeout_secs))
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                if e.is_timeout() {
                    return Ok(ToolOutput::error(format!(
                        "[TIMEOUT] Request timed out after {} seconds",
                        config.timeout_secs
                    )));
                }
                return Ok(ToolOutput::error(format!("[NETWORK_ERROR] {e}")));
            }
        };

        // Handle redirects — Policy::none() prevents auto-follow to avoid
        // authorization header leaks and SSRF on cross-host redirects.
        let response = {
            let status = response.status();
            if status == reqwest::StatusCode::MOVED_PERMANENTLY
                || status == reqwest::StatusCode::TEMPORARY_REDIRECT
                || status == reqwest::StatusCode::PERMANENT_REDIRECT
            {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok());

                match location {
                    Some(loc) => {
                        let resolved = resolve_redirect_url(&fetch_url, loc);
                        let original_host = extract_hostname(&fetch_url);
                        let redirect_host = extract_hostname(&resolved);

                        if original_host == redirect_host {
                            // Same host → follow once
                            match HTTP_CLIENT
                                .get(&resolved)
                                .header("User-Agent", &config.user_agent)
                                .timeout(Duration::from_secs(config.timeout_secs))
                                .send()
                                .await
                            {
                                Ok(resp) => resp,
                                Err(e) => {
                                    if e.is_timeout() {
                                        return Ok(ToolOutput::error(format!(
                                            "[TIMEOUT] Redirect timed out after {} seconds",
                                            config.timeout_secs
                                        )));
                                    }
                                    return Ok(ToolOutput::error(format!(
                                        "[NETWORK_ERROR] Redirect failed: {e}"
                                    )));
                                }
                            }
                        } else {
                            // Cross-host → return redirect message for model to re-fetch
                            return Ok(ToolOutput::text(format!(
                                "The URL has been redirected to a different domain.\n\
                                 Original: {fetch_url}\n\
                                 Redirect: {resolved}\n\n\
                                 To fetch the content, call this tool again with the new URL: {resolved}"
                            )));
                        }
                    }
                    None => {
                        return Ok(ToolOutput::error(
                            "[REDIRECT_ERROR] Redirect response missing Location header"
                                .to_string(),
                        ));
                    }
                }
            } else {
                response
            }
        };

        // Check HTTP status
        let status = response.status();
        if !status.is_success() {
            return Ok(ToolOutput::error(format!(
                "[HTTP_ERROR] {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        // Check Content-Length to prevent OOM on huge responses
        if let Some(content_length) = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            && content_length > max_content_length * 2
        {
            return Ok(ToolOutput::error(format!(
                "[CONTENT_TOO_LARGE] Content too large: {} bytes (max: {} bytes)",
                content_length,
                max_content_length * 2
            )));
        }

        // Get content type
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        // Get response body
        let body = match response.text().await {
            Ok(text) => text,
            Err(e) => {
                return Ok(ToolOutput::error(format!(
                    "[NETWORK_ERROR] Failed to read response body: {e}"
                )));
            }
        };

        // Convert HTML to text if needed
        let text_content = if content_type.contains("text/html") || content_type.is_empty() {
            html_to_text(&body)
        } else {
            body
        };

        // Truncate if needed (UTF-8 safe)
        let truncated = if text_content.len() > max_content_length {
            let truncated_content = truncate_utf8_safe(&text_content, max_content_length);
            format!(
                "{}\n\n[Content truncated. Showing first {} of {} bytes]",
                truncated_content,
                truncated_content.len(),
                text_content.len()
            )
        } else {
            text_content
        };

        // Return content with context
        Ok(ToolOutput::text(format!(
            "Content from {fetch_url}:\nPrompt: {prompt}\n\n{truncated}"
        )))
    }
}

/// Extract hostname from URL, excluding port and path.
fn extract_hostname(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .and_then(|rest| rest.split('/').next())
        .and_then(|host_port| host_port.split(':').next())
        .unwrap_or("")
}

/// Resolve a redirect Location header to an absolute URL.
fn resolve_redirect_url(original_url: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_string();
    }
    if location.starts_with('/') {
        let scheme_end = original_url.find("://").map(|i| i + 3).unwrap_or(0);
        let host_end = original_url[scheme_end..]
            .find('/')
            .map(|i| i + scheme_end)
            .unwrap_or(original_url.len());
        return format!("{}{location}", &original_url[..host_end]);
    }
    location.to_string()
}

/// Transform GitHub blob URLs to raw.githubusercontent.com URLs.
fn transform_github_url(url: &str) -> String {
    if url.contains("github.com") && url.contains("/blob/") {
        url.replace("github.com", "raw.githubusercontent.com")
            .replace("/blob/", "/")
    } else {
        url.to_string()
    }
}

/// Truncate string at a valid UTF-8 character boundary.
fn truncate_utf8_safe(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &s[..boundary]
}

/// Convert HTML to plain text using html2text.
fn html_to_text(html: &str) -> String {
    html2text::from_read(html.as_bytes(), MAX_LINE_WIDTH).unwrap_or_else(|_| html.to_string())
}

#[cfg(test)]
#[path = "web_fetch.test.rs"]
mod tests;
