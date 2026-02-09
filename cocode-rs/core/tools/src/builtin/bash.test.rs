use super::*;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_bash_echo() {
    let tool = BashTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "command": "echo hello"
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
async fn test_bash_failure() {
    let tool = BashTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "command": "exit 1"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(result.is_error);
}

#[test]
fn test_is_read_only() {
    assert!(is_read_only_command("ls -la"));
    assert!(is_read_only_command("cat file.txt"));
    assert!(is_read_only_command("git status"));
    assert!(!is_read_only_command("rm -rf /"));
    assert!(!is_read_only_command("ls && rm foo"));
    assert!(!is_read_only_command("echo foo > bar"));
}

#[test]
fn test_tool_properties() {
    let tool = BashTool::new();
    assert_eq!(tool.name(), "Bash");
    assert!(!tool.is_concurrent_safe());
}
