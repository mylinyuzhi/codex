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

// ── Bidirectional dependencies ─────────────────────────────────

#[tokio::test]
async fn test_add_blocks_updates_both_sides() {
    let store = structured_tasks::new_task_store();
    {
        let mut s = store.lock().await;
        s.insert("a".into(), make_task("a", "Task A", TaskStatus::Pending));
        s.insert("b".into(), make_task("b", "Task B", TaskStatus::Pending));
    }

    let tool = TaskUpdateTool::new(store.clone());
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    let input = json!({ "taskId": "a", "addBlocks": ["b"] });
    tool.execute(input, &mut ctx).await.unwrap();

    let s = store.lock().await;
    assert!(s["a"].blocks.contains(&"b".to_string()));
    assert!(s["b"].blocked_by.contains(&"a".to_string()));
}

#[tokio::test]
async fn test_add_blocked_by_updates_both_sides() {
    let store = structured_tasks::new_task_store();
    {
        let mut s = store.lock().await;
        s.insert("a".into(), make_task("a", "Task A", TaskStatus::Pending));
        s.insert("b".into(), make_task("b", "Task B", TaskStatus::Pending));
    }

    let tool = TaskUpdateTool::new(store.clone());
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    let input = json!({ "taskId": "a", "addBlockedBy": ["b"] });
    tool.execute(input, &mut ctx).await.unwrap();

    let s = store.lock().await;
    assert!(s["a"].blocked_by.contains(&"b".to_string()));
    assert!(s["b"].blocks.contains(&"a".to_string()));
}

#[tokio::test]
async fn test_remove_blocks_updates_both_sides() {
    let store = structured_tasks::new_task_store();
    {
        let mut s = store.lock().await;
        let mut a = make_task("a", "Task A", TaskStatus::Pending);
        let mut b = make_task("b", "Task B", TaskStatus::Pending);
        a.blocks.push("b".into());
        b.blocked_by.push("a".into());
        s.insert("a".into(), a);
        s.insert("b".into(), b);
    }

    let tool = TaskUpdateTool::new(store.clone());
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    let input = json!({ "taskId": "a", "removeBlocks": ["b"] });
    tool.execute(input, &mut ctx).await.unwrap();

    let s = store.lock().await;
    assert!(!s["a"].blocks.contains(&"b".to_string()));
    assert!(!s["b"].blocked_by.contains(&"a".to_string()));
}

// ── Cascading cleanup on deletion ──────────────────────────────

#[tokio::test]
async fn test_delete_cascading_cleanup() {
    let store = structured_tasks::new_task_store();
    {
        let mut s = store.lock().await;
        let mut a = make_task("a", "Task A", TaskStatus::Pending);
        let mut b = make_task("b", "Task B", TaskStatus::Pending);
        a.blocks.push("b".into());
        b.blocked_by.push("a".into());
        s.insert("a".into(), a);
        s.insert("b".into(), b);
    }

    let tool = TaskUpdateTool::new(store.clone());
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    let input = json!({ "taskId": "a", "status": "deleted" });
    tool.execute(input, &mut ctx).await.unwrap();

    let s = store.lock().await;
    assert!(matches!(s["a"].status, TaskStatus::Deleted));
    // b should no longer reference a in blocked_by
    assert!(!s["b"].blocked_by.contains(&"a".to_string()));
}

// ── Status transition validation ───────────────────────────────

#[tokio::test]
async fn test_invalid_transition_rejected() {
    let store = structured_tasks::new_task_store();
    {
        let mut s = store.lock().await;
        let mut t = make_task("a", "Task A", TaskStatus::Completed);
        t.status = TaskStatus::Completed;
        s.insert("a".into(), t);
    }

    let tool = TaskUpdateTool::new(store.clone());
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    // completed → pending is invalid
    let input = json!({ "taskId": "a", "status": "pending" });
    let err = tool.execute(input, &mut ctx).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Invalid status transition"), "got: {msg}");
}

#[tokio::test]
async fn test_valid_transition_succeeds() {
    let store = structured_tasks::new_task_store();
    {
        let mut s = store.lock().await;
        s.insert("a".into(), make_task("a", "Task A", TaskStatus::InProgress));
    }

    let tool = TaskUpdateTool::new(store.clone());
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    let input = json!({ "taskId": "a", "status": "completed" });
    tool.execute(input, &mut ctx).await.unwrap();

    let s = store.lock().await;
    assert!(matches!(s["a"].status, TaskStatus::Completed));
}

// ── Metadata null key deletion ─────────────────────────────────

#[tokio::test]
async fn test_metadata_null_deletes_key() {
    let store = structured_tasks::new_task_store();
    {
        let mut s = store.lock().await;
        let mut t = make_task("a", "Task A", TaskStatus::Pending);
        t.metadata = json!({"keep": 1, "remove": 2});
        s.insert("a".into(), t);
    }

    let tool = TaskUpdateTool::new(store.clone());
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    let input = json!({ "taskId": "a", "metadata": { "remove": null, "add": 3 } });
    tool.execute(input, &mut ctx).await.unwrap();

    let s = store.lock().await;
    let meta = &s["a"].metadata;
    assert_eq!(meta["keep"], 1);
    assert!(meta.get("remove").is_none());
    assert_eq!(meta["add"], 3);
}
