use std::sync::Arc;

use coco_tool_runtime::AgentHandle;
use coco_tool_runtime::AgentSpawnRequest;
use coco_tool_runtime::AgentSpawnResponse;
use coco_tool_runtime::AgentSpawnStatus;
use coco_tool_runtime::ToolUseContext;
use pretty_assertions::assert_eq;

use super::*;

// ── Mock AgentHandle ──

struct MockAgentHandle {
    spawn_result: tokio::sync::Mutex<Option<Result<AgentSpawnResponse, String>>>,
    send_result: tokio::sync::Mutex<Option<Result<String, String>>>,
    team_create_result: tokio::sync::Mutex<Option<Result<String, String>>>,
    team_delete_result: tokio::sync::Mutex<Option<Result<String, String>>>,
}

impl MockAgentHandle {
    fn with_spawn(result: Result<AgentSpawnResponse, String>) -> Self {
        Self {
            spawn_result: tokio::sync::Mutex::new(Some(result)),
            send_result: tokio::sync::Mutex::new(None),
            team_create_result: tokio::sync::Mutex::new(None),
            team_delete_result: tokio::sync::Mutex::new(None),
        }
    }

    fn with_send(result: Result<String, String>) -> Self {
        Self {
            spawn_result: tokio::sync::Mutex::new(None),
            send_result: tokio::sync::Mutex::new(Some(result)),
            team_create_result: tokio::sync::Mutex::new(None),
            team_delete_result: tokio::sync::Mutex::new(None),
        }
    }

    fn with_team_create(result: Result<String, String>) -> Self {
        Self {
            spawn_result: tokio::sync::Mutex::new(None),
            send_result: tokio::sync::Mutex::new(None),
            team_create_result: tokio::sync::Mutex::new(Some(result)),
            team_delete_result: tokio::sync::Mutex::new(None),
        }
    }

    fn with_team_delete(result: Result<String, String>) -> Self {
        Self {
            spawn_result: tokio::sync::Mutex::new(None),
            send_result: tokio::sync::Mutex::new(None),
            team_create_result: tokio::sync::Mutex::new(None),
            team_delete_result: tokio::sync::Mutex::new(Some(result)),
        }
    }
}

#[async_trait::async_trait]
impl AgentHandle for MockAgentHandle {
    async fn spawn_agent(&self, _req: AgentSpawnRequest) -> Result<AgentSpawnResponse, String> {
        self.spawn_result
            .lock()
            .await
            .take()
            .unwrap_or(Err("no mock result".into()))
    }

    async fn send_message(&self, _to: &str, _content: &str) -> Result<String, String> {
        self.send_result
            .lock()
            .await
            .take()
            .unwrap_or(Err("no mock result".into()))
    }

    async fn create_team(&self, _name: &str) -> Result<String, String> {
        self.team_create_result
            .lock()
            .await
            .take()
            .unwrap_or(Err("no mock result".into()))
    }

    async fn delete_team(&self, _name: &str) -> Result<String, String> {
        self.team_delete_result
            .lock()
            .await
            .take()
            .unwrap_or(Err("no mock result".into()))
    }

    async fn resume_agent(
        &self,
        _agent_id: &str,
        _prompt: Option<&str>,
    ) -> Result<AgentSpawnResponse, String> {
        Err("not implemented in mock".into())
    }

    async fn query_agent_status(&self, _agent_id: &str) -> Result<AgentSpawnResponse, String> {
        Err("not implemented in mock".into())
    }

    async fn get_agent_output(&self, _agent_id: &str) -> Result<String, String> {
        Err("not implemented in mock".into())
    }

    async fn background_agent(&self, _agent_id: &str) -> Result<(), String> {
        Err("not implemented in mock".into())
    }
}

fn ctx_with_agent(handle: impl AgentHandle + 'static) -> ToolUseContext {
    let mut ctx = ToolUseContext::test_default();
    ctx.agent = Arc::new(handle);
    ctx
}

// ── AgentTool tests ──

#[tokio::test]
async fn test_agent_tool_empty_prompt_rejected() {
    let ctx = ToolUseContext::test_default();
    let result = AgentTool
        .execute(serde_json::json!({"prompt": ""}), &ctx)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_agent_tool_missing_prompt_rejected() {
    let ctx = ToolUseContext::test_default();
    let result = AgentTool
        .execute(serde_json::json!({"description": "test"}), &ctx)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_agent_tool_rejects_remote_isolation_cleanly() {
    // Phase 6 Workstream C: `isolation: "remote"` must produce a
    // clean model-visible error rather than silently falling back
    // to sync mode (refactor plan's "Make Unsupported Parity
    // Explicit" rule).
    let ctx = ToolUseContext::test_default();
    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "do the thing",
                "isolation": "remote",
            }),
            &ctx,
        )
        .await;
    let err = result.expect_err("remote isolation must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("remote") && msg.to_lowercase().contains("not supported"),
        "error must explain remote isolation is not supported; got: {msg}"
    );
}

#[tokio::test]
async fn test_agent_tool_accepts_worktree_isolation_input_shape() {
    // `isolation: "worktree"` must NOT be rejected by the tool's
    // early gate — it falls through to the AgentHandle, which is
    // responsible for the actual worktree lifecycle. This test
    // proves the gate is remote-only, not worktree-blocking.
    let ctx = ToolUseContext::test_default();
    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "isolated task",
                "isolation": "worktree",
            }),
            &ctx,
        )
        .await;
    // NoOpAgentHandle returns Err for spawn_agent, so we expect an
    // error — but the error message must NOT be the
    // remote-unsupported one.
    let err = result.expect_err("NoOp handle returns error");
    let msg = format!("{err}");
    assert!(
        !msg.to_lowercase().contains("remote"),
        "worktree input path must not hit the remote gate; got: {msg}"
    );
}

#[tokio::test]
async fn test_agent_tool_completed_sync() {
    let response = AgentSpawnResponse {
        status: AgentSpawnStatus::Completed,
        agent_id: None,
        result: Some("Found 3 files.".into()),
        error: None,
        total_tool_use_count: 5,
        total_tokens: 1000,
        duration_ms: 2000,
        worktree_path: None,
        worktree_branch: None,
        output_file: None,
        prompt: None,
    };
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Ok(response)));
    let result = AgentTool
        .execute(serde_json::json!({"prompt": "Find files"}), &ctx)
        .await
        .unwrap();

    assert_eq!(result.data["status"], "completed");
    assert_eq!(result.data["content"], "Found 3 files.");
    assert_eq!(result.data["totalToolUseCount"], 5);
}

#[tokio::test]
async fn test_agent_tool_async_launched() {
    let response = AgentSpawnResponse {
        status: AgentSpawnStatus::AsyncLaunched,
        agent_id: Some("agent-abc".into()),
        result: None,
        error: None,
        total_tool_use_count: 0,
        total_tokens: 0,
        duration_ms: 0,
        worktree_path: None,
        worktree_branch: None,
        output_file: None,
        prompt: None,
    };
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Ok(response)));
    let result = AgentTool
        .execute(
            serde_json::json!({"prompt": "Background task", "run_in_background": true}),
            &ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.data["status"], "async_launched");
    assert_eq!(result.data["agentId"], "agent-abc");
}

#[tokio::test]
async fn test_agent_tool_teammate_spawned() {
    let response = AgentSpawnResponse {
        status: AgentSpawnStatus::TeammateSpawned,
        agent_id: Some("teammate-1".into()),
        result: None,
        error: None,
        total_tool_use_count: 0,
        total_tokens: 0,
        duration_ms: 0,
        worktree_path: None,
        worktree_branch: None,
        output_file: None,
        prompt: None,
    };
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Ok(response)));
    let result = AgentTool
        .execute(
            serde_json::json!({"prompt": "Help me", "team_name": "myteam", "name": "helper"}),
            &ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.data["status"], "teammate_spawned");
}

#[tokio::test]
async fn test_agent_tool_spawn_failed() {
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Err(
        "Agent limit exceeded".into()
    )));
    let result = AgentTool
        .execute(serde_json::json!({"prompt": "Do something"}), &ctx)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_agent_tool_with_worktree() {
    let response = AgentSpawnResponse {
        status: AgentSpawnStatus::Completed,
        agent_id: None,
        result: Some("Done".into()),
        error: None,
        total_tool_use_count: 1,
        total_tokens: 500,
        duration_ms: 1000,
        worktree_path: Some("/tmp/wt".into()),
        worktree_branch: Some("worktree-agent-abc".into()),
        output_file: None,
        prompt: None,
    };
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Ok(response)));
    let result = AgentTool
        .execute(
            serde_json::json!({"prompt": "Isolated work", "isolation": "worktree"}),
            &ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.data["worktreePath"], "/tmp/wt");
    assert_eq!(result.data["worktreeBranch"], "worktree-agent-abc");
}

// ── SendMessageTool tests ──

#[tokio::test]
async fn test_send_message_empty_to_rejected() {
    let ctx = ToolUseContext::test_default();
    let result = SendMessageTool
        .execute(serde_json::json!({"to": "", "message": "hello"}), &ctx)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_send_message_empty_content_rejected() {
    let ctx = ToolUseContext::test_default();
    let result = SendMessageTool
        .execute(serde_json::json!({"to": "agent-1", "message": ""}), &ctx)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_send_message_success() {
    let ctx = ctx_with_agent(MockAgentHandle::with_send(Ok("Message delivered".into())));
    let result = SendMessageTool
        .execute(
            serde_json::json!({"to": "researcher", "message": "Check this file"}),
            &ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.data.as_str().unwrap(), "Message delivered");
}

#[tokio::test]
async fn test_send_message_target_not_found() {
    let ctx = ctx_with_agent(MockAgentHandle::with_send(Err(
        "Agent 'unknown' not found".into()
    )));
    let result = SendMessageTool
        .execute(
            serde_json::json!({"to": "unknown", "message": "hello"}),
            &ctx,
        )
        .await;
    assert!(result.is_err());
}

// ── TeamCreateTool tests ──

#[tokio::test]
async fn test_team_create_empty_name_rejected() {
    let ctx = ToolUseContext::test_default();
    let result = TeamCreateTool
        .execute(serde_json::json!({"team_name": ""}), &ctx)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_team_create_success() {
    let ctx = ctx_with_agent(MockAgentHandle::with_team_create(Ok(
        "Team 'alpha' created".into(),
    )));
    let result = TeamCreateTool
        .execute(serde_json::json!({"team_name": "alpha"}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data.as_str().unwrap(), "Team 'alpha' created");
}

// ── TeamDeleteTool tests ──

#[tokio::test]
async fn test_team_delete_empty_name_rejected() {
    let ctx = ToolUseContext::test_default();
    let result = TeamDeleteTool
        .execute(serde_json::json!({"name": ""}), &ctx)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_team_delete_success() {
    let ctx = ctx_with_agent(MockAgentHandle::with_team_delete(Ok(
        "Team 'alpha' deleted".into(),
    )));
    let result = TeamDeleteTool
        .execute(serde_json::json!({"name": "alpha"}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data.as_str().unwrap(), "Team 'alpha' deleted");
}
