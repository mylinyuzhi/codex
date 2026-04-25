use super::*;

use coco_tool_runtime::AgentHandle;
use coco_tool_runtime::AgentSpawnRequest;
use coco_tool_runtime::AgentSpawnStatus;
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
    let team_manager = Arc::new(RwLock::new(None));

    SwarmAgentHandle::new(
        runner,
        team_manager,
        "/tmp".to_string(),
        "test-model".to_string(),
    )
}

#[tokio::test]
async fn test_spawn_subagent_sync_without_engine_fails_cleanly() {
    // Phase 6 Workstream C hardening: a sync subagent spawn without
    // an installed AgentQueryEngine must surface a clean failure
    // (not a silent "completed with placeholder" outcome). The old
    // register-but-never-start pattern silently succeeded with
    // "Agent completed (no result channel)" — that's a silent-bug
    // anti-pattern.
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
    assert_eq!(response.status, AgentSpawnStatus::Failed);
    assert!(response.agent_id.is_some());
    assert!(
        response
            .error
            .as_deref()
            .unwrap_or("")
            .contains("No AgentQueryEngine"),
        "must surface the missing-engine error clearly; got: {:?}",
        response.error
    );
}

#[tokio::test]
async fn test_spawn_subagent_sync_with_engine_routes_to_query() {
    // Positive path: with an AgentQueryEngine installed, the subagent
    // flow invokes execute_query and returns the child's result.
    use async_trait::async_trait;
    use coco_tool_runtime::AgentQueryConfig;
    use coco_tool_runtime::AgentQueryEngine;
    use coco_tool_runtime::AgentQueryResult;

    struct StubEngine;
    #[async_trait]
    impl AgentQueryEngine for StubEngine {
        async fn execute_query(
            &self,
            _prompt: &str,
            _config: AgentQueryConfig,
        ) -> anyhow::Result<AgentQueryResult> {
            Ok(AgentQueryResult {
                response_text: Some("child result".into()),
                messages: Vec::new(),
                turns: 2,
                input_tokens: 100,
                output_tokens: 50,
                tool_use_count: 3,
                cancelled: false,
            })
        }
    }

    let mut handle = create_test_handle();
    handle.set_execution_engine(Arc::new(StubEngine));

    let request = AgentSpawnRequest {
        prompt: "do work".into(),
        description: None,
        subagent_type: Some("Explore".into()),
        model: None,
        run_in_background: false,
        isolation: None,
        name: None,
        team_name: None,
        mode: None,
        cwd: None,
    };
    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::Completed);
    assert_eq!(response.result.as_deref(), Some("child result"));
    assert_eq!(response.total_tool_use_count, 3);
    assert_eq!(response.total_tokens, 150);
}

#[tokio::test]
async fn test_spawn_subagent_worktree_without_manager_fails_cleanly() {
    // `isolation: "worktree"` with no worktree manager must fail
    // with a descriptive error — not silently run without
    // isolation.
    let handle = create_test_handle();
    let request = AgentSpawnRequest {
        prompt: "isolated work".into(),
        description: None,
        subagent_type: None,
        model: None,
        run_in_background: false,
        isolation: Some("worktree".into()),
        name: None,
        team_name: None,
        mode: None,
        cwd: None,
    };
    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::Failed);
    assert!(
        response
            .error
            .as_deref()
            .unwrap_or("")
            .contains("AgentWorktreeManager"),
        "must explain the missing manager; got: {:?}",
        response.error
    );
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
