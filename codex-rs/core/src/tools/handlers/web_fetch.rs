//! WebFetch tool handler for fetching and processing web content.

use async_trait::async_trait;
use codex_protocol::config_types::WebFetchConfig;
use regex_lite::Regex;
use reqwest::Client;
use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;

use crate::error::Result as CodexResult;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

const MAX_URLS: usize = 20;

// Compile-time validated regex pattern for URL extraction
// Note: expect() is acceptable here as this is a hardcoded pattern that never fails
static URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://[^\s<>)\]]+").expect("URL regex pattern is hardcoded and must be valid")
});

pub struct WebFetchHandler {
    client: Client,
    config: WebFetchConfig,
}

impl WebFetchHandler {
    pub fn new(config: WebFetchConfig) -> CodexResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .user_agent(&config.user_agent)
            .build()
            .map_err(|e| {
                crate::error::CodexErr::Fatal(format!("failed to create HTTP client: {e}"))
            })?;

        Ok(Self { client, config })
    }

    /// Parse URLs from the prompt text. Returns up to MAX_URLS URLs.
    fn parse_urls(prompt: &str) -> Vec<String> {
        URL_REGEX
            .find_iter(prompt)
            .take(MAX_URLS)
            .map(|m| m.as_str().to_string())
            .collect()
    }

    /// Convert GitHub blob URLs to raw.githubusercontent.com URLs for direct file access.
    fn convert_github_url(url: &str) -> String {
        if url.contains("github.com") && url.contains("/blob/") {
            url.replace("github.com", "raw.githubusercontent.com")
                .replace("/blob/", "/")
        } else {
            url.to_string()
        }
    }

    /// Fetch content from a URL and convert HTML to plain text.
    async fn fetch_url(&self, url: &str) -> Result<String, String> {
        // Validate protocol (only http and https)
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err("Invalid protocol. Only http:// and https:// are supported.".to_string());
        }

        // Convert GitHub URLs if needed
        let fetch_url = Self::convert_github_url(url);

        // Fetch content
        let response = self
            .client
            .get(&fetch_url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch URL: {e}"))?;

        // Check status
        if !response.status().is_success() {
            return Err(format!(
                "HTTP error {}: {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("Unknown")
            ));
        }

        // Get content type to determine if it's HTML
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .unwrap_or_default();

        // Get response body
        let body = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {e}"))?;

        // Convert HTML to text if it's HTML content
        let text = if content_type.contains("text/html") {
            self.html_to_text(&body)
        } else {
            body
        };

        // Truncate to max length
        let truncated = if text.len() > self.config.max_content_length {
            let truncated_text = text
                .chars()
                .take(self.config.max_content_length)
                .collect::<String>();
            format!(
                "{}\n\n[Content truncated at {} characters]",
                truncated_text, self.config.max_content_length
            )
        } else {
            text
        };

        Ok(truncated)
    }

    /// Convert HTML to plain text using html2text.
    fn html_to_text(&self, html: &str) -> String {
        html2text::from_read(html.as_bytes(), usize::MAX)
    }
}

#[derive(Deserialize)]
struct WebFetchArgs {
    prompt: String,
}

#[async_trait]
impl ToolHandler for WebFetchHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation { payload, .. } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "web_fetch handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: WebFetchArgs = serde_json::from_str(&arguments).map_err(|e| {
            FunctionCallError::RespondToModel(format!("failed to parse function arguments: {e:?}"))
        })?;

        // Parse URLs from prompt
        let urls = Self::parse_urls(&args.prompt);

        if urls.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "No valid URLs found in prompt. Please provide URLs starting with http:// or https://".to_string(),
            ));
        }

        // Fetch all URLs
        let mut results = Vec::new();
        let mut errors = Vec::new();

        for url in &urls {
            match self.fetch_url(url).await {
                Ok(content) => results.push((url.clone(), content)),
                Err(e) => errors.push((url.clone(), e)),
            }
        }

        // Format output
        let mut output = String::new();

        // Add successful results
        for (i, (url, content)) in results.iter().enumerate() {
            if i > 0 {
                output.push_str("\n\n---\n\n");
            }
            output.push_str(&format!("# URL: {url}\n\n{content}"));
        }

        // Add errors if any
        if !errors.is_empty() {
            if !results.is_empty() {
                output.push_str("\n\n---\n\n");
            }
            output.push_str("# Errors:\n\n");
            for (url, error) in errors {
                output.push_str(&format!("- {url}: {error}\n"));
            }
        }

        let success = !results.is_empty();

        Ok(ToolOutput::Function {
            content: output,
            content_items: None,
            success: Some(success),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_urls() {
        let prompt = "Fetch https://example.com and http://test.com/page";
        let urls = WebFetchHandler::parse_urls(prompt);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com");
        assert_eq!(urls[1], "http://test.com/page");
    }

    #[test]
    fn test_parse_urls_max_limit() {
        let mut prompt = String::new();
        for i in 0..25 {
            prompt.push_str(&format!("https://example{}.com ", i));
        }
        let urls = WebFetchHandler::parse_urls(&prompt);
        assert_eq!(urls.len(), MAX_URLS);
    }

    #[test]
    fn test_convert_github_url() {
        let url = "https://github.com/user/repo/blob/main/file.rs";
        let converted = WebFetchHandler::convert_github_url(url);
        assert_eq!(
            converted,
            "https://raw.githubusercontent.com/user/repo/main/file.rs"
        );

        // Non-GitHub URL should remain unchanged
        let url2 = "https://example.com/file.html";
        let converted2 = WebFetchHandler::convert_github_url(url2);
        assert_eq!(converted2, url2);
    }

    #[test]
    fn test_parse_urls_with_brackets() {
        let prompt = "Check [https://example.com] and (http://test.com)";
        let urls = WebFetchHandler::parse_urls(prompt);
        assert_eq!(urls.len(), 2);
    }
}
