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
