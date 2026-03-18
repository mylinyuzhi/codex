use super::*;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_task_tool() {
    let tool = TaskTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "description": "Search codebase",
        "prompt": "Find all error handling code",
        "subagent_type": "Explore"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
}

#[test]
fn test_tool_properties() {
    let tool = TaskTool::new();
    assert_eq!(tool.name(), cocode_protocol::ToolName::Task.as_str());
    // Default concurrency is Unsafe (foreground), but background is Safe
    assert!(!tool.is_concurrent_safe());
    assert!(!tool.is_concurrency_safe_for(&serde_json::json!({})));
    assert!(!tool.is_concurrency_safe_for(&serde_json::json!({"run_in_background": false})));
    assert!(tool.is_concurrency_safe_for(&serde_json::json!({"run_in_background": true})));
}
