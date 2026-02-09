use super::*;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_web_search() {
    let tool = WebSearchTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "query": "rust programming language"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
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

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
}

#[test]
fn test_tool_properties() {
    let tool = WebSearchTool::new();
    assert_eq!(tool.name(), "WebSearch");
    assert!(tool.is_concurrent_safe());
    assert!(!tool.is_read_only()); // Network access requires approval
}
