use super::structured_tasks;
use super::*;
use serde_json::json;
use std::path::PathBuf;

// ── derive_active_form ──────────────────────────────────────────

#[test]
fn test_derive_active_form_common_verbs() {
    assert_eq!(
        structured_tasks::derive_active_form("Fix auth bug"),
        "Fixing auth bug"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Add logging"),
        "Adding logging"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Update config"),
        "Updating config"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Remove dead code"),
        "Removing dead code"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Implement feature"),
        "Implementing feature"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Refactor module"),
        "Refactoring module"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Deploy service"),
        "Deploying service"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Debug crash"),
        "Debugging crash"
    );
}

#[test]
fn test_derive_active_form_case_insensitive() {
    assert_eq!(
        structured_tasks::derive_active_form("fix auth bug"),
        "Fixing auth bug"
    );
    assert_eq!(
        structured_tasks::derive_active_form("add logging"),
        "Adding logging"
    );
}

#[test]
fn test_derive_active_form_unknown_verb() {
    assert_eq!(
        structured_tasks::derive_active_form("Foo bar"),
        "Working on: Foo bar"
    );
}

#[test]
fn test_derive_active_form_empty() {
    assert_eq!(structured_tasks::derive_active_form(""), "Working on task");
    assert_eq!(
        structured_tasks::derive_active_form("  "),
        "Working on task"
    );
}

#[test]
fn test_derive_active_form_verb_only() {
    assert_eq!(structured_tasks::derive_active_form("Fix"), "Fixing");
}

#[test]
fn test_derive_active_form_preserves_subject_case() {
    // Verb is matched case-insensitively but rest of subject preserves case
    assert_eq!(
        structured_tasks::derive_active_form("fix AuthManager Bug"),
        "Fixing AuthManager Bug"
    );
}

// ── Initial dependencies ───────────────────────────────────────

#[tokio::test]
async fn test_initial_blocks_bidirectional() {
    let store = structured_tasks::new_task_store();
    // Pre-create the target task
    {
        let mut s = store.lock().await;
        s.insert(
            "existing".into(),
            structured_tasks::StructuredTask {
                id: "existing".into(),
                subject: "Existing task".into(),
                description: None,
                status: structured_tasks::TaskStatus::Pending,
                active_form: None,
                owner: None,
                blocks: Vec::new(),
                blocked_by: Vec::new(),
                metadata: serde_json::Value::Null,
            },
        );
    }

    let tool = TaskCreateTool::new(store.clone());
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    let input = json!({
        "subject": "New task",
        "blocks": ["existing"]
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();

    // Extract the new task ID from the output
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t.clone(),
        _ => panic!("Expected text"),
    };
    let new_id = text
        .lines()
        .find(|l| l.starts_with("ID: "))
        .unwrap()
        .strip_prefix("ID: ")
        .unwrap();

    let s = store.lock().await;
    // New task should have "existing" in blocks
    assert!(s[new_id].blocks.contains(&"existing".to_string()));
    // Existing task should have new_id in blocked_by
    assert!(s["existing"].blocked_by.contains(&new_id.to_string()));
}

#[tokio::test]
async fn test_initial_blocked_by_bidirectional() {
    let store = structured_tasks::new_task_store();
    {
        let mut s = store.lock().await;
        s.insert(
            "blocker".into(),
            structured_tasks::StructuredTask {
                id: "blocker".into(),
                subject: "Blocker task".into(),
                description: None,
                status: structured_tasks::TaskStatus::Pending,
                active_form: None,
                owner: None,
                blocks: Vec::new(),
                blocked_by: Vec::new(),
                metadata: serde_json::Value::Null,
            },
        );
    }

    let tool = TaskCreateTool::new(store.clone());
    let mut ctx = ToolContext::new("c1", "s1", PathBuf::from("/tmp"));

    let input = json!({
        "subject": "Blocked task",
        "blockedBy": ["blocker"]
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();

    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t.clone(),
        _ => panic!("Expected text"),
    };
    let new_id = text
        .lines()
        .find(|l| l.starts_with("ID: "))
        .unwrap()
        .strip_prefix("ID: ")
        .unwrap();

    let s = store.lock().await;
    assert!(s[new_id].blocked_by.contains(&"blocker".to_string()));
    assert!(s["blocker"].blocks.contains(&new_id.to_string()));
}
