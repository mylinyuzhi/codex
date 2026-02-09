use super::*;

#[test]
fn test_new_coordinator() {
    let coord = AgentCoordinator::new();
    assert_eq!(coord.agent_count(), 0);
}

#[tokio::test]
async fn test_spawn_agent() {
    let mut coord = AgentCoordinator::new();
    let config = SpawnConfig {
        model: "claude-3".to_string(),
        prompt: "test".to_string(),
        tools: vec!["Bash".to_string()],
    };
    let id = coord.spawn_agent(config).await.expect("spawn");
    assert!(!id.is_empty());
    assert_eq!(coord.agent_count(), 1);
    assert_eq!(coord.get_status(&id), Some(&AgentLifecycleStatus::Running));
}

#[tokio::test]
async fn test_send_input_to_running() {
    let mut coord = AgentCoordinator::new();
    let config = SpawnConfig {
        model: "claude-3".to_string(),
        prompt: "test".to_string(),
        tools: vec![],
    };
    let id = coord.spawn_agent(config).await.expect("spawn");
    let result = coord.send_input(&id, "hello").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_send_input_to_completed() {
    let mut coord = AgentCoordinator::new();
    let config = SpawnConfig {
        model: "claude-3".to_string(),
        prompt: "test".to_string(),
        tools: vec![],
    };
    let id = coord.spawn_agent(config).await.expect("spawn");
    coord.close_agent(&id).await.expect("close");
    let result = coord.send_input(&id, "hello").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_send_input_nonexistent() {
    let coord = AgentCoordinator::new();
    let result = coord.send_input("nonexistent", "hello").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_close_agent() {
    let mut coord = AgentCoordinator::new();
    let config = SpawnConfig {
        model: "claude-3".to_string(),
        prompt: "test".to_string(),
        tools: vec![],
    };
    let id = coord.spawn_agent(config).await.expect("spawn");
    coord.close_agent(&id).await.expect("close");
    assert_eq!(
        coord.get_status(&id),
        Some(&AgentLifecycleStatus::Completed)
    );
}

#[tokio::test]
async fn test_close_nonexistent() {
    let mut coord = AgentCoordinator::new();
    let result = coord.close_agent("nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_wait_for_no_callback() {
    // Without an execute_fn, wait_for returns immediately with empty output
    let mut coord = AgentCoordinator::new();
    let config = SpawnConfig {
        model: "claude-3".to_string(),
        prompt: "test".to_string(),
        tools: vec![],
    };
    let id = coord.spawn_agent(config).await.expect("spawn");
    // Close the agent first since there's no callback to produce output
    coord.close_agent(&id).await.expect("close");
    let output = coord.wait_for(&id).await.expect("wait");
    assert!(output.is_empty());
}

#[tokio::test]
async fn test_spawn_with_execute_fn() {
    // Create a coordinator with an execution callback
    let mut coord =
        AgentCoordinator::new().with_execute_fn(Arc::new(|_model, prompt, _tools| {
            Box::pin(async move { Ok(format!("Executed: {prompt}")) })
        }));

    let config = SpawnConfig {
        model: "claude-3".to_string(),
        prompt: "test task".to_string(),
        tools: vec![],
    };
    let id = coord.spawn_agent(config).await.expect("spawn");

    // Wait for completion
    let output = coord.wait_for(&id).await.expect("wait");
    assert!(output.contains("Executed: test task"));
    assert_eq!(
        coord.get_status(&id),
        Some(&AgentLifecycleStatus::Completed)
    );
}

#[test]
fn test_get_status_missing() {
    let coord = AgentCoordinator::new();
    assert!(coord.get_status("nonexistent").is_none());
}
