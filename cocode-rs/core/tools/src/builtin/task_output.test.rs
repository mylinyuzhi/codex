use super::*;
use cocode_shell::BackgroundProcess;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::Notify;

#[tokio::test]
async fn test_task_output_tool_not_found() {
    let tool = TaskOutputTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    let input = serde_json::json!({
        "task_id": "task-nonexistent",
        "block": false,
        "timeout": 100
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    // Non-existent task returns error
    assert!(result.is_error);
}

#[tokio::test]
async fn test_task_output_tool_with_task() {
    let tool = TaskOutputTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    // Register a background task
    let output = Arc::new(Mutex::new("test output".to_string()));
    let process = BackgroundProcess {
        id: "task-123".to_string(),
        command: "echo test".to_string(),
        output,
        completed: Arc::new(Notify::new()),
    };
    ctx.shell_executor
        .background_registry
        .register("task-123".to_string(), process)
        .await;

    let input = serde_json::json!({
        "task_id": "task-123",
        "block": false,
        "timeout": 100
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("test output"));
        }
        _ => panic!("Expected text content"),
    }
}

#[test]
fn test_tool_properties() {
    let tool = TaskOutputTool::new();
    assert_eq!(tool.name(), "TaskOutput");
    assert!(tool.is_concurrent_safe());
}
