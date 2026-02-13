use super::*;
use cocode_protocol::Feature;
use cocode_protocol::ToolResultContent;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[test]
fn test_feature_gate() {
    let tool = WebSearchTool::new();
    assert_eq!(tool.feature_gate(), Some(Feature::WebSearch));
}

#[tokio::test]
async fn test_web_search_too_short() {
    let tool = WebSearchTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "query": "a"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_web_search_with_domains() {
    let tool = WebSearchTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "query": "rust async",
        "allowed_domains": ["docs.rs", "doc.rust-lang.org"],
        "blocked_domains": ["stackoverflow.com"]
    });

    // This makes a real network call (DuckDuckGo) — the result may fail in CI
    // but the tool should not panic and should return a valid ToolOutput.
    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_ok());
}

#[test]
fn test_decode_duckduckgo_url() {
    let encoded = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=abc";
    let decoded = decode_duckduckgo_url(encoded);
    assert_eq!(decoded, "https://example.com");
}

#[test]
fn test_decode_duckduckgo_url_direct() {
    let url = "https://example.com/page";
    let decoded = decode_duckduckgo_url(url);
    assert_eq!(decoded, url);
}

#[test]
fn test_format_search_results() {
    let response = SearchResponse {
        query: "rust programming".to_string(),
        results: vec![
            SearchResult {
                title: "Rust Programming".to_string(),
                url: "https://rust-lang.org".to_string(),
                snippet: "A language empowering everyone".to_string(),
            },
            SearchResult {
                title: "Learn Rust".to_string(),
                url: "https://doc.rust-lang.org".to_string(),
                snippet: "Official documentation".to_string(),
            },
        ],
    };

    let formatted = format_search_results(&response);
    assert!(formatted.contains("[1]"));
    assert!(formatted.contains("[2]"));
    assert!(formatted.contains("Sources:"));
    assert!(formatted.contains("rust-lang.org"));
    assert!(formatted.contains("Rust Programming"));
}

#[test]
fn test_format_empty_results() {
    let response = SearchResponse {
        query: "xyznonexistent".to_string(),
        results: vec![],
    };

    let formatted = format_search_results(&response);
    assert!(formatted.contains("No search results found"));
    assert!(formatted.contains("xyznonexistent"));
}

#[test]
fn test_html_entities_decode() {
    assert_eq!(html_entities_decode("&amp;"), "&");
    assert_eq!(html_entities_decode("&lt;tag&gt;"), "<tag>");
    assert_eq!(html_entities_decode("&quot;quoted&quot;"), "\"quoted\"");
    assert_eq!(html_entities_decode("it&#39;s"), "it's");
    assert_eq!(html_entities_decode("a&nbsp;b"), "a b");
    assert_eq!(html_entities_decode("&#x27;"), "'");
    assert_eq!(html_entities_decode("&#x2F;"), "/");
}

#[test]
fn test_tool_properties() {
    let tool = WebSearchTool::new();
    assert_eq!(tool.name(), "WebSearch");
    assert!(tool.is_concurrent_safe());
    assert!(!tool.is_read_only()); // Network access requires approval
}

#[test]
fn test_error_type_as_str() {
    assert_eq!(WebSearchErrorType::ProviderError.as_str(), "PROVIDER_ERROR");
    assert_eq!(WebSearchErrorType::NetworkError.as_str(), "NETWORK_ERROR");
    assert_eq!(WebSearchErrorType::Timeout.as_str(), "TIMEOUT");
    assert_eq!(WebSearchErrorType::RateLimited.as_str(), "RATE_LIMITED");
    assert_eq!(
        WebSearchErrorType::ApiKeyMissing.as_str(),
        "API_KEY_MISSING"
    );
    assert_eq!(WebSearchErrorType::ParseError.as_str(), "PARSE_ERROR");
}

#[test]
fn test_make_error_response() {
    let result = make_error_response(WebSearchErrorType::NetworkError, "test error");
    match &result.content {
        ToolResultContent::Text(text) => {
            assert!(text.contains("[NETWORK_ERROR]"));
            assert!(text.contains("test error"));
        }
        _ => panic!("Expected text content"),
    }
}

#[test]
fn test_web_search_config_via_context() {
    let config = WebSearchConfig {
        provider: WebSearchProvider::Tavily,
        max_results: 10,
        api_key: Some("test-key".to_string()),
    };
    let mut ctx = make_context();
    ctx.web_search_config = config;
    assert_eq!(ctx.web_search_config.provider, WebSearchProvider::Tavily);
    assert_eq!(ctx.web_search_config.max_results, 10);
}
