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
    ctx.services
        .shell_executor
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
    ctx.services
        .shell_executor
        .background_registry
        .register("task-stopped".to_string(), process)
        .await;
    ctx.services
        .shell_executor
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

// ── delta read tests ──

#[tokio::test]
async fn test_delta_read_first_call() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    // Write some entries
    tokio::fs::write(
        &path,
        concat!(
            r#"{"type":"progress","message":"Starting"}"#,
            "\n",
            r#"{"status":"completed","output":"Done"}"#,
            "\n"
        ),
    )
    .await
    .unwrap();

    let offsets = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
        String,
        u64,
    >::new()));

    let result = read_agent_output_delta("a1", &path, &offsets).await;
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("a1"), "Should contain task ID");
            assert!(t.contains("completed"), "Should show status");
            assert!(t.contains("Starting"), "Should include progress");
            assert!(t.contains("Done"), "Should include output");
            assert!(
                !t.contains("(new)"),
                "First read should not show delta label"
            );
        }
        _ => panic!("Expected text content"),
    }

    // Offset should be recorded
    let off = offsets.lock().await;
    assert!(*off.get("a1").unwrap() > 0);
}

#[tokio::test]
async fn test_delta_read_incremental() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    // Write initial entry
    tokio::fs::write(
        &path,
        r#"{"status":"running","output":"Step 1"}"#.to_owned() + "\n",
    )
    .await
    .unwrap();

    let offsets = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
        String,
        u64,
    >::new()));

    // First read
    let _ = read_agent_output_delta("a2", &path, &offsets).await;

    // Append new entry
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .await
        .unwrap();
    file.write_all((r#"{"status":"completed","output":"Step 2"}"#.to_owned() + "\n").as_bytes())
        .await
        .unwrap();

    // Second read should only get new entry
    let result = read_agent_output_delta("a2", &path, &offsets).await;
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("(new)"), "Delta read should show (new) label");
            assert!(t.contains("Step 2"), "Should contain new entry");
            assert!(!t.contains("Step 1"), "Should not contain old entry");
        }
        _ => panic!("Expected text content"),
    }
}

#[tokio::test]
async fn test_delta_read_no_new_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    tokio::fs::write(
        &path,
        r#"{"status":"running","output":"Only"}"#.to_owned() + "\n",
    )
    .await
    .unwrap();

    let offsets = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
        String,
        u64,
    >::new()));

    // First read
    let _ = read_agent_output_delta("a3", &path, &offsets).await;

    // Second read with no new data
    let result = read_agent_output_delta("a3", &path, &offsets).await;
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("no new output"), "Should indicate no new output");
        }
        _ => panic!("Expected text content"),
    }
}

// ── format_agent_output tests ──

#[test]
fn test_format_agent_output_single_entry() {
    let content = r#"{"status":"completed","agent_id":"a1","output":"Done"}"#;
    let result = format_agent_output("a1", content);
    assert!(!result.is_error);
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("a1"));
            assert!(t.contains("completed"));
            assert!(t.contains("Done"));
        }
        _ => panic!("Expected text content"),
    }
}

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
fn test_format_agent_output_multi_entry_jsonl() {
    let content = concat!(
        r#"{"type":"progress","agent_id":"a1","message":"Starting"}"#,
        "\n",
        r#"{"type":"turn_result","agent_id":"a1","text":"Step 1"}"#,
        "\n",
        r#"{"status":"completed","agent_id":"a1","output":"All done"}"#,
        "\n"
    );
    let result = format_agent_output("a1", content);
    assert!(!result.is_error);
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("completed"), "Should show last status");
            assert!(t.contains("Starting"), "Should include progress message");
            assert!(t.contains("Step 1"), "Should include turn text");
            assert!(t.contains("All done"), "Should include final output");
        }
        _ => panic!("Expected text content"),
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
fn test_format_agent_output_empty_content() {
    let result = format_agent_output("a1", "");
    assert!(!result.is_error);
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("a1"));
        }
        _ => panic!("Expected text content"),
    }
}

#[test]
fn test_format_agent_output_error_entries() {
    let content = r#"{"status":"failed","agent_id":"a1","error":"Something broke"}"#;
    let result = format_agent_output("a1", content);
    match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => {
            assert!(t.contains("failed"));
            assert!(t.contains("[error] Something broke"));
        }
        _ => panic!("Expected text content"),
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
