use super::*;
use cocode_shell::BackgroundProcess;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::Notify;

#[tokio::test]
async fn test_kill_shell_tool_not_found() {
    let tool = KillShellTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    let input = serde_json::json!({
        "task_id": "task-nonexistent"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    // Non-existent task returns error
    assert!(result.is_error);
}

#[tokio::test]
async fn test_kill_shell_tool_stops_task() {
    let tool = KillShellTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    // Register a background task
    let output = Arc::new(Mutex::new("task output".to_string()));
    let process = BackgroundProcess {
        id: "task-123".to_string(),
        command: "sleep 60".to_string(),
        output,
        completed: Arc::new(Notify::new()),
    };
    ctx.shell_executor
        .background_registry
        .register("task-123".to_string(), process)
        .await;

    // Verify task is running
    assert!(
        ctx.shell_executor
            .background_registry
            .is_running("task-123")
            .await
    );

    let input = serde_json::json!({
        "task_id": "task-123"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("stopped successfully"));
            assert!(t.contains("task output"));
        }
        _ => panic!("Expected text content"),
    }

    // Verify task is no longer running
    assert!(
        !ctx.shell_executor
            .background_registry
            .is_running("task-123")
            .await
    );
}

#[test]
fn test_tool_properties() {
    let tool = KillShellTool::new();
    assert_eq!(tool.name(), "TaskStop");
    assert!(tool.is_concurrent_safe());
}
