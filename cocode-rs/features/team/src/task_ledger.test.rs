use super::*;

fn temp_ledger() -> TaskLedger {
    TaskLedger::new(
        std::path::PathBuf::from("/tmp/test-ledger"),
        /*persist=*/ false,
    )
}

#[tokio::test]
async fn test_create_and_list_tasks() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Fix bug", "Fix the login bug", vec![])
        .await
        .unwrap();
    assert_eq!(t1.subject, "Fix bug");
    assert_eq!(t1.status, TeamTaskStatus::Pending);
    assert!(t1.owner.is_none());

    let tasks = ledger.list_tasks("team1").await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, t1.id);
}

#[tokio::test]
async fn test_claim_task_success() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Task A", "", vec![])
        .await
        .unwrap();

    match ledger.claim_task("team1", &t1.id, "agent-1").await.unwrap() {
        ClaimResult::Claimed(task) => {
            assert_eq!(task.owner.as_deref(), Some("agent-1"));
            assert_eq!(task.status, TeamTaskStatus::InProgress);
        }
        other => panic!("Expected Claimed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_claim_already_claimed() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Task A", "", vec![])
        .await
        .unwrap();
    ledger.claim_task("team1", &t1.id, "agent-1").await.unwrap();

    match ledger.claim_task("team1", &t1.id, "agent-2").await.unwrap() {
        ClaimResult::AlreadyClaimed { by } => assert_eq!(by, "agent-1"),
        other => panic!("Expected AlreadyClaimed, got {other:?}"),
    }
}

#[tokio::test]
async fn test_claim_blocked_task() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Blocker", "", vec![])
        .await
        .unwrap();
    let t2 = ledger
        .create_task("team1", "Blocked", "", vec![t1.id.clone()])
        .await
        .unwrap();

    match ledger.claim_task("team1", &t2.id, "agent-1").await.unwrap() {
        ClaimResult::Blocked { blocked_by } => {
            assert_eq!(blocked_by, vec![t1.id.clone()]);
        }
        other => panic!("Expected Blocked, got {other:?}"),
    }
}

#[tokio::test]
async fn test_complete_task_unblocks_dependents() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Blocker", "", vec![])
        .await
        .unwrap();
    let t2 = ledger
        .create_task("team1", "Blocked", "", vec![t1.id.clone()])
        .await
        .unwrap();

    // Claim and complete blocker.
    ledger.claim_task("team1", &t1.id, "agent-1").await.unwrap();
    ledger.complete_task("team1", &t1.id).await.unwrap();

    // Now t2 should be claimable.
    match ledger.claim_task("team1", &t2.id, "agent-2").await.unwrap() {
        ClaimResult::Claimed(task) => {
            assert_eq!(task.owner.as_deref(), Some("agent-2"));
            assert!(task.blocked_by.is_empty());
        }
        other => panic!("Expected Claimed after unblock, got {other:?}"),
    }
}

#[tokio::test]
async fn test_next_claimable_skips_blocked() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Blocker", "", vec![])
        .await
        .unwrap();
    let _t2 = ledger
        .create_task("team1", "Blocked", "", vec![t1.id.clone()])
        .await
        .unwrap();
    let t3 = ledger
        .create_task("team1", "Free task", "", vec![])
        .await
        .unwrap();

    // Claim t1 so it's InProgress (not available), t2 is blocked, t3 is free.
    ledger.claim_task("team1", &t1.id, "agent-1").await.unwrap();

    let next = ledger.next_claimable("team1").await.unwrap();
    assert_eq!(next.id, t3.id);
}

#[tokio::test]
async fn test_reassign_agent_tasks() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Task A", "", vec![])
        .await
        .unwrap();
    let t2 = ledger
        .create_task("team1", "Task B", "", vec![])
        .await
        .unwrap();

    ledger.claim_task("team1", &t1.id, "agent-1").await.unwrap();
    ledger.claim_task("team1", &t2.id, "agent-1").await.unwrap();

    let unassigned = ledger
        .reassign_agent_tasks("team1", "agent-1")
        .await
        .unwrap();
    assert_eq!(unassigned.len(), 2);

    // Both should be pending again.
    let tasks = ledger.list_tasks("team1").await;
    for task in &tasks {
        assert_eq!(task.status, TeamTaskStatus::Pending);
        assert!(task.owner.is_none());
    }
}

#[tokio::test]
async fn test_add_dependency() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "First", "", vec![])
        .await
        .unwrap();
    let t2 = ledger
        .create_task("team1", "Second", "", vec![])
        .await
        .unwrap();

    ledger
        .add_dependency("team1", &t1.id, &t2.id)
        .await
        .unwrap();

    let t1_updated = ledger.get_task("team1", &t1.id).await.unwrap();
    let t2_updated = ledger.get_task("team1", &t2.id).await.unwrap();

    assert!(t1_updated.blocks.contains(&t2.id));
    assert!(t2_updated.blocked_by.contains(&t1.id));
}

#[tokio::test]
async fn test_delete_task_cleans_references() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Blocker", "", vec![])
        .await
        .unwrap();
    let t2 = ledger
        .create_task("team1", "Blocked", "", vec![t1.id.clone()])
        .await
        .unwrap();

    ledger.delete_task("team1", &t1.id).await.unwrap();

    let t2_updated = ledger.get_task("team1", &t2.id).await.unwrap();
    assert!(
        t2_updated.blocked_by.is_empty(),
        "blocked_by should be cleaned"
    );
}

#[tokio::test]
async fn test_claim_not_found() {
    let ledger = temp_ledger();
    match ledger
        .claim_task("team1", "nonexistent", "agent-1")
        .await
        .unwrap()
    {
        ClaimResult::NotFound => {}
        other => panic!("Expected NotFound, got {other:?}"),
    }
}

#[tokio::test]
async fn test_sequential_ids() {
    let ledger = temp_ledger();
    let t1 = ledger.create_task("team1", "A", "", vec![]).await.unwrap();
    let t2 = ledger.create_task("team1", "B", "", vec![]).await.unwrap();
    let t3 = ledger.create_task("team1", "C", "", vec![]).await.unwrap();

    assert_eq!(t1.id, "1");
    assert_eq!(t2.id, "2");
    assert_eq!(t3.id, "3");
}

#[tokio::test]
async fn test_claim_completed_task_rejected() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Task A", "", vec![])
        .await
        .unwrap();
    ledger.claim_task("team1", &t1.id, "agent-1").await.unwrap();
    ledger.complete_task("team1", &t1.id).await.unwrap();

    // Completed task must not be claimable.
    match ledger.claim_task("team1", &t1.id, "agent-2").await.unwrap() {
        ClaimResult::AlreadyClaimed { .. } => {}
        other => panic!("Expected AlreadyClaimed for completed task, got {other:?}"),
    }
}

#[tokio::test]
async fn test_next_claimable_returns_none_when_all_blocked() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Blocker", "", vec![])
        .await
        .unwrap();
    let _t2 = ledger
        .create_task("team1", "Blocked", "", vec![t1.id.clone()])
        .await
        .unwrap();

    // t1 is claimed (InProgress), t2 is blocked by t1. No claimable tasks.
    ledger.claim_task("team1", &t1.id, "agent-1").await.unwrap();

    let next = ledger.next_claimable("team1").await;
    assert!(
        next.is_none(),
        "Should return None when all pending tasks are blocked"
    );
}

#[tokio::test]
async fn test_add_dependency_nonexistent_task_errors() {
    let ledger = temp_ledger();
    let t1 = ledger
        .create_task("team1", "Task A", "", vec![])
        .await
        .unwrap();

    // blocker doesn't exist
    let err = ledger.add_dependency("team1", "nonexistent", &t1.id).await;
    assert!(err.is_err());

    // blocked doesn't exist
    let err = ledger.add_dependency("team1", &t1.id, "nonexistent").await;
    assert!(err.is_err());
}

#[tokio::test]
async fn test_create_with_invalid_blockers_silently_removed() {
    let ledger = temp_ledger();
    // Reference a non-existent task as blocker.
    let t1 = ledger
        .create_task("team1", "Task", "", vec!["nonexistent".to_string()])
        .await
        .unwrap();

    assert!(
        t1.blocked_by.is_empty(),
        "Invalid blocker references should be removed"
    );
}
