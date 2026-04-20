use super::*;
use tempfile::TempDir;

fn fresh_store() -> (TempDir, Arc<TaskListStore>) {
    let dir = TempDir::new().unwrap();
    let store = TaskListStore::open(dir.path(), "test-list").unwrap();
    (dir, store)
}

#[tokio::test]
async fn test_resolve_task_list_id_precedence() {
    // Env var wins.
    // SAFETY: set_var/remove_var are unsafe in Rust 2024; this test is
    // single-threaded and the var is restored at the end.
    unsafe {
        std::env::set_var(EnvKey::CocoTaskListId, "from-env");
    }
    assert_eq!(
        resolve_task_list_id(Some("teammate"), Some("leader"), "session"),
        "from-env"
    );
    unsafe {
        std::env::remove_var(EnvKey::CocoTaskListId);
    }

    // Teammate > leader > session.
    assert_eq!(
        resolve_task_list_id(Some("teammate"), Some("leader"), "session"),
        "teammate"
    );
    assert_eq!(
        resolve_task_list_id(None, Some("leader"), "session"),
        "leader"
    );
    assert_eq!(resolve_task_list_id(None, None, "session"), "session");
}

#[tokio::test]
async fn test_sanitize_path_component_strips_unsafe() {
    assert_eq!(sanitize_path_component("hello/world"), "hello-world");
    assert_eq!(sanitize_path_component("../evil"), "---evil");
    assert_eq!(sanitize_path_component("ok_name-123"), "ok_name-123");
}

#[tokio::test]
async fn test_create_then_get() {
    let (_dir, store) = fresh_store();
    let task = store
        .create_task(
            "run tests".into(),
            "cargo test".into(),
            Some("Running tests".into()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(task.id, "1");
    assert_eq!(task.status, TaskStatus::Pending);

    let got = store.get_task("1").await.unwrap().unwrap();
    assert_eq!(got.subject, "run tests");
    assert_eq!(got.active_form.as_deref(), Some("Running tests"));
}

#[tokio::test]
async fn test_sequential_ids() {
    let (_dir, store) = fresh_store();
    for i in 1..=5 {
        let t = store
            .create_task(format!("t{i}"), "d".into(), None, None)
            .await
            .unwrap();
        assert_eq!(t.id, i.to_string());
    }
}

#[tokio::test]
async fn test_update_status_transitions() {
    let (_dir, store) = fresh_store();
    let t = store
        .create_task("x".into(), "y".into(), None, None)
        .await
        .unwrap();

    let updated = store
        .update_task(
            &t.id,
            TaskUpdate {
                status: Some(TaskStatus::InProgress),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, TaskStatus::InProgress);

    let completed = store
        .update_task(
            &t.id,
            TaskUpdate {
                status: Some(TaskStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_delete_cascades_blocks() {
    let (_dir, store) = fresh_store();
    let a = store
        .create_task("a".into(), "".into(), None, None)
        .await
        .unwrap();
    let b = store
        .create_task("b".into(), "".into(), None, None)
        .await
        .unwrap();
    let c = store
        .create_task("c".into(), "".into(), None, None)
        .await
        .unwrap();

    store.block_task(&a.id, &b.id).await.unwrap();
    store.block_task(&a.id, &c.id).await.unwrap();

    // Delete `a` — `b` and `c` should lose their blockedBy entries.
    assert!(store.delete_task(&a.id).await.unwrap());
    let b = store.get_task(&b.id).await.unwrap().unwrap();
    let c = store.get_task(&c.id).await.unwrap().unwrap();
    assert!(b.blocked_by.is_empty());
    assert!(c.blocked_by.is_empty());
}

#[tokio::test]
async fn test_delete_updates_hwm() {
    let (_dir, store) = fresh_store();
    let t = store
        .create_task("x".into(), "".into(), None, None)
        .await
        .unwrap();
    assert_eq!(t.id, "1");
    store.delete_task(&t.id).await.unwrap();

    // Next create should produce id=2, not id=1, because HWM tracks deleted ids.
    let t2 = store
        .create_task("y".into(), "".into(), None, None)
        .await
        .unwrap();
    assert_eq!(t2.id, "2");
}

#[tokio::test]
async fn test_claim_success() {
    let (_dir, store) = fresh_store();
    let t = store
        .create_task("x".into(), "".into(), None, None)
        .await
        .unwrap();
    match store.claim_task(&t.id, "alice", false).await.unwrap() {
        ClaimResult::Success(task) => assert_eq!(task.owner.as_deref(), Some("alice")),
        other => panic!("expected Success, got {other:?}"),
    }
}

#[tokio::test]
async fn test_claim_already_claimed() {
    let (_dir, store) = fresh_store();
    let t = store
        .create_task("x".into(), "".into(), None, None)
        .await
        .unwrap();
    store.claim_task(&t.id, "alice", false).await.unwrap();
    match store.claim_task(&t.id, "bob", false).await.unwrap() {
        ClaimResult::AlreadyClaimed(task) => assert_eq!(task.owner.as_deref(), Some("alice")),
        other => panic!("expected AlreadyClaimed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_claim_blocked() {
    let (_dir, store) = fresh_store();
    let a = store
        .create_task("a".into(), "".into(), None, None)
        .await
        .unwrap();
    let b = store
        .create_task("b".into(), "".into(), None, None)
        .await
        .unwrap();
    store.block_task(&a.id, &b.id).await.unwrap();
    match store.claim_task(&b.id, "bob", false).await.unwrap() {
        ClaimResult::Blocked {
            blocked_by_tasks, ..
        } => {
            assert_eq!(blocked_by_tasks, vec![a.id.clone()]);
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
}

#[tokio::test]
async fn test_claim_agent_busy() {
    let (_dir, store) = fresh_store();
    let a = store
        .create_task("a".into(), "".into(), None, None)
        .await
        .unwrap();
    let b = store
        .create_task("b".into(), "".into(), None, None)
        .await
        .unwrap();
    // alice claims a.
    store.claim_task(&a.id, "alice", false).await.unwrap();
    // alice tries to claim b with busy check -> rejected.
    match store.claim_task(&b.id, "alice", true).await.unwrap() {
        ClaimResult::AgentBusy {
            busy_with_tasks, ..
        } => {
            assert_eq!(busy_with_tasks, vec![a.id.clone()]);
        }
        other => panic!("expected AgentBusy, got {other:?}"),
    }
}

#[tokio::test]
async fn test_metadata_merge_with_null_deletion() {
    let (_dir, store) = fresh_store();
    let mut initial = HashMap::new();
    initial.insert("a".into(), serde_json::json!(1));
    initial.insert("b".into(), serde_json::json!(2));
    let t = store
        .create_task("x".into(), "".into(), None, Some(initial))
        .await
        .unwrap();

    let mut merge = HashMap::new();
    merge.insert("a".into(), serde_json::Value::Null); // delete
    merge.insert("c".into(), serde_json::json!(3)); // add
    store
        .update_task(
            &t.id,
            TaskUpdate {
                metadata_merge: Some(merge),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let got = store.get_task(&t.id).await.unwrap().unwrap();
    let meta = got.metadata.unwrap();
    assert!(!meta.contains_key("a"));
    assert_eq!(meta.get("b").unwrap(), &serde_json::json!(2));
    assert_eq!(meta.get("c").unwrap(), &serde_json::json!(3));
}

#[tokio::test]
async fn test_unassign_teammate_tasks() {
    let (_dir, store) = fresh_store();
    let a = store
        .create_task("a".into(), "".into(), None, None)
        .await
        .unwrap();
    let b = store
        .create_task("b".into(), "".into(), None, None)
        .await
        .unwrap();
    let c = store
        .create_task("c".into(), "".into(), None, None)
        .await
        .unwrap();
    store.claim_task(&a.id, "alice", false).await.unwrap();
    store.claim_task(&b.id, "alice", false).await.unwrap();
    store.claim_task(&c.id, "bob", false).await.unwrap();

    let unassigned = store
        .unassign_teammate_tasks("alice", "alice")
        .await
        .unwrap();
    assert_eq!(unassigned.len(), 2);

    // a and b should now be unowned and pending.
    let a = store.get_task(&a.id).await.unwrap().unwrap();
    assert!(a.owner.is_none());
    assert_eq!(a.status, TaskStatus::Pending);
    // c is unchanged.
    let c = store.get_task(&c.id).await.unwrap().unwrap();
    assert_eq!(c.owner.as_deref(), Some("bob"));
}
