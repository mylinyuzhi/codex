use super::*;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_web_fetch() {
    let tool = WebFetchTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "url": "https://example.com",
        "prompt": "Extract the title"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
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

#[test]
fn test_tool_properties() {
    let tool = WebFetchTool::new();
    assert_eq!(tool.name(), "WebFetch");
    assert!(tool.is_concurrent_safe());
    assert!(!tool.is_read_only()); // Network access requires approval
    assert_eq!(tool.max_result_size_chars(), 100_000);
}
