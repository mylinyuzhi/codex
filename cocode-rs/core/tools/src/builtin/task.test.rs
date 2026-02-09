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
    assert_eq!(tool.name(), "Task");
    assert!(tool.is_concurrent_safe());
}
