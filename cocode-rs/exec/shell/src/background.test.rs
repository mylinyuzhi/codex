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
async fn test_list_tasks() {
    let registry = BackgroundTaskRegistry::new();

    // Empty registry
    assert!(registry.list_tasks().await.is_empty());

    // Register two tasks
    registry
        .register("t1".to_string(), make_process("t1", "echo hello"))
        .await;
    registry
        .register("t2".to_string(), make_process("t2", "sleep 10"))
        .await;

    let tasks = registry.list_tasks().await;
    assert_eq!(tasks.len(), 2);

    // Check both entries are present (order is not guaranteed)
    let ids: Vec<&str> = tasks.iter().map(|(id, _)| id.as_str()).collect();
    assert!(ids.contains(&"t1"));
    assert!(ids.contains(&"t2"));

    // Verify command is included
    let t1 = tasks.iter().find(|(id, _)| id == "t1").expect("t1");
    assert_eq!(t1.1, "echo hello");

    // Stop one task — it should be removed from the list
    registry.stop("t1").await;
    let tasks = registry.list_tasks().await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].0, "t2");
}
