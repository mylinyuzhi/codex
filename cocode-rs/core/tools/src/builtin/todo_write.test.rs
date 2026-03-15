use super::*;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_todo_write() {
    let tool = TodoWriteTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "todos": [
            {"id": "1", "subject": "Fix bug", "status": "completed", "activeForm": "Fixing bug"},
            {"id": "2", "subject": "Add tests", "status": "in_progress", "activeForm": "Adding tests"},
            {"id": "3", "subject": "Deploy", "status": "pending", "activeForm": "Deploying"}
        ]
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(text.contains("[x]"));
    assert!(text.contains("[>]"));
    assert!(text.contains("[ ]"));
}

#[tokio::test]
async fn test_todo_write_with_legacy_content() {
    let tool = TodoWriteTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "todos": [
            {"id": "1", "content": "Fix bug", "status": "completed"},
            {"id": "2", "content": "Add tests", "status": "pending"}
        ]
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
}

#[tokio::test]
async fn test_todo_write_auto_id() {
    let tool = TodoWriteTool::new();
    let mut ctx = make_context();

    // No id field — should auto-generate
    let input = serde_json::json!({
        "todos": [
            {"subject": "Fix bug", "status": "pending"}
        ]
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(text.contains("1:"));
}

#[tokio::test]
async fn test_todo_write_max_in_progress() {
    let tool = TodoWriteTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "todos": [
            {"subject": "Task 1", "status": "in_progress", "activeForm": "Working on Task 1"},
            {"subject": "Task 2", "status": "in_progress", "activeForm": "Working on Task 2"}
        ]
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[test]
fn test_tool_properties() {
    let tool = TodoWriteTool::new();
    assert_eq!(tool.name(), "TodoWrite");
    assert!(!tool.is_concurrent_safe());
}
