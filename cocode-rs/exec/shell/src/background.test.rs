use super::*;

fn make_process(id: &str, command: &str) -> BackgroundProcess {
    BackgroundProcess {
        id: id.to_string(),
        command: command.to_string(),
        output: Arc::new(Mutex::new(String::new())),
        completed: Arc::new(Notify::new()),
        cancel_token: CancellationToken::new(),
    }
}

#[tokio::test]
async fn test_register_and_is_running() {
    let registry = BackgroundTaskRegistry::new();
    let process = make_process("task-1", "sleep 10");

    assert!(!registry.is_running("task-1").await);
    registry.register("task-1".to_string(), process).await;
    assert!(registry.is_running("task-1").await);
}

#[tokio::test]
async fn test_get_output_empty() {
    let registry = BackgroundTaskRegistry::new();
    let process = make_process("task-2", "echo hello");

    registry.register("task-2".to_string(), process).await;
    let output = registry.get_output("task-2").await;
    assert_eq!(output, Some(String::new()));
}

#[tokio::test]
async fn test_get_output_with_data() {
    let registry = BackgroundTaskRegistry::new();
    let process = make_process("task-3", "echo hello");
    let output_ref = Arc::clone(&process.output);

    registry.register("task-3".to_string(), process).await;

    // Simulate writing output
    {
        let mut out = output_ref.lock().await;
        out.push_str("hello world\n");
    }

    let output = registry.get_output("task-3").await;
    assert_eq!(output, Some("hello world\n".to_string()));
}

#[tokio::test]
async fn test_stop_existing_task() {
    let registry = BackgroundTaskRegistry::new();
    let process = make_process("task-4", "sleep 60");

    registry.register("task-4".to_string(), process).await;
    assert!(registry.is_running("task-4").await);

    let stopped = registry.stop("task-4").await;
    assert!(stopped);
    assert!(!registry.is_running("task-4").await);
}

#[tokio::test]
async fn test_stop_nonexistent_task() {
    let registry = BackgroundTaskRegistry::new();
    let stopped = registry.stop("no-such-task").await;
    assert!(!stopped);
}

#[tokio::test]
async fn test_get_output_nonexistent() {
    let registry = BackgroundTaskRegistry::new();
    assert!(registry.get_output("missing").await.is_none());
}

#[tokio::test]
async fn test_default() {
    let registry = BackgroundTaskRegistry::default();
    assert!(!registry.is_running("anything").await);
}

#[tokio::test]
async fn test_get_completed_notify_returns_handle() {
    let registry = BackgroundTaskRegistry::new();
    let process = make_process("task-n", "sleep 1");
    let completed = Arc::clone(&process.completed);

    registry.register("task-n".to_string(), process).await;

    // Should return a notify handle
    let notify = registry.get_completed_notify("task-n").await;
    assert!(notify.is_some());

    // Should be None for non-existent task
    let missing = registry.get_completed_notify("no-such").await;
    assert!(missing.is_none());

    // Notifying via original handle should wake the returned handle.
    // Must register the notified() future BEFORE calling notify_waiters().
    let notify = notify.unwrap();
    let waiter = notify.notified();
    tokio::pin!(waiter);
    // Enable the waiter to be woken
    let _ = tokio::time::timeout(std::time::Duration::from_millis(1), &mut waiter).await;
    completed.notify_waiters();
    tokio::time::timeout(std::time::Duration::from_millis(100), waiter)
        .await
        .expect("notify should have fired");
}

// ── GAP 4: Output preservation after stop ─────────────────────

#[tokio::test]
async fn test_output_preserved_after_stop() {
    let registry = BackgroundTaskRegistry::new();
    let process = make_process("task-persist", "long running cmd");
    let output_ref = Arc::clone(&process.output);

    registry.register("task-persist".to_string(), process).await;

    // Simulate output
    {
        let mut out = output_ref.lock().await;
        out.push_str("line 1\nline 2\n");
    }

    // Stop the task
    assert!(registry.stop("task-persist").await);
    assert!(!registry.is_running("task-persist").await);

    // Output should still be retrievable
    let output = registry.get_output("task-persist").await;
    assert_eq!(output, Some("line 1\nline 2\n".to_string()));
}

#[tokio::test]
async fn test_get_command_active_task() {
    let registry = BackgroundTaskRegistry::new();
    let process = make_process("task-cmd", "npm test");

    registry.register("task-cmd".to_string(), process).await;
    assert_eq!(
        registry.get_command("task-cmd").await,
        Some("npm test".to_string())
    );
}

#[tokio::test]
async fn test_get_command_after_stop() {
    let registry = BackgroundTaskRegistry::new();
    let process = make_process("task-cmd2", "cargo build");

    registry.register("task-cmd2".to_string(), process).await;
    registry.stop("task-cmd2").await;

    assert_eq!(
        registry.get_command("task-cmd2").await,
        Some("cargo build".to_string())
    );
}

#[tokio::test]
async fn test_get_command_nonexistent() {
    let registry = BackgroundTaskRegistry::new();
    assert!(registry.get_command("missing").await.is_none());
}

// ── list_tasks ─────────────────────────────────────────

#[tokio::test]
async fn test_list_tasks_empty() {
    let registry = BackgroundTaskRegistry::new();
    assert!(registry.list_tasks().await.is_empty());
}

#[tokio::test]
async fn test_list_tasks_with_active_and_completed() {
    let registry = BackgroundTaskRegistry::new();

    // Register two tasks
    registry
        .register("t1".to_string(), make_process("t1", "npm test"))
        .await;
    registry
        .register("t2".to_string(), make_process("t2", "cargo build"))
        .await;

    // Stop one
    registry.stop("t2").await;

    let snapshots = registry.list_tasks().await;
    assert_eq!(snapshots.len(), 2);

    let active: Vec<_> = snapshots.iter().filter(|s| s.is_running).collect();
    let completed: Vec<_> = snapshots.iter().filter(|s| !s.is_running).collect();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "t1");
    assert_eq!(active[0].command, "npm test");
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].id, "t2");
    assert_eq!(completed[0].command, "cargo build");
}
