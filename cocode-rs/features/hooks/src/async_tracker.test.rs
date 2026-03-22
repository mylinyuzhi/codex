use super::*;

#[test]
fn test_register_and_complete() {
    let tracker = AsyncHookTracker::new();

    tracker.register("task-1".to_string(), "test-hook".to_string());
    assert_eq!(tracker.pending_count(), 1);
    assert_eq!(tracker.completed_count(), 0);

    tracker.complete("task-1", HookResult::Continue);
    assert_eq!(tracker.pending_count(), 0);
    assert_eq!(tracker.completed_count(), 1);

    let completed = tracker.take_completed();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].task_id, "task-1");
    assert_eq!(completed[0].hook_name, "test-hook");
    assert!(!completed[0].was_blocking);

    assert_eq!(tracker.completed_count(), 0);
}

#[test]
fn test_register_with_custom_timeout() {
    let tracker = AsyncHookTracker::new();

    tracker.register_with_timeout("task-1".to_string(), "hook".to_string(), 60);
    assert_eq!(tracker.pending_count(), 1);

    // Should not be expired immediately
    let expired = tracker.check_expired();
    assert!(expired.is_empty());
}

#[test]
fn test_complete_with_reject() {
    let tracker = AsyncHookTracker::new();

    tracker.register("task-1".to_string(), "security-hook".to_string());
    tracker.complete(
        "task-1",
        HookResult::Reject {
            reason: "Not allowed".to_string(),
        },
    );

    let completed = tracker.take_completed();
    assert_eq!(completed.len(), 1);
    assert!(completed[0].was_blocking);
    assert_eq!(
        completed[0].blocking_reason,
        Some("Not allowed".to_string())
    );
}

#[test]
fn test_complete_with_context() {
    let tracker = AsyncHookTracker::new();

    tracker.register("task-1".to_string(), "context-hook".to_string());
    tracker.complete(
        "task-1",
        HookResult::ContinueWithContext {
            additional_context: Some("Extra info".to_string()),
            env_vars: std::collections::HashMap::new(),
        },
    );

    let completed = tracker.take_completed();
    assert_eq!(completed.len(), 1);
    assert!(!completed[0].was_blocking);
    assert_eq!(
        completed[0].additional_context,
        Some("Extra info".to_string())
    );
}

#[test]
fn test_complete_unknown_task() {
    let tracker = AsyncHookTracker::new();
    // Should not panic or add to completed
    tracker.complete("unknown-task", HookResult::Continue);
    assert_eq!(tracker.completed_count(), 0);
}

#[test]
fn test_cancel() {
    let tracker = AsyncHookTracker::new();

    tracker.register("task-1".to_string(), "hook".to_string());
    assert_eq!(tracker.pending_count(), 1);

    let cancelled = tracker.cancel("task-1");
    assert!(cancelled);
    assert_eq!(tracker.pending_count(), 0);
    assert_eq!(tracker.completed_count(), 0);
}

#[test]
fn test_cancel_unknown() {
    let tracker = AsyncHookTracker::new();
    let cancelled = tracker.cancel("unknown");
    assert!(!cancelled);
}

#[test]
fn test_cancel_all() {
    let tracker = AsyncHookTracker::new();

    tracker.register("task-1".to_string(), "hook-1".to_string());
    tracker.register("task-2".to_string(), "hook-2".to_string());
    tracker.register("task-3".to_string(), "hook-3".to_string());

    assert_eq!(tracker.pending_count(), 3);

    let cancelled = tracker.cancel_all();
    assert_eq!(cancelled, 3);
    assert_eq!(tracker.pending_count(), 0);
}

#[test]
fn test_cancel_all_empty() {
    let tracker = AsyncHookTracker::new();
    let cancelled = tracker.cancel_all();
    assert_eq!(cancelled, 0);
}

#[test]
fn test_check_expired_with_zero_timeout() {
    let tracker = AsyncHookTracker::new();

    // Register with zero timeout — should be expired once any time passes
    tracker.register_with_timeout("task-1".to_string(), "hook".to_string(), 0);

    // Sleep long enough that elapsed().as_secs() >= 0 (which is always true)
    // The check is `>=`, so a 0-timeout hook is expired immediately.
    let expired = tracker.check_expired();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0], "task-1");
}

#[test]
fn test_is_empty() {
    let tracker = AsyncHookTracker::new();
    assert!(tracker.is_empty());

    tracker.register("task-1".to_string(), "hook".to_string());
    assert!(!tracker.is_empty());

    tracker.complete("task-1", HookResult::Continue);
    assert!(!tracker.is_empty()); // Has completed

    tracker.take_completed();
    assert!(tracker.is_empty());
}

#[test]
fn test_multiple_hooks() {
    let tracker = AsyncHookTracker::new();

    tracker.register("task-1".to_string(), "hook-1".to_string());
    tracker.register("task-2".to_string(), "hook-2".to_string());
    tracker.register("task-3".to_string(), "hook-3".to_string());

    assert_eq!(tracker.pending_count(), 3);

    tracker.complete("task-2", HookResult::Continue);
    tracker.complete("task-1", HookResult::Continue);

    assert_eq!(tracker.pending_count(), 1);
    assert_eq!(tracker.completed_count(), 2);

    let completed = tracker.take_completed();
    assert_eq!(completed.len(), 2);
}
