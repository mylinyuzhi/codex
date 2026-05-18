use std::sync::Arc;

use coco_tool_runtime::AgentHandle;
use coco_tool_runtime::AgentSpawnRequest;
use coco_tool_runtime::AgentSpawnResponse;
use coco_tool_runtime::AgentSpawnStatus;
use coco_tool_runtime::CreateTeamResult;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolUseContext;
use pretty_assertions::assert_eq;

use super::*;

// ── Mock AgentHandle ──

struct MockAgentHandle {
    spawn_result: tokio::sync::Mutex<Option<Result<AgentSpawnResponse, String>>>,
    send_result: tokio::sync::Mutex<Option<Result<String, String>>>,
    team_create_result: tokio::sync::Mutex<Option<Result<CreateTeamResult, String>>>,
    team_delete_result: tokio::sync::Mutex<Option<Result<String, String>>>,
}

struct ConnectedMcpHandle {
    servers: Vec<String>,
}

#[async_trait::async_trait]
impl coco_tool_runtime::McpHandle for ConnectedMcpHandle {
    async fn list_resources(
        &self,
        _server_name: Option<&str>,
    ) -> Result<Vec<coco_tool_runtime::mcp_handle::McpResourceInfo>, coco_error::BoxedError> {
        Ok(Vec::new())
    }

    async fn read_resource(
        &self,
        _server_name: &str,
        _resource_uri: &str,
    ) -> Result<Vec<coco_tool_runtime::mcp_handle::McpResourceContent>, coco_error::BoxedError>
    {
        Err(Box::new(coco_error::PlainError::new(
            "unused",
            coco_error::StatusCode::Internal,
        )))
    }

    async fn call_tool(
        &self,
        _server_name: &str,
        _tool_name: &str,
        _arguments: Option<serde_json::Value>,
    ) -> Result<coco_tool_runtime::mcp_handle::McpToolCallResult, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "unused",
            coco_error::StatusCode::Internal,
        )))
    }

    async fn authenticate(&self, _server_name: &str) -> Result<String, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "unused",
            coco_error::StatusCode::Internal,
        )))
    }

    async fn connected_servers(&self) -> Vec<String> {
        self.servers.clone()
    }

    async fn list_tools(&self) -> Vec<coco_tool_runtime::McpToolSchema> {
        self.servers
            .iter()
            .map(|server| coco_tool_runtime::McpToolSchema {
                server_name: server.clone(),
                tool_name: "tool".into(),
                description: None,
                input_schema: serde_json::json!({}),
                annotations: Default::default(),
            })
            .collect()
    }
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

    fn with_team_create(result: Result<CreateTeamResult, String>) -> Self {
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

    async fn create_team(
        &self,
        _request: coco_tool_runtime::CreateTeamRequest,
    ) -> Result<CreateTeamResult, String> {
        self.team_create_result
            .lock()
            .await
            .take()
            .unwrap_or(Err("no mock result".into()))
    }

    async fn delete_team(&self) -> Result<String, String> {
        self.team_delete_result
            .lock()
            .await
            .take()
            .unwrap_or(Err("no mock result".into()))
    }

    // resume_agent uses the trait default impl (Err "not supported").

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

/// Capturing variant — records the most recent `AgentSpawnRequest` so
/// tests can assert on what AgentTool actually built. Returns a
/// no-content completed response so AgentTool's render branch is
/// exercised end-to-end.
#[derive(Default)]
struct CapturingAgentHandle {
    pub last_request: tokio::sync::Mutex<Option<AgentSpawnRequest>>,
}

#[async_trait::async_trait]
impl AgentHandle for CapturingAgentHandle {
    async fn spawn_agent(&self, req: AgentSpawnRequest) -> Result<AgentSpawnResponse, String> {
        *self.last_request.lock().await = Some(req);
        Ok(AgentSpawnResponse {
            status: AgentSpawnStatus::Completed,
            agent_id: Some("captured".into()),
            result: Some("ok".into()),
            error: None,
            total_tool_use_count: 0,
            total_tokens: 0,
            duration_ms: 0,
            worktree_path: None,
            worktree_branch: None,
            output_file: None,
            prompt: None,
            ..Default::default()
        })
    }

    async fn send_message(&self, _: &str, _: &str) -> Result<String, String> {
        Err("unused".into())
    }
    async fn create_team(
        &self,
        _: coco_tool_runtime::CreateTeamRequest,
    ) -> Result<CreateTeamResult, String> {
        Err("unused".into())
    }
    async fn delete_team(&self) -> Result<String, String> {
        Err("unused".into())
    }
    // resume_agent uses the trait default impl.
    async fn query_agent_status(&self, _: &str) -> Result<AgentSpawnResponse, String> {
        Err("unused".into())
    }
    async fn get_agent_output(&self, _: &str) -> Result<String, String> {
        Err("unused".into())
    }
    async fn background_agent(&self, _: &str) -> Result<(), String> {
        Err("unused".into())
    }
}

#[test]
fn test_agent_tool_input_schema_carries_pr11_and_t9_fields() {
    let schema = AgentTool.input_schema();
    let p = &schema.properties;
    // PR #11 fields all described.
    for field in [
        "effort",
        "use_exact_tools",
        "mcp_servers",
        "disallowed_tools",
        "max_turns",
        "initial_prompt",
    ] {
        assert!(p.contains_key(field), "schema missing field: {field}");
    }
    // T9 enums are tight.
    let mode_enum = p["mode"].get("enum").unwrap().as_array().unwrap();
    let mode_values: Vec<&str> = mode_enum.iter().filter_map(|v| v.as_str()).collect();
    for expected in [
        "default",
        "plan",
        "dontAsk",
        "acceptEdits",
        "bubble",
        "bypassPermissions",
        "auto",
        "ask",
        "deny",
    ] {
        assert!(
            mode_values.contains(&expected),
            "mode enum missing {expected}; got {mode_values:?}"
        );
    }
    let effort_enum = p["effort"].get("enum").unwrap().as_array().unwrap();
    let effort_values: Vec<&str> = effort_enum.iter().filter_map(|v| v.as_str()).collect();
    for expected in ["none", "minimal", "low", "medium", "high", "max"] {
        assert!(
            effort_values.contains(&expected),
            "effort enum missing {expected}; got {effort_values:?}"
        );
    }
    let isolation_enum = p["isolation"].get("enum").unwrap().as_array().unwrap();
    let isolation_values: Vec<&str> = isolation_enum.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(isolation_values, vec!["none", "worktree", "remote"]);
}

#[test]
fn test_agent_spawn_request_inheritance_fields_are_serde_skip() {
    // Critical contract: in-process inheritance fields MUST NOT
    // serialize. Otherwise a JSON spawn request leaks Arc'd parent
    // state across boundaries (or noisily fails on transports that
    // don't tolerate them).
    let req = AgentSpawnRequest {
        prompt: "p".into(),
        description: Some("d".into()),
        ..Default::default()
    };
    let json = serde_json::to_string(&req).unwrap();
    // Note: `parent_runtime_snapshot` is no longer a field on
    // `AgentSpawnRequest` — it moved inside `SpawnMode::Fork` as a
    // non-optional `Arc<SubagentRuntimeSnapshot>`. The `spawn_mode`
    // field itself is `#[serde(skip)]`, so the snapshot never reaches
    // JSON either way.
    for forbidden in [
        "features",
        "tool_overrides",
        "parent_tool_filter",
        "spawn_mode",
        "definition",
    ] {
        assert!(
            !json.contains(forbidden),
            "field `{forbidden}` must be #[serde(skip)] but appears in json: {json}"
        );
    }
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
                "description": "do thing",
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
        ..Default::default()
    };
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Ok(response)));
    let result = AgentTool
        .execute(
            serde_json::json!({"prompt": "Find files", "description": "find files"}),
            &ctx,
        )
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
        ..Default::default()
    };
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Ok(response)));
    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "Background task",
                "description": "bg task",
                "run_in_background": true,
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.data["status"], "async_launched");
    assert_eq!(result.data["agentId"], "agent-abc");
}

#[tokio::test]
async fn test_agent_tool_async_launched_includes_output_file_metadata() {
    let response = AgentSpawnResponse {
        status: AgentSpawnStatus::AsyncLaunched,
        agent_id: Some("agent-abc".into()),
        output_file: Some("/tmp/agent-abc.output".into()),
        ..Default::default()
    };
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Ok(response)));
    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "Background task",
                "description": "bg task",
                "run_in_background": true,
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.data["status"], "async_launched");
    assert_eq!(result.data["outputFile"], "/tmp/agent-abc.output");
    assert_eq!(result.data["canReadOutputFile"], true);
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
        ..Default::default()
    };
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Ok(response)));
    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "Help me",
                "description": "help",
                "team_name": "myteam",
                "name": "helper",
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.data["status"], "teammate_spawned");
}

#[tokio::test]
async fn test_agent_tool_omitted_subagent_type_resolves_general_purpose() {
    let handle = Arc::new(CapturingAgentHandle::default());
    let mut ctx = ToolUseContext::test_default();
    ctx.agent = handle.clone();

    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "Do broad work",
                "description": "broad work",
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(result.data["status"], "completed");
    let request = handle
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    assert_eq!(request.subagent_type.as_deref(), Some("general-purpose"));
}

#[tokio::test]
async fn test_agent_tool_omitted_subagent_type_for_team_spawn_stays_untyped() {
    let handle = Arc::new(CapturingAgentHandle::default());
    let mut ctx = ToolUseContext::test_default();
    ctx.agent = handle.clone();

    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "Help the team",
                "description": "team help",
                "team_name": "alpha",
                "name": "helper",
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(result.data["status"], "completed");
    let request = handle
        .last_request
        .lock()
        .await
        .clone()
        .expect("captured request");
    assert_eq!(request.subagent_type, None);
    assert_eq!(request.team_name.as_deref(), Some("alpha"));
    assert_eq!(request.name.as_deref(), Some("helper"));
}

#[tokio::test]
async fn test_agent_tool_uses_active_team_when_team_name_omitted() {
    let response = AgentSpawnResponse {
        status: AgentSpawnStatus::TeammateSpawned,
        agent_id: Some("helper@active-team".into()),
        ..Default::default()
    };
    let mut ctx = ctx_with_agent(MockAgentHandle::with_spawn(Ok(response)));
    ctx.team_name = Some("active-team".into());
    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "Help me",
                "description": "help",
                "name": "helper",
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.data["status"], "teammate_spawned");
    assert_eq!(result.data["team_name"], "active-team");
}

#[tokio::test]
async fn test_agent_tool_teammate_cannot_spawn_teammate() {
    let mut ctx = ToolUseContext::test_default();
    ctx.is_teammate = true;
    ctx.team_name = Some("active-team".into());

    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "Help me",
                "description": "help",
                "name": "helper",
            }),
            &ctx,
        )
        .await;
    let err = result.expect_err("teammate nesting must be rejected");
    assert!(
        format!("{err}").contains("cannot spawn other teammates"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn test_agent_tool_spawn_failed() {
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Err(
        "Agent limit exceeded".into()
    )));
    let result = AgentTool
        .execute(
            serde_json::json!({"prompt": "Do something", "description": "do"}),
            &ctx,
        )
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
        ..Default::default()
    };
    let ctx = ctx_with_agent(MockAgentHandle::with_spawn(Ok(response)));
    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "Isolated work",
                "description": "iso work",
                "isolation": "worktree",
            }),
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
            serde_json::json!({
                "to": "researcher",
                "message": "Check this file",
                "summary": "review file",
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert_eq!(result.data.as_str().unwrap(), "Message delivered");
}

#[tokio::test]
async fn test_send_message_string_without_summary_rejected() {
    // TS `SendMessageTool.ts:668-674` requires `summary` whenever the
    // message is a plain string.
    let ctx = ctx_with_agent(MockAgentHandle::with_send(Ok("ok".into())));
    let result = SendMessageTool
        .execute(
            serde_json::json!({"to": "researcher", "message": "hi"}),
            &ctx,
        )
        .await;
    assert!(
        result.is_err(),
        "string message without summary must reject"
    );
}

#[tokio::test]
async fn test_send_message_target_not_found() {
    let ctx = ctx_with_agent(MockAgentHandle::with_send(Err(
        "Agent 'unknown' not found".into()
    )));
    let result = SendMessageTool
        .execute(
            serde_json::json!({
                "to": "unknown",
                "message": "hello",
                "summary": "say hello",
            }),
            &ctx,
        )
        .await;
    assert!(result.is_err());
}

// ── Auto-resume path (TS `SendMessageTool.ts:822-872` parity) ──

/// Mock TaskHandle that returns a pre-canned status for any task_id.
/// Used by the auto-resume tests to simulate a stopped bg task.
struct StoppedTaskHandle {
    status: coco_tool_runtime::BackgroundTaskStatus,
}

#[async_trait::async_trait]
impl coco_tool_runtime::TaskHandle for StoppedTaskHandle {
    async fn spawn_shell_task(
        &self,
        _: coco_tool_runtime::BackgroundShellRequest,
    ) -> Result<String, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "not used in test",
            coco_error::StatusCode::Internal,
        )))
    }
    async fn get_task_status(
        &self,
        task_id: &str,
    ) -> Result<coco_tool_runtime::BackgroundTaskInfo, coco_error::BoxedError> {
        Ok(coco_tool_runtime::BackgroundTaskInfo {
            task_id: task_id.into(),
            status: self.status,
            summary: None,
            output_file: None,
            tool_use_id: None,
            elapsed_seconds: 0.0,
            notified: false,
        })
    }
    async fn get_task_output_delta(
        &self,
        _: &str,
        _: i64,
    ) -> Result<coco_tool_runtime::TaskOutputDelta, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "not used in test",
            coco_error::StatusCode::Internal,
        )))
    }
    async fn kill_task(&self, _: &str) -> Result<(), coco_error::BoxedError> {
        Ok(())
    }
    async fn list_tasks(&self) -> Vec<coco_tool_runtime::BackgroundTaskInfo> {
        Vec::new()
    }
    async fn poll_notifications(&self) -> Vec<coco_tool_runtime::BackgroundTaskInfo> {
        Vec::new()
    }
}

/// Mock AgentHandle that records the most recent `resume_agent` call
/// so the test can verify SendMessageTool dispatched the auto-resume.
#[derive(Default)]
struct ResumeRecordingHandle {
    last_resume: tokio::sync::Mutex<Option<(String, String, String)>>,
}

#[async_trait::async_trait]
impl AgentHandle for ResumeRecordingHandle {
    async fn spawn_agent(&self, _: AgentSpawnRequest) -> Result<AgentSpawnResponse, String> {
        Err("not expected".into())
    }
    async fn send_message(&self, _: &str, _: &str) -> Result<String, String> {
        Err("send_message must NOT be reached when auto-resume fires".into())
    }
    async fn create_team(
        &self,
        _: coco_tool_runtime::CreateTeamRequest,
    ) -> Result<CreateTeamResult, String> {
        Err("not expected".into())
    }
    async fn delete_team(&self) -> Result<String, String> {
        Err("not expected".into())
    }
    async fn query_agent_status(&self, _: &str) -> Result<AgentSpawnResponse, String> {
        Err("not expected".into())
    }
    async fn get_agent_output(&self, _: &str) -> Result<String, String> {
        Err("not expected".into())
    }
    async fn background_agent(&self, _: &str) -> Result<(), String> {
        Err("not expected".into())
    }
    async fn resume_agent(
        &self,
        agent_id: &str,
        prompt: &str,
        session_id: &str,
    ) -> Result<AgentSpawnResponse, String> {
        *self.last_resume.lock().await = Some((agent_id.into(), prompt.into(), session_id.into()));
        Ok(AgentSpawnResponse {
            status: AgentSpawnStatus::AsyncLaunched,
            agent_id: Some("resumed-task-id-7af2".into()),
            result: None,
            error: None,
            total_tool_use_count: 0,
            total_tokens: 0,
            duration_ms: 0,
            worktree_path: None,
            worktree_branch: None,
            output_file: None,
            prompt: None,
            ..Default::default()
        })
    }
}

fn ctx_with_resume_handle_and_status(
    handle: Arc<ResumeRecordingHandle>,
    status: coco_tool_runtime::BackgroundTaskStatus,
) -> ToolUseContext {
    let mut ctx = ToolUseContext::test_default();
    ctx.agent = handle;
    ctx.task_handle = Some(Arc::new(StoppedTaskHandle { status }));
    ctx.session_id_for_history = Some("sess-test".into());
    ctx
}

#[tokio::test]
async fn test_send_message_auto_resumes_completed_task() {
    let handle = Arc::new(ResumeRecordingHandle::default());
    let ctx = ctx_with_resume_handle_and_status(
        handle.clone(),
        coco_tool_runtime::BackgroundTaskStatus::Completed,
    );
    let result = SendMessageTool
        .execute(
            serde_json::json!({
                "to": "agent-7af2",
                "message": "follow up question",
                "summary": "follow up",
            }),
            &ctx,
        )
        .await
        .unwrap();
    let recorded = handle.last_resume.lock().await.clone();
    let (id, prompt, sess) = recorded.expect("resume_agent must have been called");
    assert_eq!(id, "agent-7af2");
    assert_eq!(prompt, "follow up question");
    assert_eq!(sess, "sess-test");
    assert_eq!(
        result.data.get("auto_resumed"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(
        result.data.get("resumed_as"),
        Some(&serde_json::json!("resumed-task-id-7af2"))
    );
}

#[tokio::test]
async fn test_send_message_auto_resumes_failed_task() {
    let handle = Arc::new(ResumeRecordingHandle::default());
    let ctx = ctx_with_resume_handle_and_status(
        handle.clone(),
        coco_tool_runtime::BackgroundTaskStatus::Failed,
    );
    SendMessageTool
        .execute(
            serde_json::json!({
                "to": "agent-77",
                "message": "retry",
                "summary": "retry",
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(handle.last_resume.lock().await.is_some());
}

#[tokio::test]
async fn test_send_message_rejects_resume_with_empty_session_id() {
    // Auto-resume needs the parent session id to look up the persisted
    // transcript. Empty session id was being silently fed to
    // `resume_agent` and surfacing as a confusing inner "no metadata"
    // error. Now rejected upfront.
    let handle = Arc::new(ResumeRecordingHandle::default());
    let mut ctx = ToolUseContext::test_default();
    ctx.agent = handle.clone();
    ctx.task_handle = Some(Arc::new(StoppedTaskHandle {
        status: coco_tool_runtime::BackgroundTaskStatus::Completed,
    }));
    // session_id_for_history left at None.
    let result = SendMessageTool
        .execute(
            serde_json::json!({
                "to": "agent-stopped",
                "message": "follow up",
                "summary": "follow up",
            }),
            &ctx,
        )
        .await;
    let err = result.expect_err("empty session id must reject upfront");
    let msg = format!("{err}");
    assert!(
        msg.contains("parent session id is unavailable"),
        "error must explain why resume can't proceed; got: {msg}"
    );
    assert!(
        handle.last_resume.lock().await.is_none(),
        "resume_agent must not be invoked with empty session id"
    );
}

#[tokio::test]
async fn test_send_message_does_not_resume_running_task() {
    // When the task is still Running, the tool falls through to the
    // mailbox path (`send_message`) — auto-resume must NOT fire.
    // ResumeRecordingHandle's send_message panics if reached, so the
    // test confirms the falls-through error rather than the resume.
    let handle = Arc::new(ResumeRecordingHandle::default());
    let ctx = ctx_with_resume_handle_and_status(
        handle.clone(),
        coco_tool_runtime::BackgroundTaskStatus::Running,
    );
    let result = SendMessageTool
        .execute(
            serde_json::json!({
                "to": "agent-active",
                "message": "still working?",
                "summary": "ping",
            }),
            &ctx,
        )
        .await;
    assert!(
        result.is_err(),
        "Running task must fall through to mailbox path"
    );
    assert!(
        handle.last_resume.lock().await.is_none(),
        "no resume on Running"
    );
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
    let mut ctx = ctx_with_agent(MockAgentHandle::with_team_create(Ok(CreateTeamResult {
        team_name: "alpha".into(),
        lead_agent_id: "team-lead@alpha".into(),
        task_list_id: "alpha".into(),
    })));
    ctx.session_id_for_history = Some("session-1".into());
    let result = TeamCreateTool
        .execute(serde_json::json!({"team_name": "alpha"}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data["team_name"], "alpha");
    assert_eq!(result.data["task_list_id"], "alpha");
}

// ── TeamDeleteTool tests ──

#[tokio::test]
async fn test_team_delete_empty_input_accepted() {
    // TS parity (`TeamDeleteTool.ts:21`): the input schema is
    // `z.strictObject({})` — no `name` field. Empty input passes
    // through to the handle, which resolves the team from the active
    // session context. Without a side-channel mock here the underlying
    // call returns an error; we just verify the schema doesn't reject
    // empty input upfront.
    let ctx = ToolUseContext::test_default();
    let result = TeamDeleteTool.execute(serde_json::json!({}), &ctx).await;
    // The default `NoOpAgentHandle` returns an error; the schema-level
    // accept is what we're verifying, so we only assert the failure
    // mode is downstream (handle, not input parsing).
    assert!(result.is_err());
}

#[tokio::test]
async fn test_team_delete_success() {
    let ctx = ctx_with_agent(MockAgentHandle::with_team_delete(Ok(
        "Cleaned up directories and worktrees for team \"alpha\"".into(),
    )));
    let result = TeamDeleteTool
        .execute(serde_json::json!({}), &ctx)
        .await
        .unwrap();
    let message = result
        .data
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(message.contains("alpha"));
}

#[tokio::test]
async fn test_agent_tool_threads_definition_from_catalog_to_spawn_request() {
    // T7 contract: when `ToolUseContext.agent_catalog` is installed
    // and the user's `subagent_type` matches a catalog entry,
    // AgentTool must thread `Arc<AgentDefinition>` through
    // `AgentSpawnRequest.definition`. This is what lets the runner
    // consult `definition.model` / `definition.model_role` at the
    // resolution boundary.
    use coco_subagent::AgentCatalogSnapshot;
    use coco_types::{AgentDefinition, AgentSource, AgentTypeId, ModelRole, SubagentType};
    use std::collections::BTreeMap;

    let mut active = BTreeMap::new();
    active.insert(
        "Explore".to_string(),
        AgentDefinition {
            agent_type: AgentTypeId::Builtin(SubagentType::Explore),
            name: "Explore".into(),
            when_to_use: Some("desc".into()),
            description: Some("desc".into()),
            source: AgentSource::BuiltIn,
            model: Some("anthropic/claude-haiku-4-5".into()),
            model_role: Some(ModelRole::Explore),
            ..Default::default()
        },
    );
    let snapshot = Arc::new(AgentCatalogSnapshot::new(active, Vec::new()));

    let capturing = Arc::new(CapturingAgentHandle::default());
    let mut ctx = ToolUseContext::test_default();
    ctx.agent = capturing.clone();
    ctx.agent_catalog = Some(snapshot);

    let result = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "find files",
                "description": "search code",
                "subagent_type": "Explore",
            }),
            &ctx,
        )
        .await;
    assert!(result.is_ok(), "AgentTool exec must succeed: {result:?}");

    let captured = capturing.last_request.lock().await;
    let req = captured.as_ref().expect("spawn request must be captured");
    let def = req
        .definition
        .as_ref()
        .expect("AgentTool must thread the catalog's AgentDefinition into the request");
    assert_eq!(def.name, "Explore");
    assert_eq!(def.model.as_deref(), Some("anthropic/claude-haiku-4-5"));
    assert_eq!(def.model_role, Some(ModelRole::Explore));
}

#[tokio::test]
async fn test_agent_tool_rejects_definition_when_required_mcp_missing() {
    use coco_subagent::AgentCatalogSnapshot;
    use coco_types::{AgentDefinition, AgentSource, AgentTypeId, SubagentType};
    use std::collections::BTreeMap;

    let mut active = BTreeMap::new();
    active.insert(
        "Explore".to_string(),
        AgentDefinition {
            agent_type: AgentTypeId::Builtin(SubagentType::Explore),
            name: "Explore".into(),
            when_to_use: Some("desc".into()),
            description: Some("desc".into()),
            source: AgentSource::BuiltIn,
            required_mcp_servers: vec!["github".into()],
            ..Default::default()
        },
    );

    let capturing = Arc::new(CapturingAgentHandle::default());
    let mut ctx = ToolUseContext::test_default();
    ctx.agent = capturing.clone();
    ctx.agent_catalog = Some(Arc::new(AgentCatalogSnapshot::new(active, Vec::new())));
    ctx.mcp = Arc::new(ConnectedMcpHandle {
        servers: vec!["slack".into()],
    });

    let err = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "find files",
                "description": "search code paths",
                "subagent_type": "Explore",
            }),
            &ctx,
        )
        .await
        .expect_err("missing required MCP server must fail before spawn");
    assert!(
        format!("{err}").contains("requires MCP server"),
        "unexpected error: {err}"
    );
    assert!(
        capturing.last_request.lock().await.is_none(),
        "AgentTool must not spawn when required MCP validation fails"
    );
}

#[tokio::test]
async fn test_agent_tool_threads_none_when_catalog_absent() {
    // Catalog not installed → `definition` is `None`. The runner's
    // resolver still works via `subagent_type → role` mapping.
    let capturing = Arc::new(CapturingAgentHandle::default());
    let mut ctx = ToolUseContext::test_default();
    ctx.agent = capturing.clone();
    // ctx.agent_catalog defaults to None.

    let _ = AgentTool
        .execute(
            serde_json::json!({
                "prompt": "do work",
                "description": "noop",
                "subagent_type": "Explore",
            }),
            &ctx,
        )
        .await
        .unwrap();

    let captured = capturing.last_request.lock().await;
    let req = captured.as_ref().expect("spawn request must be captured");
    assert!(
        req.definition.is_none(),
        "without a catalog, no definition should be threaded",
    );
}

// ---------------------------------------------------------------------------
// render_for_model — TS parity with AgentTool.tsx::mapToolResultToToolResultBlockParam
// (4 branches: teammate_spawned / async_launched / completed / failed)
// ---------------------------------------------------------------------------

mod render_for_model_tests {
    use super::*;
    use coco_tool_runtime::ToolResultContentPart;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn teammate_spawned_emits_spawn_message() {
        // TS `AgentTool.tsx:1308-1312`: agent_id + name + team_name +
        // mailbox hint are the four required signals.
        let data = json!({
            "status": "teammate_spawned",
            "agentId": "agent-7",
            "name": "alice",
            "team_name": "alpha-team",
        });
        let parts = AgentTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert!(text.contains("Spawned successfully"), "got: {text}");
        assert!(text.contains("agent_id: agent-7"), "got: {text}");
        assert!(text.contains("name: alice"), "got: {text}");
        assert!(text.contains("team_name: alpha-team"), "got: {text}");
        assert!(text.contains("mailbox"), "got: {text}");
    }

    #[test]
    fn teammate_spawned_omitted_fields_render_as_empty_lines() {
        // When the spawn input omits `name` or `team_name` (e.g. a
        // partially-populated test fixture), the data envelope just
        // doesn't include those keys. Render still emits the labels
        // with empty values so downstream parsers see a consistent
        // 4-line shape.
        let data = json!({
            "status": "teammate_spawned",
            "agentId": "agent-9",
        });
        let parts = AgentTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert!(text.contains("agent_id: agent-9"), "got: {text}");
        assert!(text.contains("name: \n"), "got: {text}");
        assert!(text.contains("team_name: \n"), "got: {text}");
    }

    #[test]
    fn async_launched_with_output_file_includes_file_path() {
        let data = json!({
            "status": "async_launched",
            "agentId": "agent-99",
            "prompt": "Run the test suite",
            "description": "test",
            "outputFile": "/tmp/agent-99.log",
        });
        let parts = AgentTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert!(text.contains("Async agent launched"), "got: {text}");
        assert!(text.contains("agent-99"), "got: {text}");
        assert!(text.contains("/tmp/agent-99.log"), "got: {text}");
        assert!(text.contains("non-overlapping"), "got: {text}");
    }

    #[test]
    fn async_launched_without_output_file_uses_brief_instruction() {
        let data = json!({
            "status": "async_launched",
            "agentId": "agent-100",
            "prompt": "Watch metrics",
            "description": "watch",
        });
        let parts = AgentTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert!(text.contains("Briefly tell the user"), "got: {text}");
        assert!(!text.contains("output_file"), "got: {text}");
    }

    #[test]
    fn completed_includes_content_agent_id_usage_trailer() {
        let data = json!({
            "status": "completed",
            "content": "Found 3 bugs in auth.rs",
            "prompt": "investigate",
            "totalToolUseCount": 5,
            "totalTokens": 12345,
            "durationMs": 30000,
            "oneShot": false,
            "agentId": "agent-x",
        });
        let parts = AgentTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert!(text.starts_with("Found 3 bugs in auth.rs"), "got: {text}");
        assert!(text.contains("agentId: agent-x"), "got: {text}");
        assert!(text.contains("<usage>"), "got: {text}");
        assert!(text.contains("total_tokens: 12345"), "got: {text}");
        assert!(text.contains("tool_uses: 5"), "got: {text}");
        assert!(text.contains("duration_ms: 30000"), "got: {text}");
    }

    #[test]
    fn completed_one_shot_drops_trailer() {
        // Explore / Plan are one-shot built-ins — they cannot be
        // re-addressed via SendMessage, so the agentId hint and
        // <usage> block are dead weight (~135 chars per call).
        let data = json!({
            "status": "completed",
            "content": "Architecture summary: ...",
            "prompt": "summarize",
            "totalToolUseCount": 8,
            "totalTokens": 22000,
            "durationMs": 45000,
            "oneShot": true,
            "agentId": "agent-explore-1",
        });
        let parts = AgentTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert_eq!(text, "Architecture summary: ...");
        assert!(!text.contains("agentId"), "trailer must be dropped");
        assert!(!text.contains("<usage>"), "usage block must be dropped");
    }

    #[test]
    fn completed_with_worktree_keeps_trailer_even_when_one_shot() {
        // Worktree info is load-bearing for cleanup — even one-shot
        // agents that ran in a worktree must surface its path.
        let data = json!({
            "status": "completed",
            "content": "Refactor done",
            "prompt": "refactor",
            "totalToolUseCount": 12,
            "totalTokens": 30000,
            "durationMs": 60000,
            "oneShot": true,
            "agentId": "agent-wt",
            "worktreePath": "/tmp/wt",
            "worktreeBranch": "feat/x",
        });
        let parts = AgentTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert!(text.contains("Refactor done"));
        assert!(text.contains("worktreePath: /tmp/wt"));
        assert!(text.contains("worktreeBranch: feat/x"));
        assert!(text.contains("<usage>"));
    }

    #[test]
    fn failed_emits_error_message() {
        let data = json!({
            "status": "failed",
            "error": "agent crashed: connection refused",
        });
        let parts = AgentTool.render_for_model(&data);
        let ToolResultContentPart::Text { text, .. } = &parts[0] else {
            panic!("expected Text part");
        };
        assert_eq!(text, "Agent failed: agent crashed: connection refused");
    }
}
