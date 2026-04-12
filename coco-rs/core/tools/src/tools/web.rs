use coco_tool::DescriptionOptions;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

/// Maximum response size before truncation (characters).
const MAX_FETCH_LENGTH: usize = 100_000;

pub struct WebFetchTool;

#[async_trait::async_trait]
impl Tool for WebFetchTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::WebFetch)
    }
    fn name(&self) -> &str {
        ToolName::WebFetch.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Fetches content from a URL and returns the response body as text.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "url".into(),
            serde_json::json!({"type": "string", "description": "The URL to fetch content from"}),
        );
        p.insert(
            "prompt".into(),
            serde_json::json!({"type": "string", "description": "The prompt to run on the fetched content"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let url = input.get("url").and_then(|v| v.as_str())?;
        let truncated: String = url.chars().take(47).collect();
        let display = if truncated.len() < url.len() {
            format!("Fetching {truncated}...")
        } else {
            format!("Fetching {url}")
        };
        Some(display)
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if url.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "url parameter is required".into(),
                error_code: None,
            });
        }

        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if prompt.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "prompt parameter is required".into(),
                error_code: None,
            });
        }

        let body = fetch_url(url)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to fetch {url}: {e}"),
                source: None,
            })?;

        // Truncate if needed, then return content with the prompt for the
        // model to process.  The TS version sends both content and prompt
        // through a secondary LLM call; here we return the raw content
        // alongside the user prompt so the calling model can do extraction.
        let content = if body.len() > MAX_FETCH_LENGTH {
            let slice = &body[..MAX_FETCH_LENGTH];
            format!("{slice}\n\n[Content truncated at {MAX_FETCH_LENGTH} characters]")
        } else {
            body
        };

        Ok(ToolResult {
            data: serde_json::json!({
                "url": url,
                "prompt": prompt,
                "content": content,
            }),
            new_messages: vec![],
        })
    }
}

/// Fetch URL content using reqwest, falling back to curl on failure.
async fn fetch_url(url: &str) -> Result<String, String> {
    match fetch_with_reqwest(url).await {
        Ok(body) => Ok(body),
        Err(reqwest_err) => {
            tracing::debug!("reqwest failed for {url}, falling back to curl: {reqwest_err}");
            fetch_with_curl(url).await
        }
    }
}

async fn fetch_with_reqwest(url: &str) -> Result<String, String> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    // Reject clearly binary content types
    if content_type.contains("image/")
        || content_type.contains("audio/")
        || content_type.contains("video/")
        || content_type.contains("application/octet-stream")
        || content_type.contains("application/zip")
    {
        return Err(format!(
            "Non-text content type: {content_type}. Only text-based content is supported."
        ));
    }

    response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {e}"))
}

async fn fetch_with_curl(url: &str) -> Result<String, String> {
    let output = tokio::process::Command::new("curl")
        .args(["-sL", "--max-time", "30", url])
        .output()
        .await
        .map_err(|e| format!("Failed to run curl: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("curl failed: {stderr}"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub struct WebSearchTool;

#[async_trait::async_trait]
impl Tool for WebSearchTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::WebSearch)
    }
    fn name(&self) -> &str {
        ToolName::WebSearch.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Searches the web for information using a search query.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "query".into(),
            serde_json::json!({"type": "string", "description": "The search query to use"}),
        );
        p.insert(
            "allowed_domains".into(),
            serde_json::json!({
                "type": "array",
                "items": {"type": "string"},
                "description": "Only include search results from these domains"
            }),
        );
        p.insert(
            "blocked_domains".into(),
            serde_json::json!({
                "type": "array",
                "items": {"type": "string"},
                "description": "Never include search results from these domains"
            }),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> coco_tool::ValidationResult {
        let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        if query.len() < 2 {
            return coco_tool::ValidationResult::invalid(
                "query must be at least 2 characters long",
            );
        }
        coco_tool::ValidationResult::Valid
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let query = input.get("query").and_then(|v| v.as_str())?;
        Some(format!("Searching for \"{query}\""))
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if query.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "query parameter is required".into(),
                error_code: None,
            });
        }

        let message = format!(
            "Web search for \"{query}\" is not available. \
             To enable web search, configure a search API provider in your settings. \
             Alternatively, use the WebFetch tool with a specific URL to retrieve content."
        );

        Ok(ToolResult {
            data: serde_json::json!(message),
            new_messages: vec![],
        })
    }
}
