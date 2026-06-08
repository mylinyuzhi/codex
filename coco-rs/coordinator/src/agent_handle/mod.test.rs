use super::*;

use coco_tool_runtime::AgentHandle;
use coco_tool_runtime::AgentSpawnRequest;
use coco_tool_runtime::AgentSpawnStatus;
use coco_tool_runtime::CreateTeamRequest;
use coco_tool_runtime::CreateTeamResult;
use coco_tool_runtime::TaskHandle;
use coco_tool_runtime::TaskListHandleRef;
use coco_tool_runtime::TeamTaskListRouter;
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
        source_paths: std::collections::HashMap::new(),
    };
    coco_config::build_runtime_config_with(
        settings,
        coco_config::EnvSnapshot::default(),
        coco_config::RuntimeOverrides::default(),
        catalogs,
        coco_config::parse_enabled_setting_sources(None),
    )
    .expect("runtime")
}

fn create_test_handle() -> SwarmAgentHandle {
    create_test_handle_with_registry(Arc::new(TestAgentTaskRegistry::default()))
}

fn create_test_handle_with_registry(
    task_registry: coco_tool_runtime::AgentTaskRegistryRef,
) -> SwarmAgentHandle {
    let runner = Arc::new(crate::runner::InProcessAgentRunner::new(
        "/tmp".to_string(),
        /*max_agents*/ 8,
    ));
    let team_manager = Arc::new(RwLock::new(None));
    let runtime_config = Arc::new(build_test_runtime());

    SwarmAgentHandle::new(
        runner,
        team_manager,
        "/tmp".to_string(),
        runtime_config,
        task_registry,
    )
}

#[derive(Default)]
struct TestAgentTaskRegistry {
    states: std::sync::Mutex<std::collections::HashMap<String, coco_types::TaskStateBase>>,
    outputs: std::sync::Mutex<std::collections::HashMap<String, String>>,
    detaches: std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Notify>>>,
    current_work:
        std::sync::Mutex<std::collections::HashMap<String, tokio_util::sync::CancellationToken>>,
}

impl TestAgentTaskRegistry {
    fn insert_task(
        &self,
        task_id: String,
        task_type: coco_types::TaskType,
        description: &str,
        tool_use_id: Option<&str>,
        registration: coco_tool_runtime::AgentRegistration,
    ) -> String {
        let is_backgrounded = matches!(
            registration,
            coco_tool_runtime::AgentRegistration::Background
        );
        let extras = match task_type {
            coco_types::TaskType::BgAgent => coco_types::TaskExtras::bg_agent_default(),
            coco_types::TaskType::Dream => coco_types::TaskExtras::dream(),
            coco_types::TaskType::Teammate | coco_types::TaskType::RemoteTeammate => {
                panic!("insert_task: teammate rows must go through insert_teammate_task")
            }
            coco_types::TaskType::Shell => coco_types::TaskExtras::shell_default(),
        };
        let mut extras = extras;
        extras.set_backgrounded(is_backgrounded);
        self.states.lock().expect("states lock").insert(
            task_id.clone(),
            coco_types::TaskStateBase {
                id: task_id.clone(),
                status: coco_types::TaskStatus::Running,
                notified: false,
                description: description.to_string(),
                tool_use_id: tool_use_id.map(str::to_string),
                start_time: 0,
                end_time: None,
                total_paused_ms: None,
                output_file: Some(format!("/tmp/{task_id}.out")),
                output_offset: 0,
                extras,
            },
        );
        self.detaches
            .lock()
            .expect("detaches lock")
            .insert(task_id.clone(), Arc::new(tokio::sync::Notify::new()));
        task_id
    }

    fn mark_terminal(&self, task_id: &str, status: coco_types::TaskStatus) {
        if let Some(state) = self.states.lock().expect("states lock").get_mut(task_id) {
            state.status = status;
            state.end_time = Some(1);
        }
    }

    fn teammate_by_agent_id(&self, agent_id: &str) -> Option<coco_types::TaskStateBase> {
        self.states
            .lock()
            .expect("states lock")
            .values()
            .find(|state| {
                state.task_type() == coco_types::TaskType::Teammate
                    && state
                        .teammate_extras()
                        .is_some_and(|extras| extras.agent_ref.to_string() == agent_id)
            })
            .cloned()
    }
}

#[async_trait::async_trait]
impl coco_tool_runtime::TaskHandle for TestAgentTaskRegistry {
    async fn register_agent_task(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        _invoking_agent_id: Option<&str>,
        _cancel: tokio_util::sync::CancellationToken,
        registration: coco_tool_runtime::AgentRegistration,
    ) -> String {
        self.insert_task(
            coco_types::generate_task_id(coco_types::TaskType::BgAgent),
            coco_types::TaskType::BgAgent,
            description,
            tool_use_id,
            registration,
        )
    }

    async fn register_agent_task_with_id(
        &self,
        task_id: String,
        description: &str,
        tool_use_id: Option<&str>,
        _invoking_agent_id: Option<&str>,
        _cancel: tokio_util::sync::CancellationToken,
        registration: coco_tool_runtime::AgentRegistration,
    ) -> String {
        self.insert_task(
            task_id,
            coco_types::TaskType::BgAgent,
            description,
            tool_use_id,
            registration,
        )
    }

    async fn append_output(&self, task_id: &str, chunk: &str) {
        self.outputs
            .lock()
            .expect("outputs lock")
            .entry(task_id.to_string())
            .or_default()
            .push_str(chunk);
    }

    async fn set_progress_summary(&self, _task_id: &str, _summary: String) {}

    async fn set_progress(&self, _task_id: &str, _progress: coco_types::TaskProgress) {}

    async fn mark_completed(
        &self,
        task_id: &str,
        payload: coco_tool_runtime::AgentCompletionPayload,
    ) {
        if let Some(result) = payload.result {
            self.outputs
                .lock()
                .expect("outputs lock")
                .insert(task_id.to_string(), result);
        }
        self.mark_terminal(task_id, coco_types::TaskStatus::Completed);
    }

    async fn mark_failed(&self, task_id: &str, _error: &str) {
        self.mark_terminal(task_id, coco_types::TaskStatus::Failed);
    }

    async fn complete_silent(&self, task_id: &str, succeeded: bool) {
        self.mark_terminal(
            task_id,
            if succeeded {
                coco_types::TaskStatus::Completed
            } else {
                coco_types::TaskStatus::Failed
            },
        );
    }

    async fn register_dream_task(
        &self,
        description: &str,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> String {
        self.insert_task(
            coco_types::generate_task_id(coco_types::TaskType::Dream),
            coco_types::TaskType::Dream,
            description,
            None,
            coco_tool_runtime::AgentRegistration::Background,
        )
    }

    async fn register_teammate_task(
        &self,
        request: coco_tool_runtime::TeammateTaskRegistration,
    ) -> String {
        let task_id = coco_types::generate_task_id(coco_types::TaskType::Teammate);
        let mut teammate_extras = coco_types::TeammateExtras::new(
            request.agent_ref.clone(),
            request.backend_type,
            request.prompt,
        );
        teammate_extras.pane_id = request.pane_id;
        self.states.lock().expect("states lock").insert(
            task_id.clone(),
            coco_types::TaskStateBase {
                id: task_id.clone(),
                status: coco_types::TaskStatus::Running,
                notified: false,
                description: request.agent_ref.to_string(),
                tool_use_id: None,
                start_time: 0,
                end_time: None,
                total_paused_ms: None,
                output_file: Some(format!("/tmp/{task_id}.out")),
                output_offset: 0,
                extras: coco_types::TaskExtras::Teammate(teammate_extras),
            },
        );
        task_id
    }

    async fn teammate_task_state(&self, agent_id: &str) -> Option<coco_types::TaskStateBase> {
        self.teammate_by_agent_id(agent_id)
    }

    async fn update_teammate_task(
        &self,
        agent_id: &str,
        update: coco_tool_runtime::TeammateTaskUpdate,
    ) {
        let mut states = self.states.lock().expect("states lock");
        let Some(state) = states.values_mut().find(|state| {
            state.task_type() == coco_types::TaskType::Teammate
                && state
                    .teammate_extras()
                    .is_some_and(|extras| extras.agent_ref.to_string() == agent_id)
        }) else {
            return;
        };
        let Some(extras) = state.teammate_extras_mut() else {
            return;
        };
        update.is_idle.apply_required(&mut extras.is_idle);
        update.result.apply(&mut extras.result);
        update.error.apply(&mut extras.error);
    }

    async fn set_teammate_current_work_cancel(
        &self,
        agent_id: &str,
        cancel: Option<tokio_util::sync::CancellationToken>,
    ) -> bool {
        let exists = self.teammate_by_agent_id(agent_id).is_some();
        if !exists {
            return false;
        }
        let mut current = self.current_work.lock().expect("current work lock");
        if let Some(cancel) = cancel {
            current.insert(agent_id.to_string(), cancel);
        } else {
            current.remove(agent_id);
        }
        true
    }

    async fn interrupt_teammate_current_work(&self, agent_id: &str) -> Result<bool, String> {
        if self.teammate_by_agent_id(agent_id).is_none() {
            return Err(format!("Teammate '{agent_id}' not found"));
        }
        let current = self.current_work.lock().expect("current work lock");
        let Some(cancel) = current.get(agent_id) else {
            return Ok(false);
        };
        cancel.cancel();
        Ok(true)
    }

    async fn complete_teammate_task(
        &self,
        agent_id: &str,
        status: coco_types::TaskStatus,
        result: Option<String>,
        error: Option<String>,
    ) {
        let mut states = self.states.lock().expect("states lock");
        let Some(state) = states.values_mut().find(|state| {
            state.task_type() == coco_types::TaskType::Teammate
                && state
                    .teammate_extras()
                    .is_some_and(|extras| extras.agent_ref.to_string() == agent_id)
        }) else {
            return;
        };
        state.status = status;
        state.notified = true;
        if let Some(extras) = state.teammate_extras_mut() {
            extras.result = result;
            extras.error = error;
        }
    }

    async fn detach_handle(&self, task_id: &str) -> Option<Arc<tokio::sync::Notify>> {
        self.detaches
            .lock()
            .expect("detaches lock")
            .get(task_id)
            .cloned()
    }

    async fn read_output(&self, task_id: &str) -> String {
        self.outputs
            .lock()
            .expect("outputs lock")
            .get(task_id)
            .cloned()
            .unwrap_or_default()
    }

    async fn task_state(&self, task_id: &str) -> Option<coco_types::TaskStateBase> {
        self.states
            .lock()
            .expect("states lock")
            .get(task_id)
            .cloned()
    }

    async fn output_file_path(&self, task_id: &str) -> Option<std::path::PathBuf> {
        Some(std::path::PathBuf::from(format!("/tmp/{task_id}.out")))
    }

    async fn is_terminal(&self, task_id: &str) -> bool {
        self.states
            .lock()
            .expect("states lock")
            .get(task_id)
            .map(|state| state.status.is_terminal())
            .unwrap_or(false)
    }
}

#[derive(Debug)]
struct TestTaskListRouter;

#[async_trait::async_trait]
impl TeamTaskListRouter for TestTaskListRouter {
    async fn route_team_task_list(
        &self,
        _task_list_id: &str,
    ) -> Result<TaskListHandleRef, coco_error::BoxedError> {
        Ok(Arc::new(coco_tool_runtime::InMemoryTaskListHandle::new()))
    }

    async fn clear_team_task_list_route(&self) -> Result<(), coco_error::BoxedError> {
        Ok(())
    }
}

fn create_team_request(name: &str) -> CreateTeamRequest {
    create_team_request_with_session(name, &format!("session-{}", uuid::Uuid::new_v4().simple()))
}

fn create_team_request_with_session(name: &str, leader_session_id: &str) -> CreateTeamRequest {
    CreateTeamRequest {
        requested_name: name.to_string(),
        leader_agent_id: None,
        leader_session_id: leader_session_id.to_string(),
        cwd: std::path::PathBuf::from("/tmp"),
        allowed_paths: Vec::new(),
        leader_model: Some("test-model".to_string()),
        task_list_router: Some(Arc::new(TestTaskListRouter)),
    }
}

async fn create_team(handle: &SwarmAgentHandle, name: &str) -> CreateTeamResult {
    handle.create_team(create_team_request(name)).await.unwrap()
}

#[tokio::test]
async fn test_interrupt_teammate_current_work_cancels_task_token_only() {
    let registry = Arc::new(TestAgentTaskRegistry::default());
    let handle = create_test_handle_with_registry(
        registry.clone() as coco_tool_runtime::AgentTaskRegistryRef
    );
    let agent_id = "worker@test";
    let cancel = tokio_util::sync::CancellationToken::new();
    let observed = cancel.clone();
    registry
        .register_teammate_task(coco_tool_runtime::TeammateTaskRegistration::new(
            "worker",
            "test",
            coco_types::BackendType::InProcess,
            None,
            "p".to_string(),
            tokio_util::sync::CancellationToken::new(),
        ))
        .await;
    registry
        .set_teammate_current_work_cancel(agent_id, Some(cancel))
        .await;

    assert!(
        handle
            .interrupt_teammate_current_work(agent_id)
            .await
            .unwrap()
    );
    assert!(observed.is_cancelled());
    assert!(
        handle
            .interrupt_teammate_current_work("missing@test")
            .await
            .is_err(),
        "unknown teammate should surface an error"
    );
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

/// A minimal `messages` vec that yields a non-empty handoff transcript
/// so the transcript-gate in `classify_handoff_inline` runs the
/// classifier. TS parity (`agentToolUtils.ts:411-412`): classification
/// gates on a non-empty transcript, not on agent type or tool count.
fn messages_with_transcript() -> Vec<Arc<coco_types::messages::Message>> {
    use coco_types::messages::{Message, UserMessage};
    vec![Arc::new(Message::User(UserMessage {
        message: coco_types::LlmMessage::user_text("did some work"),
        uuid: uuid::Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    }))]
}

#[tokio::test]
async fn test_classifier_skips_on_empty_transcript() {
    // An empty transcript skips classification (TS `if (!agentTranscript)
    // return null`). #113: this is NOT a read-only exemption — read-only
    // agents WITH a transcript are now classified (see test below).
    let mut handle = create_test_handle();
    handle.set_side_query(Arc::new(StubSideQuery {
        responses: tokio::sync::Mutex::new(Vec::new()), // would error if called
    }));
    let qr = coco_tool_runtime::AgentQueryResult {
        response_text: Some("explored result".into()),
        messages: Vec::new(), // empty transcript
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
async fn test_classifier_runs_for_read_only_agent_with_transcript() {
    // #113: TS does not exempt read-only agents. With a non-empty
    // transcript, `Explore` is classified like any other agent — a
    // BLOCKED verdict surfaces the SECURITY payload.
    let mut handle = create_test_handle();
    handle.set_side_query(Arc::new(StubSideQuery {
        responses: tokio::sync::Mutex::new(vec![
            "VERDICT: BLOCKED\nREASON: wrote outside scope".into(),
            "VERDICT: REVIEW".into(),
        ]),
    }));
    let qr = coco_tool_runtime::AgentQueryResult {
        response_text: Some("explored result".into()),
        messages: messages_with_transcript(),
        turns: 1,
        input_tokens: 50,
        output_tokens: 25,
        tool_use_count: 0, // zero tools — still classified (gate is transcript)
        cancelled: false,
    };
    let out = handle
        .classify_handoff_if_needed("Explore", &qr)
        .await
        .expect("classifier returned None");
    assert!(out.starts_with("SECURITY"), "got: {out}");
}

#[tokio::test]
async fn test_classifier_unavailable_prepends_warning() {
    // #120: when the classifier errors (unavailable), fail open but
    // prepend the UNAVAILABLE_WARNING to the sub-agent's output so the
    // parent verifies the work (TS agentToolUtils.ts:464-469).
    let mut handle = create_test_handle();
    handle.set_side_query(Arc::new(StubSideQuery {
        responses: tokio::sync::Mutex::new(Vec::new()), // empty → query errors
    }));
    let qr = coco_tool_runtime::AgentQueryResult {
        response_text: Some("partial work".into()),
        messages: messages_with_transcript(),
        turns: 1,
        input_tokens: 50,
        output_tokens: 25,
        tool_use_count: 2,
        cancelled: false,
    };
    let out = handle
        .classify_handoff_if_needed("general-purpose", &qr)
        .await
        .expect("classifier returned None");
    assert!(
        out.starts_with("Note: The safety classifier was unavailable"),
        "got: {out}"
    );
    assert!(
        out.contains("partial work"),
        "original output preserved: {out}"
    );
}

#[tokio::test]
async fn test_classifier_short_circuits_on_stage1_safe() {
    // Stage 1 SAFE → no stage 2 call. Only one canned response is
    // needed; if the classifier wrongly proceeds to stage 2 the test
    // would fail at the empty pop().
    let mut handle = create_test_handle();
    handle.set_side_query(Arc::new(StubSideQuery {
        responses: tokio::sync::Mutex::new(vec!["SAFE".into()]),
    }));
    let qr = coco_tool_runtime::AgentQueryResult {
        response_text: Some("clean output".into()),
        messages: messages_with_transcript(),
        turns: 1,
        input_tokens: 50,
        output_tokens: 25,
        tool_use_count: 3,
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
        messages: messages_with_transcript(),
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

    let handle = create_test_handle();

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
            policy: coco_config::FallbackPolicy::default(),
        },
    );
    handle.set_runtime_config(Arc::new(runtime));
    assert_eq!(handle.current_main_model_id().unwrap(), "claude-opus-4-7");
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
async fn test_spawn_subagent_sync_drains_stream_events_to_task_registry() {
    use async_trait::async_trait;
    use coco_tool_runtime::AgentQueryConfig;
    use coco_tool_runtime::AgentQueryEngine;
    use coco_tool_runtime::AgentQueryResult;

    struct StreamingEngine;
    #[async_trait]
    impl AgentQueryEngine for StreamingEngine {
        async fn execute_query(
            &self,
            _prompt: &str,
            config: AgentQueryConfig,
        ) -> Result<AgentQueryResult, coco_error::BoxedError> {
            let tx = config.event_tx.expect("foreground task event sink");
            tx.send(coco_types::CoreEvent::Stream(
                coco_types::AgentStreamEvent::TextDelta {
                    turn_id: "turn_1".into(),
                    delta: "live output".into(),
                },
            ))
            .await
            .expect("event receiver active");
            tx.send(coco_types::CoreEvent::Stream(
                coco_types::AgentStreamEvent::ToolUseStarted {
                    call_id: "toolu_1".into(),
                    name: "Read".into(),
                    batch_id: None,
                },
            ))
            .await
            .expect("event receiver active");
            Ok(AgentQueryResult {
                response_text: Some("child result".into()),
                messages: Vec::new(),
                turns: 1,
                input_tokens: 0,
                output_tokens: 0,
                tool_use_count: 1,
                cancelled: false,
            })
        }
    }

    let registry = Arc::new(TestAgentTaskRegistry::default());
    let mut handle = create_test_handle_with_registry(
        registry.clone() as coco_tool_runtime::AgentTaskRegistryRef
    );
    handle.set_execution_engine(Arc::new(StreamingEngine));

    let response = handle
        .spawn_agent(AgentSpawnRequest {
            prompt: "do work".into(),
            subagent_type: Some("Explore".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(response.status, AgentSpawnStatus::Completed);

    let task_id = response.agent_id.expect("task id");
    for _ in 0..50 {
        if registry
            .outputs
            .lock()
            .expect("outputs lock")
            .get(&task_id)
            .is_some_and(|text| text.contains("live output"))
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("foreground stream output was not drained into task registry");
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

/// W6.2 full: when `signal_detach` fires while a sync agent's
/// engine is still running, the spawn caller must return
/// `AsyncLaunched` immediately while the engine task keeps running
/// in the background and eventually pushes a `<task-notification>`
/// envelope via `mark_completed`. This is the TS-parity "detach but
/// keep running" behavior — superior to the W6.2-half behavior
/// (which used to terminate the engine on detach).
///
/// Uses an inline mock `AgentTaskRegistry` instead of the real
/// `coco_cli::TaskRuntime` because coordinator can't depend on
/// `coco-cli` (one-way layer rule).
#[cfg(not(windows))]
#[tokio::test]
async fn test_spawn_subagent_sync_detach_keeps_engine_running() {
    use async_trait::async_trait;
    use coco_tool_runtime::AgentCompletionPayload;
    use coco_tool_runtime::AgentQueryConfig;
    use coco_tool_runtime::AgentQueryEngine;
    use coco_tool_runtime::AgentQueryResult;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    /// Minimal in-memory mock of `AgentTaskRegistry` for this test.
    /// Tracks `mark_completed` invocations so the test can verify the
    /// engine task actually finalized after detach.
    struct MockRegistry {
        detach: Arc<tokio::sync::Notify>,
        completed: Arc<tokio::sync::Notify>,
        mark_completed_called: Arc<AtomicBool>,
        task_id: std::sync::Mutex<Option<String>>,
    }

    #[async_trait]
    impl coco_tool_runtime::TaskHandle for MockRegistry {
        async fn register_agent_task(
            &self,
            _description: &str,
            _tool_use_id: Option<&str>,
            _invoking_agent_id: Option<&str>,
            _cancel: tokio_util::sync::CancellationToken,
            _registration: coco_tool_runtime::AgentRegistration,
        ) -> String {
            let id = "a0123456789abcdef".to_string();
            *self.task_id.lock().unwrap() = Some(id.clone());
            id
        }
        async fn register_agent_task_with_id(
            &self,
            task_id: String,
            _description: &str,
            _tool_use_id: Option<&str>,
            _invoking_agent_id: Option<&str>,
            _cancel: tokio_util::sync::CancellationToken,
            _registration: coco_tool_runtime::AgentRegistration,
        ) -> String {
            *self.task_id.lock().unwrap() = Some(task_id.clone());
            task_id
        }
        async fn append_output(&self, _: &str, _: &str) {}
        async fn set_progress_summary(&self, _: &str, _: String) {}
        async fn set_progress(&self, _: &str, _: coco_types::TaskProgress) {}
        async fn mark_completed(&self, _: &str, _: AgentCompletionPayload) {
            self.mark_completed_called.store(true, Ordering::SeqCst);
            self.completed.notify_one();
        }
        async fn mark_failed(&self, _: &str, _: &str) {
            self.completed.notify_one();
        }
        async fn complete_silent(&self, _: &str, _: bool) {}
        async fn read_output(&self, _: &str) -> String {
            String::new()
        }
        async fn task_state(&self, _: &str) -> Option<coco_types::TaskStateBase> {
            None
        }
        async fn is_terminal(&self, _: &str) -> bool {
            false
        }
        async fn register_dream_task(
            &self,
            _description: &str,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> String {
            "d_unused".into()
        }
        async fn detach_handle(&self, _: &str) -> Option<Arc<tokio::sync::Notify>> {
            Some(self.detach.clone())
        }
    }

    // Engine that blocks until the test releases it.
    struct GatedEngine {
        release: Arc<tokio::sync::Notify>,
    }
    #[async_trait]
    impl AgentQueryEngine for GatedEngine {
        async fn execute_query(
            &self,
            _prompt: &str,
            _config: AgentQueryConfig,
        ) -> Result<AgentQueryResult, coco_error::BoxedError> {
            self.release.notified().await;
            Ok(AgentQueryResult {
                response_text: Some("detached result".into()),
                messages: Vec::new(),
                turns: 1,
                input_tokens: 7,
                output_tokens: 11,
                tool_use_count: 0,
                cancelled: false,
            })
        }
    }

    let release = Arc::new(tokio::sync::Notify::new());
    let detach = Arc::new(tokio::sync::Notify::new());
    let completed = Arc::new(tokio::sync::Notify::new());
    let mark_completed_called = Arc::new(AtomicBool::new(false));
    let registry = Arc::new(MockRegistry {
        detach: detach.clone(),
        completed: completed.clone(),
        mark_completed_called: mark_completed_called.clone(),
        task_id: std::sync::Mutex::new(None),
    });

    let mut handle = create_test_handle_with_registry(
        registry.clone() as coco_tool_runtime::AgentTaskRegistryRef
    );
    handle.set_execution_engine(Arc::new(GatedEngine {
        release: release.clone(),
    }));

    let request = AgentSpawnRequest {
        prompt: "long work".into(),
        subagent_type: Some("general-purpose".into()),
        ..Default::default()
    };

    // Spawn agent in another task so we can fire detach while it's
    // waiting for the engine to complete.
    let handle_arc = Arc::new(handle);
    let handle_clone = handle_arc.clone();
    let spawn_handle = tokio::spawn(async move { handle_clone.spawn_agent(request).await });

    // Give the spawn time to reach the detach-race select! arm.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Fire detach via the same Notify the engine task subscribed to.
    detach.notify_one();

    // Spawn caller returns AsyncLaunched (engine still running).
    let response = tokio::time::timeout(std::time::Duration::from_secs(2), spawn_handle)
        .await
        .expect("spawn must return within 2s")
        .expect("join")
        .expect("spawn must succeed");
    assert_eq!(
        response.status,
        AgentSpawnStatus::AsyncLaunched,
        "detach must return AsyncLaunched, not Completed; got {:?}",
        response.status
    );

    // Verify engine is still running by checking mark_completed hasn't fired.
    assert!(
        !mark_completed_called.load(Ordering::SeqCst),
        "mark_completed must NOT have fired before engine completes"
    );

    // Now release the engine.
    release.notify_one();

    // Engine task should call mark_completed (since it was detached).
    tokio::time::timeout(std::time::Duration::from_secs(2), completed.notified())
        .await
        .expect("engine task must call mark_completed after release");

    assert!(
        mark_completed_called.load(Ordering::SeqCst),
        "detached path must route through mark_completed (push notification), \
         not complete_silent"
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
    let team_name = format!("agentteam-test-{}", uuid::Uuid::new_v4().simple());
    let _ = crate::team_file::cleanup_team_directories(&team_name);
    let handle = create_test_handle();
    create_team(&handle, &team_name).await;
    let request = AgentSpawnRequest {
        prompt: "Help me".to_string(),
        name: Some("researcher".to_string()),
        team_name: Some(team_name.clone()),
        ..Default::default()
    };

    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::TeammateSpawned);
    assert!(response.agent_id.is_some());
    assert!(
        response
            .agent_id
            .unwrap()
            .contains(&format!("researcher@{team_name}"))
    );
    let _ = handle.delete_team().await;
}

#[tokio::test]
async fn test_spawn_teammate_drives_engine_when_installed() {
    // Gap C regression: pre-fix, `spawn_teammate` called only
    // `runner.register_agent(...)` and never started the runner-loop.
    // Teammates registered as Running but no LLM turn ever fired.
    // This test installs a teammate execution engine and asserts that
    // (a) spawn returns TeammateSpawned, (b) the engine's run_query is
    // invoked at least once via the runner-loop kickoff, (c) the
    // teammate has a TaskManager-backed task projection.
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
    let team_name = format!("agentteam-test-{}", uuid::Uuid::new_v4().simple());
    let _ = crate::team_file::cleanup_team_directories(&team_name);
    let registry = Arc::new(TestAgentTaskRegistry::default());
    let mut handle = create_test_handle_with_registry(
        registry.clone() as coco_tool_runtime::AgentTaskRegistryRef
    );
    handle.set_teammate_execution_engine(Arc::new(CountingTeammateEngine {
        calls: calls.clone(),
    }));
    create_team(&handle, &team_name).await;

    let response = handle
        .spawn_agent(AgentSpawnRequest {
            prompt: "do work".into(),
            name: Some("worker".into()),
            team_name: Some(team_name.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(response.status, AgentSpawnStatus::TeammateSpawned);
    let agent_id = response.agent_id.expect("agent_id present");

    // Task projection is created at spawn time even if the runner-loop
    // hasn't ticked yet. Without Gap C there was no running-task row.
    let mirror = registry.teammate_task_state(&agent_id).await;
    assert!(
        mirror.is_some(),
        "teammate task projection must exist after spawn"
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
    let _ = handle.delete_team().await;
}

#[tokio::test]
async fn test_query_team_agent_without_local_task_reports_not_controllable() {
    let team_name = format!("agentteam-test-{}", uuid::Uuid::new_v4().simple());
    let _ = crate::team_file::cleanup_team_directories(&team_name);
    let handle = create_test_handle();
    create_team(&handle, &team_name).await;

    let reservation = handle
        .roster_store
        .reserve_member(SpawnMemberRequest {
            desired_name: "remote-worker".to_string(),
            team_name: team_name.clone(),
            agent_type: None,
            model: None,
            prompt: "work".to_string(),
            color: None,
            plan_mode_required: false,
            cwd: "/tmp".to_string(),
            worktree_path: None,
            mode: None,
        })
        .await
        .unwrap();
    handle
        .roster_store
        .commit_member(CommitMemberRequest {
            team_name: team_name.clone(),
            agent_id: reservation.agent_id.clone(),
            backend_type: crate::types::BackendType::Tmux,
            pane_id: Some("%1".to_string()),
            session_id: Some("other-process".to_string()),
        })
        .await
        .unwrap();

    let err = handle
        .query_agent_status(&reservation.agent_id)
        .await
        .unwrap_err();
    assert!(
        err.contains("not locally controllable"),
        "cross-process roster-only teammate must not fabricate status: {err}"
    );
    let _ = crate::team_file::cleanup_team_directories(&team_name);
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
    let team_name = format!("agentteam-test-{}", uuid::Uuid::new_v4().simple());
    let _ = crate::team_file::cleanup_team_directories(&team_name);
    let mut handle = create_test_handle();
    handle.set_teammate_execution_engine(Arc::new(CapturingEngine {
        captured: captured.clone(),
    }));
    create_team(&handle, &team_name).await;
    handle
        .set_teammate_base_system_prompt("LEADER PROMPT BODY".into())
        .await;

    handle
        .spawn_agent(AgentSpawnRequest {
            prompt: "do work".into(),
            name: Some("worker".into()),
            team_name: Some(team_name.clone()),
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
    let _ = handle.delete_team().await;
}

#[tokio::test]
async fn test_spawn_teammate_forwards_runner_query_options() {
    use crate::runner_loop::{
        AgentExecutionEngine, AgentQueryConfig as RunnerCfg, AgentQueryResult as RunnerResult,
    };

    struct CapturingEngine {
        captured: Arc<tokio::sync::Mutex<Option<RunnerCfg>>>,
    }

    #[async_trait::async_trait]
    impl AgentExecutionEngine for CapturingEngine {
        async fn run_query(&self, _prompt: &str, config: RunnerCfg) -> crate::Result<RunnerResult> {
            *self.captured.lock().await = Some(config);
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
    let team_name = format!("agentteam-test-{}", uuid::Uuid::new_v4().simple());
    let _ = crate::team_file::cleanup_team_directories(&team_name);
    let mut handle = create_test_handle();
    handle.set_teammate_execution_engine(Arc::new(CapturingEngine {
        captured: captured.clone(),
    }));
    create_team(&handle, &team_name).await;

    // Static effort lives on `AgentDefinition.effort`; the coordinator
    // All static knobs (effort / use_exact_tools / mcp_servers /
    // disallowed_tools / max_turns / initial_prompt) live on
    // `AgentDefinition` and are read via `request.definition` at
    // RunnerConfig assembly time. Per-spawn override slots on the
    // request struct are gone (audit pass: dead-field cleanup).
    let def = std::sync::Arc::new(coco_types::AgentDefinition {
        agent_type: coco_types::AgentTypeId::Custom("worker".into()),
        name: "worker".into(),
        effort: Some(coco_types::ReasoningEffort::High),
        use_exact_tools: true,
        mcp_servers: vec![coco_types::AgentMcpServerSpec::Name("github".into())],
        disallowed_tools: vec!["Bash".into()],
        ..Default::default()
    });
    handle
        .spawn_agent(AgentSpawnRequest {
            prompt: "do work".into(),
            name: Some("worker".into()),
            team_name: Some(team_name.clone()),
            definition: Some(def),
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
    assert_eq!(observed.effort, Some(coco_types::ReasoningEffort::High));
    assert!(observed.use_exact_tools);
    assert_eq!(observed.mcp_servers, vec!["github"]);
    assert_eq!(observed.disallowed_tools, vec!["Bash"]);
    assert_eq!(observed.model_role, Some(coco_types::ModelRole::Main));
    let _ = handle.delete_team().await;
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
    let team_name = format!("agentteam-test-{}", uuid::Uuid::new_v4().simple());
    let _ = crate::team_file::cleanup_team_directories(&team_name);

    // Create
    let result = handle.create_team(create_team_request(&team_name)).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().team_name, team_name);

    // Delete
    let result = handle.delete_team().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_team_lifecycle_writes_roster_and_blocks_delete_while_active() {
    let team_name = format!("agentteam-test-{}", uuid::Uuid::new_v4().simple());
    let _ = crate::team_file::cleanup_team_directories(&team_name);

    let handle = create_test_handle();
    let created = create_team(&handle, &team_name).await;
    assert_eq!(created.team_name, team_name);

    let duplicate = handle
        .create_team(create_team_request("another-team"))
        .await;
    assert!(
        duplicate
            .expect_err("second active team must be rejected")
            .contains("already has active team"),
    );

    let team_file = crate::team_file::read_team_file(&team_name)
        .unwrap()
        .expect("team file must exist after TeamCreate");
    assert_eq!(team_file.name, team_name);
    assert_eq!(team_file.members.len(), 1);
    assert_eq!(team_file.members[0].name, crate::constants::TEAM_LEAD_NAME);
    assert_eq!(team_file.lead_agent_id, format!("team-lead@{team_name}"));

    let spawned = handle
        .spawn_agent(AgentSpawnRequest {
            prompt: "inspect the repo".into(),
            name: Some("researcher".into()),
            team_name: Some(team_name.clone()),
            session_id: "session-1".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(spawned.status, AgentSpawnStatus::TeammateSpawned);
    let agent_id = spawned.agent_id.expect("spawned teammate id");

    let disk_file = crate::team_file::read_team_file(&team_name)
        .unwrap()
        .expect("team file must still exist");
    let disk_member = disk_file
        .members
        .iter()
        .find(|m| m.name == "researcher")
        .expect("spawned teammate must be persisted to disk");
    assert_eq!(disk_member.agent_id, agent_id);
    assert_eq!(
        disk_member.backend_type,
        Some(crate::types::BackendType::InProcess)
    );
    assert_eq!(disk_member.session_id.as_deref(), None);

    let manager_file = handle
        .team_manager
        .read()
        .await
        .as_ref()
        .expect("team manager installed")
        .team_file()
        .await;
    assert!(
        manager_file.members.iter().any(|m| m.name == "researcher"),
        "in-memory roster must mirror disk roster"
    );

    let statuses = crate::discovery::get_teammate_statuses(&team_name);
    assert!(
        statuses.iter().any(|s| s.name == "researcher"),
        "discovery must see the spawned teammate; got {statuses:?}"
    );

    let broadcast = handle.send_message("*", "status please").await.unwrap();
    assert!(broadcast.contains("1 recipients"));
    let mailbox = crate::mailbox::read_mailbox("researcher", &team_name).unwrap();
    assert!(
        mailbox.iter().any(|m| m.text == "status please"),
        "broadcast must write to teammate mailbox"
    );

    let delete = handle.delete_team().await;
    assert!(
        delete
            .expect_err("delete must block while non-lead member is active")
            .contains("active members: researcher")
    );

    handle
        .roster_store
        .rollback_member(&team_name, &agent_id)
        .await
        .unwrap();
    let deleted = handle.delete_team().await.unwrap();
    assert!(deleted.contains(&team_name));
    assert!(
        !crate::team_file::get_team_dir(&team_name).exists(),
        "team directory must be cleaned up"
    );
}

#[tokio::test]
async fn test_create_team_rejects_existing_team_for_same_leader_session() {
    let session_id = format!("session-{}", uuid::Uuid::new_v4().simple());
    let first_name = format!("agentteam-test-{}", uuid::Uuid::new_v4().simple());
    let second_name = format!("agentteam-test-{}", uuid::Uuid::new_v4().simple());
    let _ = crate::team_file::cleanup_team_directories(&first_name);
    let _ = crate::team_file::cleanup_team_directories(&second_name);

    let first_handle = create_test_handle();
    first_handle
        .create_team(create_team_request_with_session(&first_name, &session_id))
        .await
        .expect("first team create succeeds");

    let second_handle = create_test_handle();
    let duplicate = second_handle
        .create_team(create_team_request_with_session(&second_name, &session_id))
        .await;
    assert!(
        duplicate
            .expect_err("same leader session must not create another team")
            .contains("leader session already has active team"),
    );

    let _ = first_handle.delete_team().await;
    let _ = crate::team_file::cleanup_team_directories(&first_name);
    let _ = crate::team_file::cleanup_team_directories(&second_name);
}

#[tokio::test]
async fn test_create_team_uses_unique_name_when_requested_dir_exists() {
    let base = format!("agentteam-test-{}", uuid::Uuid::new_v4().simple());
    let expected = format!("{base}-2");
    let _ = crate::team_file::cleanup_team_directories(&base);
    let _ = crate::team_file::cleanup_team_directories(&expected);
    std::fs::create_dir_all(crate::team_file::get_team_dir(&base)).unwrap();

    let handle = create_test_handle();
    let created = handle
        .create_team(create_team_request(&base))
        .await
        .unwrap();
    assert_eq!(created.team_name, expected);
    assert!(
        crate::team_file::read_team_file(&expected)
            .unwrap()
            .is_some(),
        "unique team file should be written under {expected}"
    );

    let _ = handle.delete_team().await;
    let _ = crate::team_file::cleanup_team_directories(&base);
    let _ = crate::team_file::cleanup_team_directories(&expected);
}

#[tokio::test]
async fn test_query_unknown_agent() {
    let handle = create_test_handle();
    let result = handle.query_agent_status("nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_query_local_agent_status_uses_registry_without_team_fallback() {
    use async_trait::async_trait;
    use coco_tool_runtime::AgentCompletionPayload;

    struct Registry {
        state: coco_types::TaskStateBase,
    }

    #[async_trait]
    impl coco_tool_runtime::TaskHandle for Registry {
        async fn register_agent_task(
            &self,
            _: &str,
            _: Option<&str>,
            _: Option<&str>,
            _: tokio_util::sync::CancellationToken,
            _: coco_tool_runtime::AgentRegistration,
        ) -> String {
            self.state.id.clone()
        }
        async fn register_agent_task_with_id(
            &self,
            task_id: String,
            _: &str,
            _: Option<&str>,
            _: Option<&str>,
            _: tokio_util::sync::CancellationToken,
            _: coco_tool_runtime::AgentRegistration,
        ) -> String {
            task_id
        }
        async fn append_output(&self, _: &str, _: &str) {}
        async fn set_progress_summary(&self, _: &str, _: String) {}
        async fn set_progress(&self, _: &str, _: coco_types::TaskProgress) {}
        async fn mark_completed(&self, _: &str, _: AgentCompletionPayload) {}
        async fn mark_failed(&self, _: &str, _: &str) {}
        async fn complete_silent(&self, _: &str, _: bool) {}
        async fn register_dream_task(
            &self,
            _: &str,
            _: tokio_util::sync::CancellationToken,
        ) -> String {
            "dtest".into()
        }
        async fn detach_handle(&self, _: &str) -> Option<Arc<tokio::sync::Notify>> {
            None
        }
        async fn read_output(&self, _: &str) -> String {
            "registry output".into()
        }
        async fn task_state(&self, task_id: &str) -> Option<coco_types::TaskStateBase> {
            (task_id == self.state.id).then(|| self.state.clone())
        }
        async fn is_terminal(&self, _: &str) -> bool {
            true
        }
    }

    let agent_id = "a0123456789abcdef".to_string();
    let handle = create_test_handle_with_registry(Arc::new(Registry {
        state: coco_types::TaskStateBase {
            id: agent_id.clone(),
            status: coco_types::TaskStatus::Completed,
            notified: true,
            description: "registry task".into(),
            tool_use_id: None,
            start_time: 10,
            end_time: Some(25),
            total_paused_ms: None,
            output_file: Some("/tmp/agent.out".into()),
            output_offset: 0,
            extras: coco_types::TaskExtras::bg_agent_default(),
        },
    })
        as coco_tool_runtime::AgentTaskRegistryRef);
    let status = handle.query_agent_status(&agent_id).await.unwrap();
    assert_eq!(status.status, AgentSpawnStatus::Completed);
    assert_eq!(status.result.as_deref(), Some("registry output"));
    assert_eq!(status.duration_ms, 15);

    let output = handle.get_agent_output(&agent_id).await.unwrap();
    assert_eq!(output, "registry output");
}

#[tokio::test]
async fn test_spawn_subagent_validation_failure_does_not_leak_state() {
    // Regression: spawn_subagent used SwarmAgentHandle's agent list as
    // a LocalAgent fallback store. Validation failures must not leave
    // LocalAgent state in the team-agent container.
    let registry = Arc::new(TestAgentTaskRegistry::default());
    let handle = create_test_handle_with_registry(
        registry.clone() as coco_tool_runtime::AgentTaskRegistryRef
    );
    let request = AgentSpawnRequest {
        prompt: "isolated work".into(),
        // Worktree without a manager — first gate fails.
        isolation: Some("worktree".into()),
        ..Default::default()
    };
    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::Failed);
    let agents = registry.states.lock().expect("states lock");
    assert!(
        agents.is_empty(),
        "validation failure must not leave a dangling running-task entry; got {agents:?}",
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
        captured: tokio::sync::Mutex<Option<Vec<std::sync::Arc<coco_messages::Message>>>>,
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

    // Build a typed ToolResultMessage that the engine stub can observe.
    let tool_result_msg = std::sync::Arc::new(coco_messages::create_tool_result_message(
        "abc",
        "Bash",
        "Bash".parse().unwrap(),
        "REAL TOOL OUTPUT - must survive",
        false,
    ));
    let parent_messages = vec![tool_result_msg];
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

/// G1 regression: fork-mode user turn must be wrapped in
/// `<fork-boilerplate>...</fork-boilerplate>` + `Your directive: ` so
/// the worker receives its rules AND a future
/// `is_in_fork_child(parent_messages)` scan can detect recursion.
///
/// Pre-fix, spawn.rs called `build_fork_context` but threw away
/// `ctx.directive` and sent `request.prompt` verbatim — recursion
/// guard could never trigger.
///
/// TS parity: `forkSubagent.ts::buildChildMessage`.
#[tokio::test]
async fn test_spawn_subagent_fork_mode_wraps_directive_with_boilerplate() {
    use async_trait::async_trait;
    use coco_tool_runtime::AgentQueryConfig;
    use coco_tool_runtime::AgentQueryEngine;
    use coco_tool_runtime::AgentQueryResult;

    struct CapturingEngine {
        captured_prompt: tokio::sync::Mutex<Option<String>>,
        captured_system: tokio::sync::Mutex<Option<String>>,
        captured_messages: tokio::sync::Mutex<Option<Vec<std::sync::Arc<coco_messages::Message>>>>,
    }
    #[async_trait]
    impl AgentQueryEngine for CapturingEngine {
        async fn execute_query(
            &self,
            prompt: &str,
            config: AgentQueryConfig,
        ) -> Result<AgentQueryResult, coco_error::BoxedError> {
            *self.captured_prompt.lock().await = Some(prompt.to_string());
            *self.captured_system.lock().await = Some(config.system_prompt.clone());
            *self.captured_messages.lock().await = Some(config.fork_context_messages.clone());
            Ok(AgentQueryResult {
                response_text: Some("done".into()),
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
        captured_system: tokio::sync::Mutex::new(None),
        captured_messages: tokio::sync::Mutex::new(None),
    });
    let mut handle = create_test_handle();
    handle.set_execution_engine(captured.clone());
    handle.set_hook_registry(Arc::new(coco_hooks::HookRegistry::new()));

    let parent_messages = vec![std::sync::Arc::new(
        coco_messages::create_tool_result_message(
            "tu1",
            "Bash",
            "Bash".parse().unwrap(),
            "noisy parent output",
            false,
        ),
    )];
    let parent_snapshot = std::sync::Arc::new(coco_types::SubagentRuntimeSnapshot {
        provider: "anthropic".into(),
        api: coco_types::ProviderApi::Anthropic,
        api_model_name: "claude-opus-4-7".into(),
        base_url: "https://api.anthropic.com".into(),
        wire_api: None,
    });

    let request = AgentSpawnRequest {
        prompt: "Research how Foo works".into(),
        // Fork mode is only chosen by AgentTool when no subagent_type
        // is supplied; mirror that here so the runner takes the Fork
        // branch.
        subagent_type: None,
        spawn_mode: coco_tool_runtime::SpawnMode::Fork {
            rendered_system_prompt: "PARENT SYSTEM PROMPT".into(),
            parent_messages: parent_messages.clone(),
            parent_snapshot,
        },
        ..Default::default()
    };
    let response = handle.spawn_agent(request).await.unwrap();
    assert_eq!(response.status, AgentSpawnStatus::Completed);

    // Worker user turn carries the boilerplate + rules + directive.
    let observed_prompt = captured.captured_prompt.lock().await.clone().unwrap();
    assert!(
        observed_prompt.contains(&format!("<{}>", coco_subagent::FORK_BOILERPLATE_TAG)),
        "fork directive must be wrapped in `<{}>`; got: {observed_prompt}",
        coco_subagent::FORK_BOILERPLATE_TAG,
    );
    assert!(
        observed_prompt.contains(&format!("</{}>", coco_subagent::FORK_BOILERPLATE_TAG)),
        "fork directive must close the `</{}>` tag; got: {observed_prompt}",
        coco_subagent::FORK_BOILERPLATE_TAG,
    );
    assert!(
        observed_prompt.contains(coco_subagent::FORK_DIRECTIVE_PREFIX),
        "fork prompt must include the `Your directive: ` prefix; got: {observed_prompt}",
    );
    assert!(
        observed_prompt.contains("Research how Foo works"),
        "fork prompt must end with the original directive text; got: {observed_prompt}",
    );

    // Recursion guard precondition: the wrapped child message must be
    // detectable by `is_in_fork_child` once it lands in history. The
    // runner injects only via the new user turn (not into parent
    // messages), so we synthesize the user message a downstream turn
    // would see and assert detection.
    let downstream_history = vec![std::sync::Arc::new(coco_messages::create_user_message(
        &observed_prompt,
    ))];
    assert!(
        coco_subagent::is_in_fork_child(&downstream_history),
        "is_in_fork_child must detect the wrapped directive — without this, fork-of-fork is silently allowed",
    );

    // Inherited history's `tool_result` blocks were rewritten to
    // FORK_PLACEHOLDER (build_fork_context contract).
    let observed_messages = captured.captured_messages.lock().await.clone().unwrap();
    let serialized = serde_json::to_string(&observed_messages).unwrap();
    assert!(
        serialized.contains(coco_subagent::FORK_PLACEHOLDER),
        "Fork must rewrite parent tool_results to FORK_PLACEHOLDER; got: {serialized}",
    );
    assert!(
        !serialized.contains("noisy parent output"),
        "Fork must scrub the original tool_result content; got: {serialized}",
    );

    // Pinned system prompt — verbatim from the snapshot.
    let observed_system = captured.captured_system.lock().await.clone().unwrap();
    assert_eq!(observed_system, "PARENT SYSTEM PROMPT");
}

/// The team's task-list directory under `config_home()/tasks`, matching
/// what `cleanup_team_directories` removes and what `TaskListStore::open`
/// materializes. Used by the delete-notify tests below.
fn team_tasks_dir(team_name: &str) -> std::path::PathBuf {
    let task_list_id = crate::types::sanitize_name(team_name);
    coco_config::global_config::config_home()
        .join("tasks")
        .join(coco_tasks::task_list::sanitize_path_component(
            &task_list_id,
        ))
}

#[tokio::test]
async fn test_delete_team_notifies_task_list_subscriber_on_success() {
    // TS parity: `cleanupTeamDirectories` fires `notifyTasksUpdated()`
    // inside the `rm(tasksDir)` `try`. After deleting the team (and its
    // task-list dir), a subscriber on the wired task-list store must
    // observe the change notification.
    let mut handle = create_test_handle();
    let team_name = format!("agentteam-notify-{}", uuid::Uuid::new_v4().simple());
    let _ = crate::team_file::cleanup_team_directories(&team_name);

    create_team(&handle, &team_name).await;

    // Wire a real disk-backed store at the same path cleanup removes, and
    // materialize the dir so the removal actually succeeds (→ notify).
    let tasks_root = coco_config::global_config::config_home().join("tasks");
    let task_list_id = crate::types::sanitize_name(&team_name);
    let store = coco_tasks::TaskListStore::open(&tasks_root, &task_list_id).unwrap();
    let tasks_dir = team_tasks_dir(&team_name);
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(tasks_dir.join("t.json"), "{}").unwrap();

    let mut rx = store.subscribe_changes();
    handle.set_task_list(store.clone() as TaskListHandleRef);

    let result = handle.delete_team().await;
    assert!(result.is_ok(), "delete_team failed: {result:?}");
    assert!(!tasks_dir.exists(), "task-list dir should be removed");

    assert!(
        rx.try_recv().is_ok(),
        "subscriber must observe a tasks-changed notification after successful delete",
    );

    let _ = crate::team_file::cleanup_team_directories(&team_name);
}

/// Whether the process is actually bound by Unix directory permissions.
/// Root / `CAP_DAC_OVERRIDE` bypass them, which would make the 0o500-based
/// removal-failure simulation below succeed instead of fail. Probe the real
/// behavior rather than guess at the uid (portable across Linux/macOS).
#[cfg(unix)]
fn unix_dir_perms_enforced() -> bool {
    use std::os::unix::fs::PermissionsExt;
    let dir =
        std::env::temp_dir().join(format!("coco-perm-probe-{}", uuid::Uuid::new_v4().simple()));
    let inner = dir.join("inner");
    if std::fs::create_dir_all(&inner).is_err() {
        return true;
    }
    let _ = std::fs::write(inner.join("f"), "x");
    let _ = std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o500));
    let enforced = std::fs::remove_dir_all(&inner).is_err();
    let _ = std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::remove_dir_all(&dir);
    enforced
}

#[cfg(unix)]
#[tokio::test]
async fn test_delete_team_does_not_notify_when_tasks_dir_removal_fails() {
    // TS parity: the `catch` path of `rm(tasksDir)` does NOT notify. Force
    // the removal to fail (non-empty dir with no write permission → its
    // child can't be unlinked) and assert no notification fires.
    use std::os::unix::fs::PermissionsExt;

    // Privileged environments (root / CAP_DAC_OVERRIDE) bypass the 0o500
    // permission this simulation relies on, so the removal would succeed and
    // the premise can't hold — skip rather than assert a false failure.
    if !unix_dir_perms_enforced() {
        return;
    }

    let mut handle = create_test_handle();
    let team_name = format!("agentteam-nonotify-{}", uuid::Uuid::new_v4().simple());
    let _ = crate::team_file::cleanup_team_directories(&team_name);

    create_team(&handle, &team_name).await;

    let tasks_root = coco_config::global_config::config_home().join("tasks");
    let task_list_id = crate::types::sanitize_name(&team_name);
    let store = coco_tasks::TaskListStore::open(&tasks_root, &task_list_id).unwrap();

    // Materialize the dir with a child, then strip write perm so
    // `remove_dir_all` fails unlinking the child.
    let tasks_dir = team_tasks_dir(&team_name);
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(tasks_dir.join("t.json"), "{}").unwrap();
    std::fs::set_permissions(&tasks_dir, std::fs::Permissions::from_mode(0o500)).unwrap();

    let mut rx = store.subscribe_changes();
    handle.set_task_list(store.clone() as TaskListHandleRef);

    // delete_team swallows the tasks-dir failure (best-effort) and returns
    // Ok; the notification must NOT fire because the removal failed.
    let result = handle.delete_team().await;
    assert!(
        result.is_ok(),
        "delete_team should still succeed: {result:?}"
    );

    assert!(
        matches!(
            rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ),
        "no notification must fire when the tasks-dir removal fails",
    );

    // Restore perms so the dir can be cleaned up.
    let _ = std::fs::set_permissions(&tasks_dir, std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::remove_dir_all(&tasks_dir);
    let _ = crate::team_file::cleanup_team_directories(&team_name);
}
