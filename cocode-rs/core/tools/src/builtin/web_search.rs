//! WebSearch tool for searching the web.
//!
//! Executes web searches via DuckDuckGo or Tavily backends.
//! Returns formatted markdown results with citation markers.
//! Includes LRU cache with TTL to reduce API calls.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ApprovalRequest;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::PermissionResult;
use cocode_protocol::ToolOutput;
use cocode_protocol::WebSearchConfig;
use cocode_protocol::WebSearchProvider;
use lru::LruCache;
use serde::Deserialize;
use serde_json::Value;
use std::num::NonZeroUsize;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

// Constants
const SEARCH_TIMEOUT_SECS: u64 = 15;
const CACHE_SIZE: usize = 100;
const CACHE_TTL_SECS: u64 = 15 * 60; // 15 minutes

/// Static HTTP client for connection pooling
#[allow(clippy::expect_used)]
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(SEARCH_TIMEOUT_SECS))
        .user_agent("cocode-web-search/1.0")
        .build()
        .expect("Failed to create HTTP client")
});

/// Cached search result with timestamp
struct CachedResult {
    response: SearchResponse,
    cached_at: Instant,
}

/// LRU cache for search results with TTL
#[allow(clippy::expect_used)]
static SEARCH_CACHE: LazyLock<Mutex<LruCache<String, CachedResult>>> = LazyLock::new(|| {
    Mutex::new(LruCache::new(
        NonZeroUsize::new(CACHE_SIZE).expect("CACHE_SIZE must be > 0"),
    ))
});

/// Get cached search result if not expired
fn get_cached(
    query: &str,
    provider: WebSearchProvider,
    max_results: usize,
) -> Option<SearchResponse> {
    let key = format!("{provider:?}:{max_results}:{query}");
    let mut cache = SEARCH_CACHE.lock().ok()?;
    if let Some(cached) = cache.get(&key) {
        if cached.cached_at.elapsed() < Duration::from_secs(CACHE_TTL_SECS) {
            tracing::debug!("web_search cache hit for: {}", query);
            return Some(cached.response.clone());
        }
        // Expired - remove from cache
        cache.pop(&key);
    }
    None
}

/// Store search result in cache
fn set_cached(
    query: &str,
    provider: WebSearchProvider,
    max_results: usize,
    response: SearchResponse,
) {
    let key = format!("{provider:?}:{max_results}:{query}");
    if let Ok(mut cache) = SEARCH_CACHE.lock() {
        cache.put(
            key,
            CachedResult {
                response,
                cached_at: Instant::now(),
            },
        );
    }
}

/// Error types for web_search operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebSearchErrorType {
    ProviderError,
    NetworkError,
    Timeout,
    RateLimited,
    ApiKeyMissing,
    ParseError,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
impl WebSearchErrorType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::ProviderError => "PROVIDER_ERROR",
            Self::NetworkError => "NETWORK_ERROR",
            Self::Timeout => "TIMEOUT",
            Self::RateLimited => "RATE_LIMITED",
            Self::ApiKeyMissing => "API_KEY_MISSING",
            Self::ParseError => "PARSE_ERROR",
        }
    }
}

/// A single search result
#[derive(Debug, Clone)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

/// Search response with results and metadata
#[derive(Debug, Clone)]
struct SearchResponse {
    results: Vec<SearchResult>,
    query: String,
}

/// Tool for performing web searches.
///
/// Searches the web using configured provider (DuckDuckGo or Tavily).
/// Includes LRU cache with TTL to reduce API calls.
/// Configuration is read from `ToolContext.web_search_config` at execute-time.
pub struct WebSearchTool;

impl WebSearchTool {
    /// Create a new WebSearch tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }

    fn description(&self) -> &str {
        prompts::WEB_SEARCH_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query to use",
                    "minLength": 2
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (1-20)",
                    "minimum": 1,
                    "maximum": 20
                },
                "allowed_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only include search results from these domains"
                },
                "blocked_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Never include search results from these domains"
                }
            },
            "required": ["query"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        false // Network access requires approval
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::WebSearch)
    }

    async fn check_permission(&self, input: &Value, _ctx: &ToolContext) -> PermissionResult {
        let query = match input.get("query").and_then(|v| v.as_str()) {
            Some(q) => q,
            None => return PermissionResult::Passthrough,
        };

        PermissionResult::NeedsApproval {
            request: ApprovalRequest {
                request_id: format!("websearch-{}", query.len()),
                tool_name: self.name().to_string(),
                description: format!("Web search: {query}"),
                risks: vec![],
                allow_remember: true,
                proposed_prefix_pattern: None,
            },
        }
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let config = &ctx.web_search_config;

        // 1. Parse query
        let query = input["query"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "query must be a string",
            }
            .build()
        })?;

        let query = query.trim();
        if query.len() < 2 {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "query must be at least 2 characters",
            }
            .build());
        }

        // 2. Determine max_results (clamp to valid range)
        let max_results = input
            .get("max_results")
            .and_then(serde_json::Value::as_i64)
            .map(|n| n as usize)
            .unwrap_or(config.max_results)
            .clamp(1, 20);

        ctx.emit_progress(format!("Searching: {query}")).await;

        // 3. Check cache first
        if let Some(cached_response) = get_cached(query, config.provider, max_results) {
            let content = format_search_results(&cached_response);
            return Ok(ToolOutput::text(content));
        }

        // 4. Execute search based on provider
        let response = match config.provider {
            WebSearchProvider::DuckDuckGo => search_duckduckgo(query, max_results).await,
            WebSearchProvider::Tavily => search_tavily(query, max_results, config).await,
            WebSearchProvider::OpenAI => {
                // OpenAI native search not implemented - use DuckDuckGo fallback
                search_duckduckgo(query, max_results).await
            }
        };

        // 5. Format and return results, cache successful responses
        match response {
            Ok(search_response) => {
                // Apply domain filtering if specified
                let search_response = apply_domain_filters(&input, search_response);

                // Cache successful result
                set_cached(query, config.provider, max_results, search_response.clone());
                let content = format_search_results(&search_response);
                Ok(ToolOutput::text(content))
            }
            Err((error_type, message)) => Ok(make_error_response(error_type, &message)),
        }
    }
}

/// Apply allowed_domains / blocked_domains filters to search results.
fn apply_domain_filters(input: &Value, mut response: SearchResponse) -> SearchResponse {
    let allowed: Vec<&str> = input
        .get("allowed_domains")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let blocked: Vec<&str> = input
        .get("blocked_domains")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    if allowed.is_empty() && blocked.is_empty() {
        return response;
    }

    response.results.retain(|r| {
        let url = &r.url;
        if !allowed.is_empty() && !allowed.iter().any(|d| url.contains(d)) {
            return false;
        }
        if blocked.iter().any(|d| url.contains(d)) {
            return false;
        }
        true
    });

    response
}

/// URL-encode a query string for use in URLs
fn url_encode(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// DuckDuckGo search implementation using HTML scraping
async fn search_duckduckgo(
    query: &str,
    max_results: usize,
) -> std::result::Result<SearchResponse, (WebSearchErrorType, String)> {
    let url = format!("https://html.duckduckgo.com/html/?q={}", url_encode(query));

    let response = HTTP_CLIENT
        .get(&url)
        .header("Accept", "text/html")
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                (WebSearchErrorType::Timeout, "Search timed out".to_string())
            } else {
                (
                    WebSearchErrorType::NetworkError,
                    format!("Network error: {e}"),
                )
            }
        })?;

    if !response.status().is_success() {
        return Err((
            WebSearchErrorType::ProviderError,
            format!("DuckDuckGo returned status {}", response.status()),
        ));
    }

    let html = response.text().await.map_err(|e| {
        (
            WebSearchErrorType::ParseError,
            format!("Failed to read response: {e}"),
        )
    })?;

    parse_duckduckgo_html(&html, query, max_results)
}

/// Parse DuckDuckGo HTML results page
#[allow(clippy::unwrap_used)]
fn parse_duckduckgo_html(
    html: &str,
    query: &str,
    max_results: usize,
) -> std::result::Result<SearchResponse, (WebSearchErrorType, String)> {
    let mut results = Vec::new();

    // Pattern for result links: class="result__a" href="..." followed by link text
    let link_re =
        regex_lite::Regex::new(r#"class="result__a"[^>]*href="([^"]+)"[^>]*>([^<]+)</a>"#).unwrap();

    // Pattern for snippets
    let snippet_re = regex_lite::Regex::new(r#"class="result__snippet"[^>]*>([^<]+)"#).unwrap();

    let mut links: Vec<(String, String)> = Vec::new();
    for cap in link_re.captures_iter(html) {
        let url = decode_duckduckgo_url(&cap[1]);
        let title = html_entities_decode(&cap[2]);
        if !url.is_empty() && !title.is_empty() {
            links.push((url, title));
        }
    }

    let snippets: Vec<String> = snippet_re
        .captures_iter(html)
        .map(|cap| html_entities_decode(&cap[1]))
        .collect();

    // Combine links and snippets
    for (idx, (url, title)) in links.into_iter().take(max_results).enumerate() {
        let snippet = snippets.get(idx).cloned().unwrap_or_default();
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }

    Ok(SearchResponse {
        results,
        query: query.to_string(),
    })
}

/// Simple percent-decode for URL paths
fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2
                && let Ok(byte) = u8::from_str_radix(&hex, 16)
            {
                result.push(byte as char);
                continue;
            }
            result.push('%');
            result.push_str(&hex);
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

/// Decode DuckDuckGo redirect URLs
fn decode_duckduckgo_url(encoded: &str) -> String {
    // DuckDuckGo uses //duckduckgo.com/l/?uddg=URL format
    if let Some(uddg_start) = encoded.find("uddg=") {
        let url_part = &encoded[uddg_start + 5..];
        if let Some(end) = url_part.find('&') {
            return percent_decode(&url_part[..end]);
        }
        return percent_decode(url_part);
    }
    // If not a redirect URL, return as-is
    encoded.to_string()
}

/// Tavily API response types
#[derive(Debug, Deserialize)]
struct TavilyResponse {
    results: Vec<TavilyResult>,
}

#[derive(Debug, Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: String,
    #[allow(dead_code)]
    score: f64,
}

/// Tavily search implementation using REST API
async fn search_tavily(
    query: &str,
    max_results: usize,
    config: &WebSearchConfig,
) -> std::result::Result<SearchResponse, (WebSearchErrorType, String)> {
    // Get API key: config takes precedence, then env var
    let api_key = config
        .api_key
        .clone()
        .or_else(|| std::env::var("TAVILY_API_KEY").ok())
        .ok_or_else(|| {
            (
                WebSearchErrorType::ApiKeyMissing,
                "TAVILY_API_KEY not set. Configure in [tools.web_search_config] api_key = \"...\" \
                 or set TAVILY_API_KEY env var. Get key at https://tavily.com"
                    .to_string(),
            )
        })?;

    let request_body = serde_json::json!({
        "api_key": api_key,
        "query": query,
        "max_results": max_results,
        "search_depth": "basic",
        "include_answer": false,
        "include_raw_content": false,
    });

    let response = HTTP_CLIENT
        .post("https://api.tavily.com/search")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                (WebSearchErrorType::Timeout, "Search timed out".to_string())
            } else {
                (
                    WebSearchErrorType::NetworkError,
                    format!("Network error: {e}"),
                )
            }
        })?;

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err((
            WebSearchErrorType::RateLimited,
            "Tavily API rate limit exceeded".to_string(),
        ));
    }
    if !status.is_success() {
        return Err((
            WebSearchErrorType::ProviderError,
            format!("Tavily API returned status {status}"),
        ));
    }

    let tavily_response: TavilyResponse = response.json().await.map_err(|e| {
        (
            WebSearchErrorType::ParseError,
            format!("Failed to parse response: {e}"),
        )
    })?;

    let results = tavily_response
        .results
        .into_iter()
        .map(|r| SearchResult {
            title: r.title,
            url: r.url,
            snippet: r.content,
        })
        .collect();

    Ok(SearchResponse {
        results,
        query: query.to_string(),
    })
}

/// Format search results as markdown with citation markers
fn format_search_results(response: &SearchResponse) -> String {
    if response.results.is_empty() {
        return format!(
            "No search results found for \"{}\"\n\n\
             Try rephrasing your query or using different keywords.",
            response.query
        );
    }

    let mut output = format!("Web search results for \"{}\":\n\n", response.query);

    // Format each result with citation marker
    for (idx, result) in response.results.iter().enumerate() {
        let citation_num = idx + 1;
        output.push_str(&format!(
            "[{citation_num}] **{}**\n{}\nSource: {}\n\n",
            result.title,
            result.snippet.trim(),
            result.url
        ));
    }

    // Add sources footer
    output.push_str("Sources:\n");
    for (idx, result) in response.results.iter().enumerate() {
        output.push_str(&format!(
            "[{}] {} ({})\n",
            idx + 1,
            result.title,
            result.url
        ));
    }

    output
}

/// Create standardized error response as ToolOutput
fn make_error_response(error_type: WebSearchErrorType, message: &str) -> ToolOutput {
    ToolOutput::text(format!("[{}] {}", error_type.as_str(), message))
}

/// Decode HTML entities
fn html_entities_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
}

#[cfg(test)]
#[path = "web_search.test.rs"]
mod tests;
