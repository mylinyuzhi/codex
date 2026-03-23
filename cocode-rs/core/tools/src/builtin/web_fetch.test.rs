use super::*;
use cocode_protocol::Feature;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

// ========== Helper function tests ==========

#[test]
fn test_transform_github_url_blob() {
    let url = "https://github.com/user/repo/blob/main/file.txt";
    let transformed = transform_github_url(url);
    assert_eq!(
        transformed,
        "https://raw.githubusercontent.com/user/repo/main/file.txt"
    );
}

#[test]
fn test_transform_github_url_non_blob() {
    let url = "https://github.com/user/repo";
    let transformed = transform_github_url(url);
    assert_eq!(transformed, url);
}

#[test]
fn test_transform_non_github_url() {
    let url = "https://example.com/page";
    let transformed = transform_github_url(url);
    assert_eq!(transformed, url);
}

#[test]
fn test_transform_github_url_nested_blob() {
    let url = "https://github.com/org/repo/blob/feature/branch/src/main.rs";
    let transformed = transform_github_url(url);
    assert_eq!(
        transformed,
        "https://raw.githubusercontent.com/org/repo/feature/branch/src/main.rs"
    );
}

// ========== HTML to Text Tests ==========

#[test]
fn test_html_to_text_simple() {
    let html = "<html><body><h1>Hello</h1><p>World</p></body></html>";
    let text = html_to_text(html);
    assert!(text.contains("Hello"));
    assert!(text.contains("World"));
}

#[test]
fn test_html_to_text_strips_tags() {
    let html = "<p><strong>Bold</strong> and <em>italic</em></p>";
    let text = html_to_text(html);
    assert!(text.contains("Bold"));
    assert!(text.contains("italic"));
    assert!(!text.contains("<strong>"));
    assert!(!text.contains("<em>"));
}

#[test]
fn test_html_to_text_plain_text() {
    let plain = "Just some plain text without any HTML";
    let text = html_to_text(plain);
    assert!(text.contains("Just some plain text"));
}

// ========== Tool property tests ==========

#[test]
fn test_tool_properties() {
    let tool = WebFetchTool::new();
    assert_eq!(tool.name(), cocode_protocol::ToolName::WebFetch.as_str());
    assert!(tool.is_concurrent_safe());
    assert!(tool.is_read_only());
    assert_eq!(tool.max_result_size_chars(), 100_000);
}

#[test]
fn test_feature_gate() {
    let tool = WebFetchTool::new();
    assert_eq!(tool.feature_gate(), Some(Feature::WebFetch));
}

// ========== Execute tests ==========

#[tokio::test]
async fn test_web_fetch_empty_url() {
    let tool = WebFetchTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "url": "   ",
        "prompt": "Extract the title"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_web_fetch_invalid_url() {
    let tool = WebFetchTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "url": "not-a-url",
        "prompt": "Extract the title"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_web_fetch_ftp_rejected() {
    let tool = WebFetchTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "url": "ftp://example.com/file",
        "prompt": "Extract the title"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_web_fetch_missing_url() {
    let tool = WebFetchTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "prompt": "Extract the title"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_web_fetch_missing_prompt() {
    let tool = WebFetchTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "url": "https://example.com"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

// ========== Permission tests ==========

#[tokio::test]
async fn test_permission_preapproved_hosts() {
    let tool = WebFetchTool::new();
    let ctx = make_context();

    // github.com should be preapproved
    let input = serde_json::json!({ "url": "https://github.com/user/repo" });
    assert!(matches!(
        tool.check_permission(&input, &ctx).await,
        PermissionResult::Allowed
    ));

    // docs.rs should be preapproved
    let input = serde_json::json!({ "url": "https://docs.rs/serde/latest/serde/" });
    assert!(matches!(
        tool.check_permission(&input, &ctx).await,
        PermissionResult::Allowed
    ));
}

#[tokio::test]
async fn test_permission_unknown_host() {
    let tool = WebFetchTool::new();
    let ctx = make_context();

    let input = serde_json::json!({ "url": "https://evil-site.com/payload" });
    assert!(matches!(
        tool.check_permission(&input, &ctx).await,
        PermissionResult::NeedsApproval { .. }
    ));
}

// ========== Static HTTP Client Test ==========

#[test]
fn test_static_http_client_is_accessible() {
    let _ = &*HTTP_CLIENT;
}

// ========== Redirect helper tests ==========

#[test]
fn test_extract_hostname_https() {
    assert_eq!(extract_hostname("https://example.com/path"), "example.com");
}

#[test]
fn test_extract_hostname_with_port() {
    assert_eq!(
        extract_hostname("https://example.com:8080/path"),
        "example.com"
    );
}

#[test]
fn test_extract_hostname_http() {
    assert_eq!(extract_hostname("http://localhost/path"), "localhost");
}

#[test]
fn test_extract_hostname_no_scheme() {
    assert_eq!(extract_hostname("just-a-string"), "");
}

#[test]
fn test_extract_hostname_no_path() {
    assert_eq!(extract_hostname("https://example.com"), "example.com");
}

#[test]
fn test_resolve_redirect_url_absolute() {
    let resolved = resolve_redirect_url("https://example.com/old", "https://other.com/new");
    assert_eq!(resolved, "https://other.com/new");
}

#[test]
fn test_resolve_redirect_url_relative_path() {
    let resolved = resolve_redirect_url("https://example.com/old/page", "/new-path");
    assert_eq!(resolved, "https://example.com/new-path");
}

#[test]
fn test_resolve_redirect_url_relative_no_leading_slash() {
    let resolved = resolve_redirect_url("https://example.com/old", "new-path");
    assert_eq!(resolved, "new-path");
}

#[test]
fn test_resolve_redirect_url_preserves_port() {
    let resolved = resolve_redirect_url("https://example.com:8080/old", "/new");
    assert_eq!(resolved, "https://example.com:8080/new");
}
