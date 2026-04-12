use tokio::sync::mpsc;
use tokio::sync::oneshot;

use std::sync::Arc;

use super::*;

fn make_runner(max_agents: i32) -> InProcessAgentRunner {
    let (tx, _rx) = mpsc::channel::<PermissionRequest>(16);
    let bridge = Arc::new(PermissionBridge::new(tx));
    InProcessAgentRunner::new(bridge, "/tmp/test".into(), max_agents)
}

fn make_spawn_config(name: &str, team: &str) -> SpawnConfig {
    SpawnConfig {
        name: name.into(),
        team_name: team.into(),
        prompt: "test prompt".into(),
        color: None,
        plan_mode_required: false,
        model: None,
        working_dir: None,
        system_prompt: None,
        allowed_tools: vec![],
        allow_permission_prompts: false,
        effort: None,
        use_exact_tools: false,
        isolation: coco_types::AgentIsolation::None,
        memory_scope: None,
        mcp_servers: vec![],
        disallowed_tools: vec![],
        max_turns: None,
    }
}

#[tokio::test]
async fn test_spawn_agent_success() {
    let runner = make_runner(5);
    let result = runner
        .spawn_agent(make_spawn_config("researcher", "team-a"))
        .await;

    assert!(result.success);
    assert_eq!(result.agent_id, "researcher@team-a");
    assert!(result.error.is_none());

    let active: Vec<AgentContext> = runner.active_agents().await;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].agent_name, "researcher");
}

#[tokio::test]
async fn test_spawn_agent_duplicate_rejected() {
    let runner = make_runner(5);
    let r1 = runner
        .spawn_agent(make_spawn_config("worker", "team-a"))
        .await;
    assert!(r1.success);

    let r2 = runner
        .spawn_agent(make_spawn_config("worker", "team-a"))
        .await;
    assert!(!r2.success);
    assert!(
        r2.error
            .as_ref()
            .is_some_and(|e: &String| e.contains("already exists"))
    );
}

#[tokio::test]
async fn test_spawn_agent_max_capacity() {
    let runner = make_runner(1);
    let r1 = runner.spawn_agent(make_spawn_config("a1", "team")).await;
    assert!(r1.success);

    let r2 = runner.spawn_agent(make_spawn_config("a2", "team")).await;
    assert!(!r2.success);
    assert!(
        r2.error
            .as_ref()
            .is_some_and(|e: &String| e.contains("Max agents"))
    );
}

#[tokio::test]
async fn test_cancel_agent() {
    let runner = make_runner(5);
    let result = runner
        .spawn_agent(make_spawn_config("agent-x", "team"))
        .await;
    assert!(result.success);

    let cancelled = runner.cancel_agent("agent-x@team").await;
    assert!(cancelled);

    // Agent should be removed
    assert_eq!(runner.active_count().await, 0);

    // Cancelling again should return false
    assert!(!runner.cancel_agent("agent-x@team").await);
}

#[tokio::test]
async fn test_cancel_all_agents() {
    let runner = make_runner(5);
    runner.spawn_agent(make_spawn_config("a1", "team")).await;
    runner.spawn_agent(make_spawn_config("a2", "team")).await;
    assert_eq!(runner.active_count().await, 2);

    runner.cancel_all().await;
    assert_eq!(runner.active_count().await, 0);
}

#[tokio::test]
async fn test_collect_result_with_channel() {
    let runner = make_runner(5);
    let spawn = runner
        .spawn_agent(make_spawn_config("worker", "team"))
        .await;
    assert!(spawn.success);

    let (result_tx, result_rx) = oneshot::channel::<RunnerResult>();

    runner.set_result_channel("worker@team", result_rx).await;

    // Simulate agent completing
    result_tx
        .send(RunnerResult {
            success: true,
            error: None,
            output: Some("task done".into()),
            turns: 3,
        })
        .expect("send result");

    let result: Option<RunnerResult> = runner.collect_result("worker@team").await;
    assert!(result.is_some());
    let r = result.expect("result exists");
    assert!(r.success);
    assert_eq!(r.output.as_deref(), Some("task done"));
    assert_eq!(r.turns, 3);
}

#[tokio::test]
async fn test_get_context() {
    let runner = make_runner(5);
    runner
        .spawn_agent(SpawnConfig {
            name: "ctx-agent".into(),
            team_name: "ctx-team".into(),
            prompt: "hello".into(),
            color: Some("blue".into()),
            plan_mode_required: true,
            model: Some("opus-4".into()),
            working_dir: Some("/custom/dir".into()),
            system_prompt: Some("You are helpful.".into()),
            allowed_tools: vec!["Read".into(), "Write".into()],
            allow_permission_prompts: true,
            effort: Some("high".into()),
            use_exact_tools: true,
            isolation: coco_types::AgentIsolation::Worktree,
            memory_scope: Some(coco_types::MemoryScope::Project),
            mcp_servers: vec!["github".into()],
            disallowed_tools: vec!["Bash".into()],
            max_turns: Some(15),
        })
        .await;

    let ctx: Option<AgentContext> = runner.get_context("ctx-agent@ctx-team").await;
    assert!(ctx.is_some());
    let ctx = ctx.expect("context exists");
    assert_eq!(ctx.agent_id, "ctx-agent@ctx-team");
    assert_eq!(ctx.color.as_deref(), Some("blue"));
    assert!(ctx.plan_mode_required);
    assert_eq!(ctx.model.as_deref(), Some("opus-4"));
    assert_eq!(ctx.working_dir, "/custom/dir");
    assert_eq!(ctx.allowed_tools, vec!["Read", "Write"]);
    assert!(ctx.allow_permission_prompts);
    assert_eq!(ctx.effort.as_deref(), Some("high"));
    assert!(ctx.use_exact_tools);
    assert_eq!(ctx.isolation, coco_types::AgentIsolation::Worktree);
    assert_eq!(ctx.memory_scope, Some(coco_types::MemoryScope::Project));
    assert_eq!(ctx.mcp_servers, vec!["github"]);
    assert_eq!(ctx.disallowed_tools, vec!["Bash"]);
    assert_eq!(ctx.max_turns, Some(15));
}

#[tokio::test]
async fn test_collect_nonexistent_agent() {
    let runner = make_runner(5);
    let result: Option<RunnerResult> = runner.collect_result("nonexistent@team").await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_permission_bridge_resolve() {
    let (tx, mut rx) = mpsc::channel::<PermissionRequest>(16);
    let bridge = Arc::new(PermissionBridge::new(tx));

    let bridge_clone: Arc<PermissionBridge> = Arc::clone(&bridge);
    let handle = tokio::spawn(async move {
        bridge_clone
            .request_permission(PermissionRequest {
                id: "req-1".into(),
                agent_id: "worker@team".into(),
                tool_name: "Bash".into(),
                description: "run tests".into(),
                input: serde_json::json!({"command": "cargo test"}),
            })
            .await
    });

    // Leader side: receive and resolve
    let received: PermissionRequest = rx.recv().await.expect("should receive request");
    assert_eq!(received.id, "req-1");
    assert_eq!(received.tool_name, "Bash");

    let resolved = bridge
        .resolve(
            "req-1",
            PermissionResolution {
                decision: PermissionDecision::Approved,
                feedback: None,
            },
        )
        .await;
    assert!(resolved);

    let result: Result<PermissionResolution, String> = handle.await.expect("task should complete");
    let resolution = result.expect("should get resolution");
    assert_eq!(resolution.decision, PermissionDecision::Approved);
}
