use super::structured_tasks::StructuredTask;
use super::structured_tasks::TaskStatus;
use super::structured_tasks::{self};
use super::*;
use serde_json::json;
use std::path::PathBuf;

fn make_task(id: &str, subject: &str, status: TaskStatus) -> StructuredTask {
    StructuredTask {
        id: id.to_string(),
        subject: subject.to_string(),
        description: None,
        status,
        active_form: None,
        owner: None,
        blocks: Vec::new(),
        blocked_by: Vec::new(),
        metadata: serde_json::Value::Null,
    }
}

// ── Smart blocker filtering ────────────────────────────────────

#[tokio::test]
async fn test_smart_blocker_filtering() {
    let store = structured_tasks::new_task_store();
    {
        let mut s = store.lock().await;
        s.insert(
            "a".into(),
            make_task("a", "Done task", TaskStatus::Completed),
        );
        let mut b = make_task("b", "Blocked task", TaskStatus::Pending);
        b.blocked_by = vec!["a".into(), "c".into()];
        s.insert("b".into(), b);
        s.insert(
            "c".into(),
            make_task("c", "Pending blocker", TaskStatus::Pending),
        );
    }

    let tool = TaskListTool::new(store);
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    let input = json!({ "status": "all" });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t.clone(),
        _ => panic!("Expected text"),
    };

    // Task b should show blocked_by: c only (a is completed, filtered out)
    assert!(text.contains("blocked by: c"), "got: {text}");
    assert!(!text.contains("blocked by: a"), "got: {text}");
}

// ── Internal task filtering ────────────────────────────────────

#[tokio::test]
async fn test_internal_task_hidden() {
    let store = structured_tasks::new_task_store();
    {
        let mut s = store.lock().await;
        s.insert(
            "visible".into(),
            make_task("visible", "Visible task", TaskStatus::Pending),
        );
        let mut hidden = make_task("hidden", "Internal task", TaskStatus::Pending);
        hidden.metadata = json!({"_internal": true});
        s.insert("hidden".into(), hidden);
    }

    let tool = TaskListTool::new(store);
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    let input = json!({ "status": "all" });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t.clone(),
        _ => panic!("Expected text"),
    };

    assert!(text.contains("visible"), "got: {text}");
    assert!(!text.contains("hidden"), "got: {text}");
    assert!(!text.contains("Internal task"), "got: {text}");
}
