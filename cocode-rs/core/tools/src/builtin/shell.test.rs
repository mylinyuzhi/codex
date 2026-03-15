use super::*;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_shell_echo() {
    let tool = ShellTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "command": ["echo", "hello"]
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(content.contains("hello"));
    assert!(!result.is_error);
}

#[tokio::test]
async fn test_shell_failure() {
    let tool = ShellTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "command": ["false"]
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(result.is_error);
}

#[tokio::test]
async fn test_shell_empty_command() {
    let tool = ShellTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "command": []
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[test]
fn test_tool_properties() {
    let tool = ShellTool::new();
    assert_eq!(tool.name(), "shell");
    assert!(!tool.is_concurrent_safe());
    assert_eq!(tool.max_result_size_chars(), 30_000);
}
