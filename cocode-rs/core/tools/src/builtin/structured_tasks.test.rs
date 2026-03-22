use super::*;
use std::collections::BTreeMap;
use std::collections::HashSet;

// ── can_transition_to ──────────────────────────────────────────

#[test]
fn test_valid_transitions() {
    assert!(TaskStatus::Pending.can_transition_to(TaskStatus::InProgress));
    assert!(TaskStatus::Pending.can_transition_to(TaskStatus::Completed));
    assert!(TaskStatus::InProgress.can_transition_to(TaskStatus::Completed));
}

#[test]
fn test_any_to_deleted() {
    assert!(TaskStatus::Pending.can_transition_to(TaskStatus::Deleted));
    assert!(TaskStatus::InProgress.can_transition_to(TaskStatus::Deleted));
    assert!(TaskStatus::Completed.can_transition_to(TaskStatus::Deleted));
    assert!(TaskStatus::Deleted.can_transition_to(TaskStatus::Deleted));
}

#[test]
fn test_invalid_transitions() {
    assert!(!TaskStatus::Completed.can_transition_to(TaskStatus::Pending));
    assert!(!TaskStatus::Completed.can_transition_to(TaskStatus::InProgress));
    assert!(!TaskStatus::InProgress.can_transition_to(TaskStatus::Pending));
    assert!(!TaskStatus::Deleted.can_transition_to(TaskStatus::Pending));
    assert!(!TaskStatus::Deleted.can_transition_to(TaskStatus::InProgress));
    assert!(!TaskStatus::Deleted.can_transition_to(TaskStatus::Completed));
}

// ── is_internal_task ───────────────────────────────────────────

fn make_task(id: &str, subject: &str, metadata: serde_json::Value) -> StructuredTask {
    StructuredTask {
        id: id.to_string(),
        subject: subject.to_string(),
        description: None,
        status: TaskStatus::Pending,
        active_form: None,
        owner: None,
        blocks: Vec::new(),
        blocked_by: Vec::new(),
        metadata,
    }
}

#[test]
fn test_is_internal_task_true() {
    let task = make_task("t1", "internal", serde_json::json!({"_internal": true}));
    assert!(is_internal_task(&task));
}

#[test]
fn test_is_internal_task_false() {
    let task = make_task("t1", "normal", serde_json::json!({}));
    assert!(!is_internal_task(&task));

    let task2 = make_task("t2", "normal", serde_json::Value::Null);
    assert!(!is_internal_task(&task2));

    let task3 = make_task(
        "t3",
        "not internal",
        serde_json::json!({"_internal": false}),
    );
    assert!(!is_internal_task(&task3));
}

// ── format_single_task ─────────────────────────────────────────

#[test]
fn test_format_single_task_deleted_returns_none() {
    let mut task = make_task("t1", "subject", serde_json::Value::Null);
    task.status = TaskStatus::Deleted;
    assert!(format_single_task(&task, None).is_none());
}

#[test]
fn test_format_single_task_internal_returns_none() {
    let task = make_task("t1", "subject", serde_json::json!({"_internal": true}));
    assert!(format_single_task(&task, None).is_none());
}

#[test]
fn test_format_single_task_pending() {
    let task = make_task("t1", "Fix bug", serde_json::Value::Null);
    let out = format_single_task(&task, None).unwrap();
    assert!(out.starts_with("[ ] t1: Fix bug"));
}

#[test]
fn test_format_single_task_in_progress() {
    let mut task = make_task("t1", "Fix bug", serde_json::Value::Null);
    task.status = TaskStatus::InProgress;
    let out = format_single_task(&task, None).unwrap();
    assert!(out.starts_with("[>] t1: Fix bug"));
}

#[test]
fn test_format_single_task_completed() {
    let mut task = make_task("t1", "Fix bug", serde_json::Value::Null);
    task.status = TaskStatus::Completed;
    let out = format_single_task(&task, None).unwrap();
    assert!(out.starts_with("[x] t1: Fix bug"));
}

#[test]
fn test_format_single_task_blocker_filtering() {
    let mut task = make_task("t1", "Fix bug", serde_json::Value::Null);
    task.blocked_by = vec!["t2".to_string(), "t3".to_string()];

    // Without filtering — show all blockers
    let out = format_single_task(&task, None).unwrap();
    assert!(out.contains("blocked by: t2, t3"));

    // With filtering — t2 is completed, only t3 remains
    let completed: HashSet<String> = ["t2".to_string()].into();
    let out = format_single_task(&task, Some(&completed)).unwrap();
    assert!(out.contains("blocked by: t3"));
    assert!(!out.contains("t2"));

    // All blockers completed — no "blocked by" line
    let completed: HashSet<String> = ["t2".to_string(), "t3".to_string()].into();
    let out = format_single_task(&task, Some(&completed)).unwrap();
    assert!(!out.contains("blocked by"));
}

#[test]
fn test_format_task_summary_filtered_hides_internal() {
    let mut tasks = BTreeMap::new();
    tasks.insert(
        "t1".to_string(),
        make_task("t1", "Visible", serde_json::Value::Null),
    );
    tasks.insert(
        "t2".to_string(),
        make_task("t2", "Hidden", serde_json::json!({"_internal": true})),
    );
    let out = format_task_summary(&tasks);
    assert!(out.contains("t1"));
    assert!(!out.contains("t2"));
}
