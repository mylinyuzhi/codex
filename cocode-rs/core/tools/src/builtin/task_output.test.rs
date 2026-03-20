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
        cancel_token: tokio_util::sync::CancellationToken::new(),
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
            assert!(t.contains("test output"), "got: {t}");
            // GAP 5: command should appear in header
            assert!(
                t.contains("echo test"),
                "expected command in header, got: {t}"
            );
        }
        _ => panic!("Expected text content"),
    }
}

#[tokio::test]
async fn test_task_output_tool_completed_after_stop() {
    let tool = TaskOutputTool::new();
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));

    // Register and immediately stop a task (output should be preserved)
    let output = Arc::new(Mutex::new("final output\n".to_string()));
    let process = BackgroundProcess {
        id: "task-stopped".to_string(),
        command: "cargo build".to_string(),
        output,
        completed: Arc::new(Notify::new()),
        cancel_token: tokio_util::sync::CancellationToken::new(),
    };
    ctx.shell_executor
        .background_registry
        .register("task-stopped".to_string(), process)
        .await;
    ctx.shell_executor
        .background_registry
        .stop("task-stopped")
        .await;

    let input = serde_json::json!({
        "task_id": "task-stopped",
        "block": false,
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("final output"), "got: {t}");
            assert!(
                t.contains("cargo build"),
                "expected command in header, got: {t}"
            );
            assert!(t.contains("completed"), "got: {t}");
        }
        _ => panic!("Expected text content"),
    }
}

#[test]
fn test_tool_properties() {
    let tool = TaskOutputTool::new();
    assert_eq!(tool.name(), "TaskOutput");
    assert!(tool.is_concurrent_safe());
    assert!(tool.is_read_only());
}

// ── JSONL parsing ──────────────────────────────────────────────

#[test]
fn test_format_agent_output_jsonl_multiline() {
    // Agent output files are JSONL — multiple lines
    let content = r#"{"status":"running","output":"starting..."}
{"status":"completed","output":"all done"}"#;
    let result = format_agent_output("agent-1", content);
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("completed"), "got: {t}");
            assert!(t.contains("all done"), "got: {t}");
        }
        _ => panic!("Expected text"),
    }
}

#[test]
fn test_format_agent_output_single_json() {
    // Backward compat: single JSON object (not multi-line)
    let content = r#"{"status":"completed","output":"result"}"#;
    let result = format_agent_output("agent-1", content);
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("completed"), "got: {t}");
            assert!(t.contains("result"), "got: {t}");
        }
        _ => panic!("Expected text"),
    }
}

#[test]
fn test_format_agent_output_raw_fallback() {
    let content = "some raw output that isn't JSON";
    let result = format_agent_output("agent-1", content);
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("some raw output"), "got: {t}");
        }
        _ => panic!("Expected text"),
    }
}
