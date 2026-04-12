use super::*;

use coco_tool::AgentHandle;
use coco_tool::AgentSpawnRequest;
use coco_tool::AgentSpawnStatus;
use std::sync::Arc;
use tokio::sync::RwLock;

fn create_test_handle() -> SwarmAgentHandle {
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    let bridge = Arc::new(super::super::swarm_runner::PermissionBridge::new(tx));
    let runner = Arc::new(super::super::swarm_runner::InProcessAgentRunner::new(
        bridge,
        "/tmp".to_string(),
        /*max_agents*/ 8,
    ));
    let backends = Arc::new(super::super::swarm_backend::BackendRegistry::new());
    let team_manager = Arc::new(RwLock::new(None));

    SwarmAgentHandle::new(
        runner,
        backends,
        team_manager,
        "/tmp".to_string(),
        "test-model".to_string(),
    )
}

#[tokio::test]
async fn test_spawn_subagent_sync() {
    let handle = create_test_handle();
    let request = AgentSpawnRequest {
        prompt: "Find files".to_string(),
        description: Some("search".to_string()),
        subagent_type: Some("Explore".to_string()),
        model: None,
        run_in_background: false,
        isolation: None,
        name: None,
        team_name: None,
        mode: None,
        cwd: None,
    };

    let response = handle.spawn_agent(request).await.unwrap();
    // Sync agent spawns and waits — with no execution engine, it completes
    // immediately with no result channel
    assert!(response.agent_id.is_some());
}

#[tokio::test]
async fn test_spawn_subagent_async() {
    let handle = create_test_handle();
    let request = AgentSpawnRequest {
        prompt: "Background work".to_string(),
        description: None,
        subagent_type: None,
        model: None,
        run_in_background: true,
        isolation: None,
        name: None,
        team_name: None,
        mode: None,
        cwd: None,
    };

    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::AsyncLaunched);
    assert!(response.agent_id.is_some());
}

#[tokio::test]
async fn test_spawn_teammate() {
    let handle = create_test_handle();
    let request = AgentSpawnRequest {
        prompt: "Help me".to_string(),
        description: None,
        subagent_type: None,
        model: None,
        run_in_background: false,
        isolation: None,
        name: Some("researcher".to_string()),
        team_name: Some("my-team".to_string()),
        mode: None,
        cwd: None,
    };

    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::TeammateSpawned);
    assert!(response.agent_id.is_some());
    assert!(response.agent_id.unwrap().contains("researcher@my-team"));
}

#[tokio::test]
async fn test_send_message_no_team() {
    let handle = create_test_handle();
    let result = handle.send_message("target", "hello").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("No active team"));
}

#[tokio::test]
async fn test_create_and_delete_team() {
    let handle = create_test_handle();

    // Create
    let result = handle.create_team("alpha").await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("alpha"));

    // Delete
    let result = handle.delete_team("alpha").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_query_unknown_agent() {
    let handle = create_test_handle();
    let result = handle.query_agent_status("nonexistent").await;
    assert!(result.is_err());
}
