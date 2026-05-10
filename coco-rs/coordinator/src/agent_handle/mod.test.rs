use super::*;

use coco_tool_runtime::AgentHandle;
use coco_tool_runtime::AgentSpawnRequest;
use coco_tool_runtime::AgentSpawnStatus;
use std::sync::Arc;
use tokio::sync::RwLock;

fn build_test_runtime() -> coco_config::RuntimeConfig {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let catalogs = coco_config::CatalogPaths::empty_in(tmp.path());
    let settings = coco_config::SettingsWithSource {
        merged: coco_config::Settings {
            // Multi-LLM SDK: Main has no implicit default. The
            // SwarmAgentHandle tests don't care which model — pick a
            // builtin so config build succeeds.
            model: Some("anthropic/claude-opus-4-7".into()),
            ..Default::default()
        },
        per_source: std::collections::HashMap::new(),
    };
    coco_config::build_runtime_config_with(
        settings,
        coco_config::EnvSnapshot::default(),
        coco_config::RuntimeOverrides::default(),
        catalogs,
    )
    .expect("runtime")
}

fn create_test_handle() -> SwarmAgentHandle {
    let runner = Arc::new(crate::runner::InProcessAgentRunner::new(
        "/tmp".to_string(),
        /*max_agents*/ 8,
    ));
    let team_manager = Arc::new(RwLock::new(None));
    let runtime_config = Arc::new(build_test_runtime());

    SwarmAgentHandle::new(runner, team_manager, "/tmp".to_string(), runtime_config)
}

// ── Handoff classifier orchestration (D4) ──

/// Side-query stub that returns canned text per call. Drives the
/// 2-stage classifier path without a real LLM.
struct StubSideQuery {
    responses: tokio::sync::Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl coco_tool_runtime::SideQuery for StubSideQuery {
    async fn query(
        &self,
        _request: coco_tool_runtime::SideQueryRequest,
    ) -> Result<coco_tool_runtime::SideQueryResponse, coco_error::BoxedError> {
        let next = self.responses.lock().await.pop().ok_or_else(|| {
            Box::new(coco_error::PlainError::new(
                "no canned response left",
                coco_error::StatusCode::Internal,
            )) as coco_error::BoxedError
        })?;
        Ok(coco_tool_runtime::SideQueryResponse {
            text: Some(next),
            tool_uses: Vec::new(),
            stop_reason: coco_types::SideQueryStopReason::EndTurn,
            usage: coco_types::SideQueryUsage::default(),
            model_used: "stub".into(),
        })
    }

    fn model_id(&self) -> &str {
        "stub"
    }
}

#[tokio::test]
async fn test_classifier_passes_through_when_side_query_unconfigured() {
    // Without a SideQuery installed the classifier degrades to a
    // pass-through (fail-open). TS parity: an unconfigured classifier
    // is a no-op rather than a hard fail.
    let handle = create_test_handle();
    let qr = coco_tool_runtime::AgentQueryResult {
        response_text: Some("computed answer".into()),
        messages: Vec::new(),
        turns: 1,
        input_tokens: 50,
        output_tokens: 25,
        tool_use_count: 3,
        cancelled: false,
    };
    let out = handle
        .classify_handoff_if_needed("general-purpose", &qr)
        .await;
    assert_eq!(out.as_deref(), Some("computed answer"));
}

#[tokio::test]
async fn test_classifier_skips_for_read_only_agents() {
    // `Explore` is read-only — `should_classify` returns false, so the
    // SideQuery is never invoked. We assert by configuring a stub that
    // would error if called.
    let mut handle = create_test_handle();
    handle.set_side_query(Arc::new(StubSideQuery {
        responses: tokio::sync::Mutex::new(Vec::new()), // would error
    }));
    let qr = coco_tool_runtime::AgentQueryResult {
        response_text: Some("explored result".into()),
        messages: Vec::new(),
        turns: 1,
        input_tokens: 50,
        output_tokens: 25,
        tool_use_count: 3,
        cancelled: false,
    };
    let out = handle.classify_handoff_if_needed("Explore", &qr).await;
    assert_eq!(out.as_deref(), Some("explored result"));
}

#[tokio::test]
async fn test_classifier_short_circuits_on_stage1_safe() {
    // Stage 1 SAFE → no stage 2 call. Only one canned response is
    // needed; if the classifier wrongly proceeds to stage 2 the test
    // would fail at the empty pop().
    let mut handle = create_test_handle();
    handle.set_side_query(Arc::new(StubSideQuery {
        responses: tokio::sync::Mutex::new(vec!["VERDICT: SAFE".into()]),
    }));
    let qr = coco_tool_runtime::AgentQueryResult {
        response_text: Some("clean output".into()),
        messages: Vec::new(),
        turns: 1,
        input_tokens: 50,
        output_tokens: 25,
        tool_use_count: 3, // > 0 so should_classify true for non-read-only
        cancelled: false,
    };
    let out = handle
        .classify_handoff_if_needed("general-purpose", &qr)
        .await;
    assert_eq!(out.as_deref(), Some("clean output"));
}

#[tokio::test]
async fn test_classifier_blocks_when_verdict_is_blocked() {
    // Stage 1 raises a flag, stage 2 confirms BLOCKED. The output
    // returned to the parent must be the SECURITY payload, not the
    // child's original response.
    let mut handle = create_test_handle();
    handle.set_side_query(Arc::new(StubSideQuery {
        // Vec::pop returns the *last* element, so reverse the call
        // order: stage 1 fires first → bottom of the vec.
        responses: tokio::sync::Mutex::new(vec![
            // Stage 2 (consumed second / popped second)
            "VERDICT: BLOCKED\nREASON: explicit prompt-injection attempt".into(),
            // Stage 1 (consumed first / popped first)
            "VERDICT: REVIEW".into(),
        ]),
    }));
    let qr = coco_tool_runtime::AgentQueryResult {
        response_text: Some("malicious child output".into()),
        messages: Vec::new(),
        turns: 1,
        input_tokens: 50,
        output_tokens: 25,
        tool_use_count: 5,
        cancelled: false,
    };
    let out = handle
        .classify_handoff_if_needed("general-purpose", &qr)
        .await
        .expect("classifier returned None");
    assert!(
        out.starts_with("SECURITY"),
        "Blocked verdict must surface as a SECURITY-prefixed payload, got: {out}"
    );
    assert!(
        !out.contains("malicious child output"),
        "child's raw output must not leak when blocked: {out}"
    );
}

#[tokio::test]
async fn test_set_runtime_config_replaces_main_model_resolver() {
    use coco_types::ModelRole;
    use coco_types::ModelSpec;
    use coco_types::ProviderApi;

    let mut handle = create_test_handle();

    // Publish a fresh runtime with Main re-pointed. T6 contract:
    // lookup is live, not frozen at construction. This is the
    // hot-reload scenario — `current_main_model_id` reflects the
    // newly published `Arc<RuntimeConfig>`, not the one captured by
    // `SwarmAgentHandle::new`.
    let mut runtime = build_test_runtime();
    runtime.model_roles.roles.insert(
        ModelRole::Main,
        coco_config::RoleSlots {
            primary: ModelSpec {
                provider: "anthropic".into(),
                api: ProviderApi::Anthropic,
                model_id: "claude-opus-4-7".into(),
                display_name: "Claude Opus 4.7".into(),
            },
            fallbacks: Vec::new(),
            recovery: None,
        },
    );
    handle.set_runtime_config(Arc::new(runtime));
    assert_eq!(handle.current_main_model_id(), "claude-opus-4-7");
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
        ..Default::default()
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
        ) -> Result<AgentQueryResult, coco_error::BoxedError> {
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
        subagent_type: Some("Explore".into()),
        ..Default::default()
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
        isolation: Some("worktree".into()),
        ..Default::default()
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
    // P2': background spawns now actually drive the engine in a
    // detached task. The handle must have an execution engine
    // installed — without one the spawn fails fast with a clear
    // error instead of silently returning AsyncLaunched.
    use async_trait::async_trait;
    use coco_tool_runtime::AgentQueryConfig;
    use coco_tool_runtime::AgentQueryEngine;
    use coco_tool_runtime::AgentQueryResult;

    struct BgStubEngine;
    #[async_trait]
    impl AgentQueryEngine for BgStubEngine {
        async fn execute_query(
            &self,
            _prompt: &str,
            _config: AgentQueryConfig,
        ) -> Result<AgentQueryResult, coco_error::BoxedError> {
            Ok(AgentQueryResult {
                response_text: Some("background result".into()),
                messages: Vec::new(),
                turns: 1,
                input_tokens: 10,
                output_tokens: 20,
                tool_use_count: 0,
                cancelled: false,
            })
        }
    }

    let mut handle = create_test_handle();
    handle.set_execution_engine(Arc::new(BgStubEngine));

    let request = AgentSpawnRequest {
        prompt: "Background work".to_string(),
        run_in_background: true,
        ..Default::default()
    };

    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::AsyncLaunched);
    assert!(response.agent_id.is_some());
}

#[tokio::test]
async fn test_spawn_subagent_async_without_engine_fails_cleanly() {
    // Without an engine the bg path can't drive the spawn — surface
    // a real failure instead of the prior phantom AsyncLaunched.
    let handle = create_test_handle();
    let request = AgentSpawnRequest {
        prompt: "Background work".to_string(),
        run_in_background: true,
        ..Default::default()
    };
    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::Failed);
}

#[tokio::test]
async fn test_spawn_teammate() {
    let handle = create_test_handle();
    let request = AgentSpawnRequest {
        prompt: "Help me".to_string(),
        name: Some("researcher".to_string()),
        team_name: Some("my-team".to_string()),
        ..Default::default()
    };

    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::TeammateSpawned);
    assert!(response.agent_id.is_some());
    assert!(response.agent_id.unwrap().contains("researcher@my-team"));
}

#[tokio::test]
async fn test_spawn_teammate_drives_engine_when_installed() {
    // Gap C regression: pre-fix, `spawn_teammate` called only
    // `runner.register_agent(...)` and never started the runner-loop.
    // Teammates registered as Running but no LLM turn ever fired.
    // This test installs a teammate execution engine and asserts that
    // (a) spawn returns TeammateSpawned, (b) the engine's run_query is
    // invoked at least once via the runner-loop kickoff, (c) the
    // teammate's task-state mirror exists.
    use crate::runner_loop::{
        AgentExecutionEngine, AgentQueryConfig as RunnerCfg, AgentQueryResult as RunnerResult,
    };
    use std::sync::atomic::{AtomicI32, Ordering};

    struct CountingTeammateEngine {
        calls: Arc<AtomicI32>,
    }

    #[async_trait::async_trait]
    impl AgentExecutionEngine for CountingTeammateEngine {
        async fn run_query(
            &self,
            _prompt: &str,
            _config: RunnerCfg,
        ) -> crate::Result<RunnerResult> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(RunnerResult {
                messages: Vec::new(),
                token_count: 10,
                input_tokens: 5,
                output_tokens: 5,
                turns: 1,
                tool_use_count: 0,
                cancelled: true,
                response_text: Some("ok".into()),
            })
        }
    }

    let calls = Arc::new(AtomicI32::new(0));
    let mut handle = create_test_handle();
    handle.set_teammate_execution_engine(Arc::new(CountingTeammateEngine {
        calls: calls.clone(),
    }));

    let response = handle
        .spawn_agent(AgentSpawnRequest {
            prompt: "do work".into(),
            name: Some("worker".into()),
            team_name: Some("alpha".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(response.status, AgentSpawnStatus::TeammateSpawned);
    let agent_id = response.agent_id.expect("agent_id present");

    // Task-state mirror is created at spawn time even if the runner-loop
    // hasn't ticked yet. Without Gap C the mirror was never registered.
    let mirror = handle.teammate_task_state(&agent_id).await;
    assert!(
        mirror.is_some(),
        "teammate task-state mirror must exist after spawn"
    );

    // Wait for the runner-loop to tick at least once. The engine stub
    // returns `cancelled: true` on the first turn so the loop exits
    // promptly.
    for _ in 0..50 {
        if calls.load(Ordering::SeqCst) >= 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        calls.load(Ordering::SeqCst) >= 1,
        "teammate runner-loop must invoke the engine's run_query (Gap C); \
         call count = {}",
        calls.load(Ordering::SeqCst),
    );
}

#[tokio::test]
async fn test_spawn_subagent_fresh_threads_definition_system_prompt() {
    // Regression: the Fresh branch of spawn_subagent used to seed
    // `AgentQueryConfig.system_prompt` with `String::new()`, dropping
    // the agent's role instructions. TS `runAgent.ts` calls
    // `agentDefinition.getSystemPrompt(...)` to build the prompt; the
    // Rust analogue is `AgentDefinition.system_prompt`. This test
    // installs a stub engine that captures the AgentQueryConfig and
    // asserts the definition's system_prompt body is present.
    use async_trait::async_trait;
    use coco_tool_runtime::AgentQueryConfig;
    use coco_tool_runtime::AgentQueryEngine;
    use coco_tool_runtime::AgentQueryResult;

    struct CapturingEngine {
        captured: Arc<tokio::sync::Mutex<Option<String>>>,
    }

    #[async_trait]
    impl AgentQueryEngine for CapturingEngine {
        async fn execute_query(
            &self,
            _prompt: &str,
            config: AgentQueryConfig,
        ) -> Result<AgentQueryResult, coco_error::BoxedError> {
            *self.captured.lock().await = Some(config.system_prompt);
            Ok(AgentQueryResult {
                response_text: Some("ok".into()),
                messages: Vec::new(),
                turns: 1,
                input_tokens: 0,
                output_tokens: 0,
                tool_use_count: 0,
                cancelled: false,
            })
        }
    }

    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let mut handle = create_test_handle();
    handle.set_execution_engine(Arc::new(CapturingEngine {
        captured: captured.clone(),
    }));

    let definition = std::sync::Arc::new(coco_types::AgentDefinition {
        name: "Explore".into(),
        agent_type: coco_types::AgentTypeId::Builtin(coco_types::SubagentType::Explore),
        system_prompt: Some("EXPLORE ROLE INSTRUCTIONS".into()),
        ..Default::default()
    });

    let request = AgentSpawnRequest {
        prompt: "do work".into(),
        subagent_type: Some("Explore".into()),
        definition: Some(definition),
        ..Default::default()
    };
    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::Completed);
    let observed = captured.lock().await.clone().expect("engine ran");
    assert!(
        observed.contains("EXPLORE ROLE INSTRUCTIONS"),
        "Fresh spawn must seed system_prompt from definition; got: {observed:?}"
    );
}

#[tokio::test]
async fn test_spawn_teammate_uses_base_system_prompt_when_no_initial_prompt() {
    // Pre-fix: spawn_teammate ignored the leader's resolved system
    // prompt and passed only `request.initial_prompt` (usually `None`)
    // to the runner-loop, which built only the addendum. This test
    // installs a base prompt + engine, captures the system_prompt the
    // runner-loop hands to the engine, and asserts the leader's base
    // is composed with the team addendum.
    use crate::runner_loop::{
        AgentExecutionEngine, AgentQueryConfig as RunnerCfg, AgentQueryResult as RunnerResult,
    };

    struct CapturingEngine {
        captured: Arc<tokio::sync::Mutex<Option<String>>>,
    }

    #[async_trait::async_trait]
    impl AgentExecutionEngine for CapturingEngine {
        async fn run_query(&self, _prompt: &str, config: RunnerCfg) -> crate::Result<RunnerResult> {
            *self.captured.lock().await = Some(config.system_prompt);
            Ok(RunnerResult {
                messages: Vec::new(),
                token_count: 0,
                input_tokens: 0,
                output_tokens: 0,
                turns: 1,
                tool_use_count: 0,
                cancelled: true,
                response_text: None,
            })
        }
    }

    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let mut handle = create_test_handle();
    handle.set_teammate_execution_engine(Arc::new(CapturingEngine {
        captured: captured.clone(),
    }));
    handle
        .set_teammate_base_system_prompt("LEADER PROMPT BODY".into())
        .await;

    handle
        .spawn_agent(AgentSpawnRequest {
            prompt: "do work".into(),
            name: Some("worker".into()),
            team_name: Some("alpha".into()),
            ..Default::default()
        })
        .await
        .unwrap();

    for _ in 0..50 {
        if captured.lock().await.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let observed = captured.lock().await.clone().expect("engine ran");
    assert!(
        observed.contains("LEADER PROMPT BODY"),
        "teammate must inherit leader base prompt; got: {observed}"
    );
    assert!(
        observed.contains("Agent Teammate Communication"),
        "teammate prompt must include team addendum; got: {observed}"
    );
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
    let result = handle.delete_team().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_query_unknown_agent() {
    let handle = create_test_handle();
    let result = handle.query_agent_status("nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_spawn_subagent_validation_failure_does_not_leak_state() {
    // Regression: spawn_subagent used to push a Pending entry to the
    // agents list BEFORE running validation. A worktree-creation or
    // missing-engine failure then left a dangling state visible to
    // SubagentPanel / query_agent_status. The fix only commits state
    // after both gates pass.
    let handle = create_test_handle();
    let request = AgentSpawnRequest {
        prompt: "isolated work".into(),
        // Worktree without a manager — first gate fails.
        isolation: Some("worktree".into()),
        ..Default::default()
    };
    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::Failed);
    let agents = handle.agents().read().await;
    assert!(
        agents.is_empty(),
        "validation failure must not leave a dangling agent entry; got {agents:?}",
    );
}

#[tokio::test]
async fn test_subagent_start_hook_injects_additional_context() {
    // SubagentStart hook fires before engine.execute_query and
    // additional_contexts are prepended to the prompt as
    // <hook-additional-context> blocks. TS parity:
    // runAgent.ts:530-555.
    use async_trait::async_trait;
    use coco_tool_runtime::AgentQueryConfig;
    use coco_tool_runtime::AgentQueryEngine;
    use coco_tool_runtime::AgentQueryResult;

    struct CapturingEngine {
        captured_prompt: tokio::sync::Mutex<Option<String>>,
    }
    #[async_trait]
    impl AgentQueryEngine for CapturingEngine {
        async fn execute_query(
            &self,
            prompt: &str,
            _config: AgentQueryConfig,
        ) -> Result<AgentQueryResult, coco_error::BoxedError> {
            *self.captured_prompt.lock().await = Some(prompt.to_string());
            Ok(AgentQueryResult {
                response_text: Some("ok".into()),
                messages: Vec::new(),
                turns: 1,
                input_tokens: 0,
                output_tokens: 0,
                tool_use_count: 0,
                cancelled: false,
            })
        }
    }

    // Build a registry with one SubagentStart hook that injects context.
    let registry = coco_hooks::HookRegistry::new();
    let hook = coco_hooks::HookDefinition {
        event: coco_types::HookEventType::SubagentStart,
        matcher: None,
        handler: coco_hooks::HookHandler::Prompt {
            prompt: "INJECTED CONTEXT FROM HOOK".into(),
            model: None,
            timeout_ms: None,
        },
        priority: 0,
        scope: coco_types::HookScope::Session,
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        status_message: None,
    };
    registry.register_deduped(hook);

    let captured = Arc::new(CapturingEngine {
        captured_prompt: tokio::sync::Mutex::new(None),
    });
    let mut handle = create_test_handle();
    handle.set_execution_engine(captured.clone());
    handle.set_hook_registry(Arc::new(registry));

    let response = handle
        .spawn_agent(AgentSpawnRequest {
            prompt: "ORIGINAL PROMPT".into(),
            subagent_type: Some("Explore".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(response.status, AgentSpawnStatus::Completed);

    let observed = captured.captured_prompt.lock().await.clone().unwrap();
    assert!(
        observed.contains("ORIGINAL PROMPT"),
        "original prompt must survive; got: {observed}"
    );
    assert!(
        observed.contains("INJECTED CONTEXT FROM HOOK"),
        "SubagentStart additional_contexts must be prepended; got: {observed}"
    );
    assert!(
        observed.contains("<hook-additional-context>"),
        "additional_contexts must be wrapped in XML blocks; got: {observed}"
    );
}

#[tokio::test]
async fn test_subagent_start_hook_no_context_leaves_prompt_unchanged() {
    // When SubagentStart hooks return no additional_contexts (e.g. a
    // command hook that exits 0 with no JSON output), the prompt
    // passes through unchanged — no empty <hook-additional-context>
    // wrapper.
    use async_trait::async_trait;
    use coco_tool_runtime::AgentQueryConfig;
    use coco_tool_runtime::AgentQueryEngine;
    use coco_tool_runtime::AgentQueryResult;

    struct CapturingEngine {
        captured_prompt: tokio::sync::Mutex<Option<String>>,
    }
    #[async_trait]
    impl AgentQueryEngine for CapturingEngine {
        async fn execute_query(
            &self,
            prompt: &str,
            _config: AgentQueryConfig,
        ) -> Result<AgentQueryResult, coco_error::BoxedError> {
            *self.captured_prompt.lock().await = Some(prompt.to_string());
            Ok(AgentQueryResult {
                response_text: Some("ok".into()),
                messages: Vec::new(),
                turns: 1,
                input_tokens: 0,
                output_tokens: 0,
                tool_use_count: 0,
                cancelled: false,
            })
        }
    }

    let captured = Arc::new(CapturingEngine {
        captured_prompt: tokio::sync::Mutex::new(None),
    });
    let mut handle = create_test_handle();
    handle.set_execution_engine(captured.clone());
    // Empty registry — no SubagentStart hooks → no contexts → original
    // prompt passes through.
    handle.set_hook_registry(Arc::new(coco_hooks::HookRegistry::new()));

    handle
        .spawn_agent(AgentSpawnRequest {
            prompt: "ORIGINAL PROMPT".into(),
            subagent_type: Some("Explore".into()),
            ..Default::default()
        })
        .await
        .unwrap();

    let observed = captured.captured_prompt.lock().await.clone().unwrap();
    assert_eq!(observed, "ORIGINAL PROMPT");
    assert!(!observed.contains("<hook-additional-context>"));
}

#[tokio::test]
async fn test_spawn_subagent_resume_mode_preserves_tool_results() {
    // SpawnMode::Resume must NOT rewrite tool_result blocks to
    // FORK_PLACEHOLDER (that's Fork's job, and a resumed child needs
    // the real outputs to continue). Verifies via the engine stub
    // that fork_context_messages flow through verbatim.
    use async_trait::async_trait;
    use coco_tool_runtime::AgentQueryConfig;
    use coco_tool_runtime::AgentQueryEngine;
    use coco_tool_runtime::AgentQueryResult;

    struct CapturingEngine {
        captured: tokio::sync::Mutex<Option<Vec<serde_json::Value>>>,
    }
    #[async_trait]
    impl AgentQueryEngine for CapturingEngine {
        async fn execute_query(
            &self,
            _prompt: &str,
            config: AgentQueryConfig,
        ) -> Result<AgentQueryResult, coco_error::BoxedError> {
            *self.captured.lock().await = Some(config.fork_context_messages.clone());
            Ok(AgentQueryResult {
                response_text: Some("resumed".into()),
                messages: Vec::new(),
                turns: 1,
                input_tokens: 0,
                output_tokens: 0,
                tool_use_count: 0,
                cancelled: false,
            })
        }
    }

    let captured = Arc::new(CapturingEngine {
        captured: tokio::sync::Mutex::new(None),
    });
    let mut handle = create_test_handle();
    handle.set_execution_engine(captured.clone());

    let parent_messages = vec![serde_json::json!({
        "role": "user",
        "content": [{
            "type": "tool_result",
            "tool_use_id": "abc",
            "content": "REAL TOOL OUTPUT - must survive",
        }],
    })];
    let request = AgentSpawnRequest {
        prompt: "follow up".into(),
        spawn_mode: coco_tool_runtime::SpawnMode::Resume {
            parent_messages: parent_messages.clone(),
        },
        ..Default::default()
    };
    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::Completed);

    let observed = captured.captured.lock().await.clone().unwrap();
    let serialized = serde_json::to_string(&observed).unwrap();
    assert!(
        serialized.contains("REAL TOOL OUTPUT - must survive"),
        "Resume must preserve tool_result content verbatim; got {serialized}",
    );
    assert!(
        !serialized.contains(coco_subagent::FORK_PLACEHOLDER),
        "Resume must NOT rewrite tool_results to FORK_PLACEHOLDER; got {serialized}",
    );
}
