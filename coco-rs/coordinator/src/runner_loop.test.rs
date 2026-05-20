use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::*;

// Pull moved helpers into scope so the existing test bodies don't
// need module-path qualification.
use crate::runner_loop_mailbox_permission::MailboxPermissionBridge;
use crate::runner_loop_wait::wait_for_plan_approval;

// ── Mock Engine ──

struct MockEngine {
    response: String,
}

#[async_trait]
impl AgentExecutionEngine for MockEngine {
    async fn run_query(
        &self,
        _prompt: &str,
        _config: AgentQueryConfig,
    ) -> crate::Result<AgentQueryResult> {
        Ok(AgentQueryResult {
            messages: vec![std::sync::Arc::new(
                coco_messages::create_assistant_message(
                    vec![coco_messages::AssistantContent::Text(
                        coco_messages::TextContent {
                            text: self.response.clone(),
                            provider_metadata: None,
                        },
                    )],
                    "test-model",
                    coco_types::TokenUsage::default(),
                ),
            )],
            token_count: 100,
            input_tokens: 60,
            output_tokens: 40,
            turns: 1,
            tool_use_count: 0,
            cancelled: false,
            response_text: Some(self.response.clone()),
        })
    }
}

struct ErrorEngine;

#[async_trait]
impl AgentExecutionEngine for ErrorEngine {
    async fn run_query(
        &self,
        _prompt: &str,
        _config: AgentQueryConfig,
    ) -> crate::Result<AgentQueryResult> {
        Err(crate::CoordinatorError::generic("API error: rate limited"))
    }
}

struct InterruptibleEngine {
    started: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    interrupted: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[async_trait]
impl AgentExecutionEngine for InterruptibleEngine {
    async fn run_query(
        &self,
        _prompt: &str,
        config: AgentQueryConfig,
    ) -> crate::Result<AgentQueryResult> {
        if let Some(tx) = self.started.lock().await.take() {
            let _ = tx.send(());
        }
        let cancel = config.cancel.expect("runner must pass per-turn cancel");
        cancel.cancelled().await;
        if let Some(tx) = self.interrupted.lock().await.take() {
            let _ = tx.send(());
        }
        Ok(AgentQueryResult {
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

struct LiveControlEngine {
    started: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    observed: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[async_trait]
impl AgentExecutionEngine for LiveControlEngine {
    async fn run_query(
        &self,
        _prompt: &str,
        config: AgentQueryConfig,
    ) -> crate::Result<AgentQueryResult> {
        if let Some(tx) = self.started.lock().await.take() {
            let _ = tx.send(());
        }
        let mode = config
            .live_permission_mode
            .expect("runner must pass live mode");
        let rules = config
            .live_permission_rules
            .expect("runner must pass live rules");
        let cancel = config.cancel.expect("runner must pass per-turn cancel");
        loop {
            let mode_seen = *mode.read().await == coco_types::PermissionMode::AcceptEdits;
            let rule_seen = rules
                .read()
                .await
                .iter()
                .any(|rule| rule.value.tool_pattern == "Edit");
            if mode_seen && rule_seen {
                if let Some(tx) = self.observed.lock().await.take() {
                    let _ = tx.send(());
                }
                return Ok(AgentQueryResult {
                    messages: Vec::new(),
                    token_count: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    turns: 1,
                    tool_use_count: 0,
                    cancelled: false,
                    response_text: Some("observed live control".into()),
                });
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    return Ok(AgentQueryResult {
                        messages: Vec::new(),
                        token_count: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        turns: 1,
                        tool_use_count: 0,
                        cancelled: true,
                        response_text: None,
                    });
                }
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
    }
}

fn make_identity() -> TeammateIdentity {
    TeammateIdentity {
        agent_id: "worker@test".into(),
        agent_name: "worker".into(),
        team_name: "test".into(),
        color: None,
        plan_mode_required: false,
    }
}

fn make_config(cancelled: Arc<AtomicBool>) -> InProcessRunnerConfig {
    InProcessRunnerConfig {
        identity: make_identity(),
        task_id: "task-1".into(),
        prompt: "Do something".into(),
        model: None,
        system_prompt: None,
        system_prompt_mode: SystemPromptMode::Default,
        allowed_tools: vec![],
        allow_permission_prompts: false,
        max_turns: Some(1),
        cancelled,
        auto_compact_threshold: 100_000,
        bypass_permissions_available: false,
        features: None,
        tool_overrides: None,
        parent_tool_filter: None,
        effort: None,
        use_exact_tools: false,
        mcp_servers: Vec::new(),
        disallowed_tools: Vec::new(),
        model_role: None,
        model_selection: coco_types::LlmModelSelection::InheritMain,
        task_list: None,
        roster_store: None,
        plan_mode_required: false,
        hooks: None,
        orchestration_ctx: None,
    }
}

fn make_task_state(identity: &TeammateIdentity) -> tokio::sync::RwLock<InProcessTeammateTaskState> {
    tokio::sync::RwLock::new(InProcessTeammateTaskState::new(
        "task-test".into(),
        identity.clone(),
        "prompt".into(),
    ))
}

#[tokio::test]
async fn test_run_with_immediate_cancel() {
    let cancelled = Arc::new(AtomicBool::new(true));
    let config = make_config(cancelled);
    let engine = MockEngine {
        response: "done".into(),
    };
    let task_state = tokio::sync::RwLock::new(InProcessTeammateTaskState::new(
        "task-1".into(),
        make_identity(),
        "test".into(),
    ));

    let result = run_in_process_teammate(config, &engine, &task_state).await;
    assert!(result.success);
    assert_eq!(result.turns, 0);
}

#[tokio::test]
async fn test_run_with_error_engine() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let config = make_config(cancelled);
    let engine = ErrorEngine;
    let task_state = tokio::sync::RwLock::new(InProcessTeammateTaskState::new(
        "task-1".into(),
        make_identity(),
        "test".into(),
    ));

    let result = run_in_process_teammate(config, &engine, &task_state).await;
    assert!(!result.success);
    assert!(
        result
            .error
            .as_ref()
            .is_some_and(|e| e.contains("rate limited"))
    );
}

#[tokio::test]
async fn test_run_single_turn_then_cancel() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = Arc::clone(&cancelled);

    let config = make_config(cancelled);
    let engine = MockEngine {
        response: "I did it".into(),
    };
    let task_state = tokio::sync::RwLock::new(InProcessTeammateTaskState::new(
        "task-1".into(),
        make_identity(),
        "test".into(),
    ));

    // Cancel after a short delay (before wait_for_next_prompt blocks)
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancelled_clone.store(true, Ordering::Relaxed);
    });

    let result = run_in_process_teammate(config, &engine, &task_state).await;
    assert!(result.success);
    assert_eq!(result.turns, 1);
    assert_eq!(result.total_input_tokens, 60);
    assert_eq!(result.total_output_tokens, 40);

    // Task state should reflect completion
    let state = task_state.read().await;
    assert!(state.is_idle);
    assert_eq!(state.turn_count, 1);
}

#[tokio::test]
async fn test_current_work_interrupt_returns_teammate_to_idle() {
    let scratch = tempfile::TempDir::new().unwrap();
    unsafe {
        std::env::set_var("COCO_CONFIG_DIR", scratch.path());
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut config = make_config(cancelled.clone());
    config.max_turns = None;

    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (interrupted_tx, interrupted_rx) = tokio::sync::oneshot::channel();
    let engine = Arc::new(InterruptibleEngine {
        started: tokio::sync::Mutex::new(Some(started_tx)),
        interrupted: tokio::sync::Mutex::new(Some(interrupted_tx)),
    });
    let task_state = Arc::new(tokio::sync::RwLock::new(InProcessTeammateTaskState::new(
        "task-1".into(),
        make_identity(),
        "test".into(),
    )));

    let run = {
        let engine = engine.clone();
        let task_state = task_state.clone();
        tokio::spawn(
            async move { run_in_process_teammate(config, engine.as_ref(), &task_state).await },
        )
    };

    started_rx.await.expect("engine must start");
    assert!(
        task_state.read().await.interrupt_current_work(),
        "active per-turn token should be exposed through task state"
    );
    interrupted_rx.await.expect("engine must observe interrupt");

    for _ in 0..50 {
        if task_state.read().await.is_idle {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    {
        let state = task_state.read().await;
        assert!(
            state.is_idle,
            "teammate should return to idle after interrupt"
        );
        assert!(state.current_work_cancel.is_none());
        assert!(
            state
                .messages
                .iter()
                .any(|m| m.content == "Interrupted by user."),
            "interrupt message should be mirrored into teammate transcript"
        );
    }

    cancelled.store(true, Ordering::Relaxed);
    let result = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .expect("runner must exit after lifecycle cancel")
        .expect("join must succeed");
    assert!(result.success);
    assert_eq!(result.turns, 1);
}

#[tokio::test]
async fn test_active_query_drains_control_messages_into_live_state() {
    let scratch = tempfile::TempDir::new().unwrap();
    unsafe {
        std::env::set_var("COCO_CONFIG_DIR", scratch.path());
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut config = make_config(cancelled.clone());
    config.max_turns = None;

    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
    let engine = Arc::new(LiveControlEngine {
        started: tokio::sync::Mutex::new(Some(started_tx)),
        observed: tokio::sync::Mutex::new(Some(observed_tx)),
    });
    let task_state = Arc::new(tokio::sync::RwLock::new(InProcessTeammateTaskState::new(
        "task-1".into(),
        make_identity(),
        "test".into(),
    )));

    let run = {
        let engine = engine.clone();
        let task_state = task_state.clone();
        tokio::spawn(
            async move { run_in_process_teammate(config, engine.as_ref(), &task_state).await },
        )
    };

    started_rx.await.expect("engine must start");
    let writer_stop = tokio_util::sync::CancellationToken::new();
    let writer = {
        let writer_stop = writer_stop.clone();
        tokio::spawn(async move {
            loop {
                let _ = crate::mailbox::write_to_mailbox(
                    "worker",
                    crate::mailbox::TeammateMessage {
                        from: TEAM_LEAD_NAME.into(),
                        text: crate::mailbox::create_mode_set_request(
                            coco_types::PermissionMode::AcceptEdits,
                            TEAM_LEAD_NAME,
                        ),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        read: false,
                        color: None,
                        summary: Some("mode".into()),
                    },
                    "test",
                );
                let _ = crate::mailbox::write_to_mailbox(
                    "worker",
                    crate::mailbox::TeammateMessage {
                        from: TEAM_LEAD_NAME.into(),
                        text: serde_json::to_string(
                            &crate::mailbox::ProtocolMessage::TeamPermissionUpdate {
                                permission_update:
                                    crate::mailbox::protocol::WireTeamPermissionUpdate::AddRules {
                                        rules: vec![
                                            crate::mailbox::protocol::WirePermissionRuleValue {
                                                tool_name: "Edit".into(),
                                                rule_content: Some("/repo/**".into()),
                                            },
                                        ],
                                        behavior: coco_types::PermissionBehavior::Allow,
                                        destination: crate::mailbox::protocol::WireTeamPermissionUpdateDestination::Session,
                                    },
                                directory_path: "/repo".into(),
                                tool_name: "Edit".into(),
                            },
                        )
                        .unwrap(),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        read: false,
                        color: None,
                        summary: Some("permission".into()),
                    },
                    "test",
                );
                tokio::select! {
                    _ = writer_stop.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                }
            }
        })
    };

    tokio::time::timeout(Duration::from_secs(3), observed_rx)
        .await
        .expect("active query should observe live control updates")
        .expect("engine should signal observation");
    writer_stop.cancel();
    writer.await.expect("writer should stop cleanly");

    cancelled.store(true, Ordering::Relaxed);
    tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .expect("runner must exit after lifecycle cancel")
        .expect("join must succeed");
}

#[test]
fn test_wait_result_variants() {
    let _ = WaitResult::Aborted;
    let _ = WaitResult::ShutdownRequest {
        original_text: "stop".into(),
    };
    let _ = WaitResult::NewMessage {
        message: "hello".into(),
        from: "leader".into(),
        color: None,
        summary: None,
    };
}

#[test]
fn test_agent_query_config_clone() {
    let config = AgentQueryConfig {
        system_prompt: "test".into(),
        max_turns: Some(5),
        allowed_tools: vec![coco_types::ToolName::Read.as_str().into()],
        preserve_tool_use_results: true,
        ..Default::default()
    };
    let cloned = config;
    assert_eq!(cloned.system_prompt, "test");
    assert_eq!(cloned.max_turns, Some(5));
}

#[test]
fn test_poll_interval() {
    assert_eq!(POLL_INTERVAL_MS, 500);
}

// ── D1: semantic compaction in runner_loop ──

/// Engine stub that records every `compact_messages` call so the test
/// can assert it actually ran (and with what payload).
struct CompactingEngine {
    response: String,
    /// Tokens reported per turn so the threshold check trips.
    input_tokens: i64,
    output_tokens: i64,
    /// Recorded compaction invocations.
    compact_calls: Arc<tokio::sync::Mutex<Vec<i64>>>,
}

#[async_trait]
impl AgentExecutionEngine for CompactingEngine {
    async fn run_query(
        &self,
        _prompt: &str,
        _config: AgentQueryConfig,
    ) -> crate::Result<AgentQueryResult> {
        Ok(AgentQueryResult {
            messages: vec![
                std::sync::Arc::new(coco_messages::create_user_message("ask")),
                std::sync::Arc::new(coco_messages::create_assistant_message(
                    vec![coco_messages::AssistantContent::Text(
                        coco_messages::TextContent {
                            text: self.response.clone(),
                            provider_metadata: None,
                        },
                    )],
                    "test-model",
                    coco_types::TokenUsage::default(),
                )),
            ],
            token_count: self.input_tokens + self.output_tokens,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            turns: 1,
            tool_use_count: 0,
            cancelled: false,
            response_text: Some(self.response.clone()),
        })
    }

    async fn compact_messages(
        &self,
        messages: Vec<std::sync::Arc<coco_messages::Message>>,
        total_tokens: i64,
    ) -> crate::Result<Vec<std::sync::Arc<coco_messages::Message>>> {
        self.compact_calls.lock().await.push(total_tokens);
        // Return a strictly smaller history so the runner uses our
        // result instead of falling through to the safety valve.
        Ok(messages.into_iter().take(1).collect())
    }
}

#[tokio::test]
async fn test_runner_invokes_compact_when_over_threshold() {
    // Single turn pushes total tokens > threshold → runner must
    // call `compact_messages` with the live token count. We cancel
    // after one turn so the loop exits cleanly.
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = Arc::clone(&cancelled);
    let mut config = make_config(cancelled.clone());
    config.max_turns = Some(1);
    config.auto_compact_threshold = 100;

    let compact_calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let engine = CompactingEngine {
        response: "ok".into(),
        input_tokens: 80,
        output_tokens: 60, // 80 + 60 = 140 > 100 → tripped
        compact_calls: compact_calls.clone(),
    };
    let task_state = tokio::sync::RwLock::new(InProcessTeammateTaskState::new(
        "task-1".into(),
        make_identity(),
        "test".into(),
    ));
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancelled_clone.store(true, Ordering::Relaxed);
    });

    let _ = run_in_process_teammate(config, &engine, &task_state).await;

    let calls = compact_calls.lock().await;
    assert!(
        !calls.is_empty(),
        "compact_messages must be invoked when over threshold"
    );
    assert_eq!(calls[0], 140, "compact_messages got the right token count");
}

#[tokio::test]
async fn test_runner_skips_compact_when_under_threshold() {
    // Token usage stays below threshold → compact_messages must NOT
    // be invoked. Avoids paying for compaction on short workers.
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = Arc::clone(&cancelled);
    let mut config = make_config(cancelled.clone());
    config.max_turns = Some(1);
    config.auto_compact_threshold = 1_000_000;

    let compact_calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let engine = CompactingEngine {
        response: "ok".into(),
        input_tokens: 100,
        output_tokens: 50,
        compact_calls: compact_calls.clone(),
    };
    let task_state = tokio::sync::RwLock::new(InProcessTeammateTaskState::new(
        "task-1".into(),
        make_identity(),
        "test".into(),
    ));
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancelled_clone.store(true, Ordering::Relaxed);
    });

    let _ = run_in_process_teammate(config, &engine, &task_state).await;

    let calls = compact_calls.lock().await;
    assert!(
        calls.is_empty(),
        "compact_messages must not be invoked under threshold; got {calls:?}"
    );
}

// ── D5: plan-approval block-and-await ──

#[tokio::test]
async fn test_wait_for_plan_approval_returns_none_on_cancel() {
    // Cancellation must short-circuit the poll loop so a teardown
    // doesn't block forever waiting for a leader response that's
    // never coming.
    let identity = make_identity();
    let cancelled = Arc::new(AtomicBool::new(true));
    let result = wait_for_plan_approval(&identity, &cancelled, "req-cancel").await;
    assert!(result.is_none(), "cancel must yield None, got {result:?}");
}

#[tokio::test]
async fn test_wait_for_plan_approval_picks_up_matching_response() {
    // Drop a `PlanApprovalResponse` envelope into the worker's
    // mailbox with the matching request_id; the helper must see it
    // and return `Some((approved, feedback))`.
    let scratch = tempfile::TempDir::new().unwrap();
    // SAFETY: Tests run sequentially within a single process for this
    // suite (file IO ensures it). The mailbox helpers honour
    // CocoConfigDir for the on-disk root.
    unsafe {
        std::env::set_var("COCO_CONFIG_DIR", scratch.path());
    }
    let identity = TeammateIdentity {
        agent_id: "worker@d5".into(),
        agent_name: format!("worker-{}", uuid::Uuid::new_v4().simple()),
        team_name: format!("team-{}", uuid::Uuid::new_v4().simple()),
        color: None,
        plan_mode_required: true,
    };

    // Pre-write a plan-approval response so the helper finds it on
    // its first poll and exits without waiting. No
    // `create_plan_approval_response_message` helper exists yet (the
    // leader UI is the producer in production), so we serialise the
    // protocol variant directly.
    let response = mailbox::ProtocolMessage::PlanApprovalResponse {
        request_id: "req-1".into(),
        approved: true,
        feedback: Some("looks good".into()),
        timestamp: chrono::Utc::now().to_rfc3339(),
        permission_mode: None,
    };
    let envelope = serde_json::to_string(&response).unwrap();
    let message = mailbox::TeammateMessage {
        from: "team-lead".into(),
        text: envelope,
        timestamp: chrono::Utc::now().to_rfc3339(),
        read: false,
        color: None,
        summary: Some("plan approval".to_string()),
    };
    mailbox::write_to_mailbox(&identity.agent_name, message, &identity.team_name).unwrap();

    let cancelled = Arc::new(AtomicBool::new(false));
    let outcome = tokio::time::timeout(
        Duration::from_secs(3),
        wait_for_plan_approval(&identity, &cancelled, "req-1"),
    )
    .await
    .expect("wait_for_plan_approval must return within timeout");

    let (approved, feedback) = outcome.expect("must return Some on match");
    assert!(approved, "explicit approval must surface as true");
    assert_eq!(feedback.as_deref(), Some("looks good"));
}

// ── D6: MailboxPermissionBridge ──

use coco_tool_runtime::ToolPermissionBridge;

#[tokio::test]
async fn test_mailbox_permission_bridge_fails_closed_on_cancel() {
    // When cancellation fires before a response arrives the bridge
    // returns Err — the worker must NOT silently approve. Fail-closed
    // semantics are critical for the security model.
    let identity = TeammateIdentity {
        agent_id: "worker@d6".into(),
        agent_name: format!("worker-{}", uuid::Uuid::new_v4().simple()),
        team_name: format!("team-{}", uuid::Uuid::new_v4().simple()),
        color: None,
        plan_mode_required: false,
    };
    let cancelled = Arc::new(AtomicBool::new(true));
    let bridge = MailboxPermissionBridge::new(identity, cancelled);

    let req = coco_tool_runtime::ToolPermissionRequest {
        id: "req-cancel".into(),
        tool_use_id: "tu-1".into(),
        agent_id: "worker@d6".into(),
        tool_name: "Bash".into(),
        description: "rm -rf /tmp/test".into(),
        input: serde_json::json!({"command": "rm -rf /tmp/test"}),
        suggestions: vec![],
        choices: None,
    };
    let result = bridge.request_permission(req).await;
    assert!(
        result.is_err(),
        "cancelled bridge must fail closed (Err), got {result:?}"
    );
}

#[tokio::test]
async fn test_mailbox_permission_bridge_returns_resolution_on_response() {
    // Pre-write a PermissionResponse and ensure the bridge unwraps
    // it correctly into ToolPermissionResolution::Approved.
    let scratch = tempfile::TempDir::new().unwrap();
    unsafe {
        std::env::set_var("COCO_CONFIG_DIR", scratch.path());
    }
    let identity = TeammateIdentity {
        agent_id: "worker@d6b".into(),
        agent_name: format!("worker-{}", uuid::Uuid::new_v4().simple()),
        team_name: format!("team-{}", uuid::Uuid::new_v4().simple()),
        color: None,
        plan_mode_required: false,
    };

    // Write the approval response BEFORE the bridge sends its request,
    // so the helper finds it on the first poll. (`request_permission_via_mailbox`
    // writes its request to the LEADER's inbox, then polls the
    // worker's own inbox for the response — that's where we inject.)
    let envelope =
        mailbox::create_permission_response_message("req-ok", /*approved*/ true, None);
    let response_msg = mailbox::TeammateMessage {
        from: "team-lead".into(),
        text: envelope,
        timestamp: chrono::Utc::now().to_rfc3339(),
        read: false,
        color: None,
        summary: Some("permission response".to_string()),
    };
    mailbox::write_to_mailbox(&identity.agent_name, response_msg, &identity.team_name).unwrap();

    let cancelled = Arc::new(AtomicBool::new(false));
    let bridge = MailboxPermissionBridge::new(identity.clone(), cancelled);
    let req = coco_tool_runtime::ToolPermissionRequest {
        id: "req-ok".into(),
        tool_use_id: "tu-1".into(),
        agent_id: identity.agent_id.clone(),
        tool_name: "Read".into(),
        description: "read file".into(),
        input: serde_json::json!({"path": "/tmp/x"}),
        suggestions: vec![],
        choices: None,
    };
    let resolution = tokio::time::timeout(Duration::from_secs(3), bridge.request_permission(req))
        .await
        .expect("must return within timeout")
        .expect("bridge must return Ok on response match");
    assert_eq!(
        resolution.decision,
        coco_tool_runtime::ToolPermissionDecision::Approved
    );
}

#[tokio::test]
async fn test_mailbox_permission_bridge_preserves_response_payload() {
    use coco_tool_runtime::ToolPermissionBridge;

    let scratch = tempfile::TempDir::new().unwrap();
    unsafe {
        std::env::set_var("COCO_CONFIG_DIR", scratch.path());
    }
    let identity = TeammateIdentity {
        agent_id: "worker@payload".into(),
        agent_name: format!("worker-{}", uuid::Uuid::new_v4().simple()),
        team_name: format!("team-{}", uuid::Uuid::new_v4().simple()),
        color: None,
        plan_mode_required: false,
    };
    let response_text = serde_json::json!({
        "type": "permission_response",
        "request_id": "req-payload",
        "subtype": "success",
        "response": {
            "updated_input": {"path": "/tmp/updated"},
            "permission_updates": [{
                "type": "addRules",
                "rules": [{"toolName": "Read", "ruleContent": "/tmp/**"}],
                "behavior": "allow",
                "destination": "session"
            }]
        }
    })
    .to_string();
    mailbox::write_to_mailbox(
        &identity.agent_name,
        mailbox::TeammateMessage {
            from: TEAM_LEAD_NAME.into(),
            text: response_text,
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            color: None,
            summary: Some("permission response".to_string()),
        },
        &identity.team_name,
    )
    .unwrap();

    let bridge = MailboxPermissionBridge::new(identity.clone(), Arc::new(AtomicBool::new(false)));
    let req = coco_tool_runtime::ToolPermissionRequest {
        id: "req-payload".into(),
        tool_use_id: "tu-1".into(),
        agent_id: identity.agent_id.clone(),
        tool_name: "Read".into(),
        description: "read file".into(),
        input: serde_json::json!({"path": "/tmp/original"}),
        suggestions: vec![],
        choices: None,
    };
    let resolution = tokio::time::timeout(Duration::from_secs(3), bridge.request_permission(req))
        .await
        .expect("must return within timeout")
        .expect("bridge must return Ok on response match");

    assert_eq!(
        resolution.updated_input,
        Some(serde_json::json!({"path": "/tmp/updated"}))
    );
    assert_eq!(resolution.applied_updates.len(), 1);
    assert!(resolution.content_blocks.is_none());
}

#[tokio::test]
async fn test_wait_for_next_prompt_claims_unassigned_task() {
    let scratch = tempfile::TempDir::new().unwrap();
    unsafe {
        std::env::set_var("COCO_CONFIG_DIR", scratch.path());
    }
    let identity = TeammateIdentity {
        agent_id: "worker@tasks".into(),
        agent_name: format!("worker-{}", uuid::Uuid::new_v4().simple()),
        team_name: format!("team-{}", uuid::Uuid::new_v4().simple()),
        color: None,
        plan_mode_required: false,
    };
    let task_list: coco_tool_runtime::TaskListHandleRef =
        Arc::new(coco_tool_runtime::InMemoryTaskListHandle::new());
    let task = task_list
        .create_task(
            "Wire task polling".into(),
            "Claim this when mailbox is empty".into(),
            None,
            None,
        )
        .await
        .unwrap();

    let cancelled = Arc::new(AtomicBool::new(false));
    let task_state = make_task_state(&identity);
    let control_state = tokio::sync::RwLock::new(TeammateControlState::default());
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        wait_for_next_prompt_or_shutdown(
            &identity,
            &cancelled,
            Some(&task_list),
            &task_state,
            &control_state,
        ),
    )
    .await
    .expect("task polling must return within timeout");

    match result {
        WaitResult::NewMessage {
            message, summary, ..
        } => {
            assert!(message.contains("Complete all open tasks. Start with task #1:"));
            assert!(message.contains("Wire task polling"));
            assert_eq!(summary.as_deref(), Some("task list assignment"));
        }
        other => panic!("expected task assignment, got {other:?}"),
    }

    let claimed = task_list
        .get_task(&task.id)
        .await
        .unwrap()
        .expect("task must still exist");
    assert_eq!(claimed.owner.as_deref(), Some(identity.agent_name.as_str()));
    assert_eq!(claimed.status, coco_types::TaskListStatus::InProgress);
}

#[tokio::test]
async fn test_wait_for_next_prompt_mailbox_beats_task_claim() {
    let team_name = format!("team-{}", uuid::Uuid::new_v4().simple());
    let identity = TeammateIdentity {
        agent_id: format!("worker@{team_name}"),
        agent_name: format!("worker-{}", uuid::Uuid::new_v4().simple()),
        team_name: team_name.clone(),
        color: None,
        plan_mode_required: false,
    };
    let task_list: coco_tool_runtime::TaskListHandleRef =
        Arc::new(coco_tool_runtime::InMemoryTaskListHandle::new());
    let task = task_list
        .create_task("Task should wait".into(), "mailbox wins".into(), None, None)
        .await
        .unwrap();
    crate::mailbox::write_to_mailbox(
        &identity.agent_name,
        crate::mailbox::TeammateMessage {
            from: crate::constants::TEAM_LEAD_NAME.into(),
            text: "direct instruction".into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            color: None,
            summary: Some("leader message".into()),
        },
        &identity.team_name,
    )
    .unwrap();

    let cancelled = Arc::new(AtomicBool::new(false));
    let task_state = make_task_state(&identity);
    let control_state = tokio::sync::RwLock::new(TeammateControlState::default());
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        wait_for_next_prompt_or_shutdown(
            &identity,
            &cancelled,
            Some(&task_list),
            &task_state,
            &control_state,
        ),
    )
    .await
    .expect("mailbox polling must return within timeout");

    match result {
        WaitResult::NewMessage {
            message, summary, ..
        } => {
            assert_eq!(message, "direct instruction");
            assert_eq!(summary.as_deref(), Some("leader message"));
        }
        other => panic!("expected mailbox message, got {other:?}"),
    }

    let unclaimed = task_list.get_task(&task.id).await.unwrap().unwrap();
    assert_eq!(unclaimed.owner, None);
    assert_eq!(unclaimed.status, coco_types::TaskListStatus::Pending);
}

#[tokio::test]
async fn test_wait_for_next_prompt_applies_control_messages_before_task() {
    let scratch = tempfile::TempDir::new().unwrap();
    unsafe {
        std::env::set_var("COCO_CONFIG_DIR", scratch.path());
    }
    let team_name = format!("team-{}", uuid::Uuid::new_v4().simple());
    let identity = TeammateIdentity {
        agent_id: format!("worker@{team_name}"),
        agent_name: format!("worker-{}", uuid::Uuid::new_v4().simple()),
        team_name: team_name.clone(),
        color: None,
        plan_mode_required: false,
    };
    crate::mailbox::write_to_mailbox(
        &identity.agent_name,
        crate::mailbox::TeammateMessage {
            from: TEAM_LEAD_NAME.into(),
            text: crate::mailbox::create_mode_set_request(
                coco_types::PermissionMode::AcceptEdits,
                TEAM_LEAD_NAME,
            ),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            color: None,
            summary: None,
        },
        &identity.team_name,
    )
    .unwrap();
    crate::mailbox::write_to_mailbox(
        &identity.agent_name,
        crate::mailbox::TeammateMessage {
            from: TEAM_LEAD_NAME.into(),
            text: serde_json::json!({
                "type": "team_permission_update",
                "permissionUpdate": {
                    "type": "addRules",
                    "rules": [{"toolName": "Edit", "ruleContent": "/repo/**"}],
                    "behavior": "allow",
                    "destination": "session"
                },
                "directoryPath": "/repo",
                "toolName": "Edit"
            })
            .to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            color: None,
            summary: None,
        },
        &identity.team_name,
    )
    .unwrap();

    let task_list: coco_tool_runtime::TaskListHandleRef =
        Arc::new(coco_tool_runtime::InMemoryTaskListHandle::new());
    task_list
        .create_task("Next task".into(), String::new(), None, None)
        .await
        .unwrap();

    let cancelled = Arc::new(AtomicBool::new(false));
    let task_state = make_task_state(&identity);
    let control_state = tokio::sync::RwLock::new(TeammateControlState::default());
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        wait_for_next_prompt_or_shutdown(
            &identity,
            &cancelled,
            Some(&task_list),
            &task_state,
            &control_state,
        ),
    )
    .await
    .expect("control messages should be consumed before task assignment");

    assert!(matches!(result, WaitResult::NewMessage { .. }));
    assert_eq!(
        task_state.read().await.permission_mode,
        coco_types::PermissionMode::AcceptEdits
    );
    let rules_store = control_state.read().await.team_permission_rules.clone();
    let rules = rules_store.read().await;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].value.tool_pattern, "Edit");
    assert_eq!(rules[0].value.rule_content.as_deref(), Some("/repo/**"));
}

#[tokio::test]
async fn test_claim_first_available_task_skips_owned_and_blocked_tasks() {
    let task_list: coco_tool_runtime::TaskListHandleRef =
        Arc::new(coco_tool_runtime::InMemoryTaskListHandle::new());
    let owned = task_list
        .create_task("Owned".into(), String::new(), None, None)
        .await
        .unwrap();
    task_list
        .claim_task(&owned.id, "other-worker", false)
        .await
        .unwrap();
    let blocker = task_list
        .create_task("Blocker".into(), String::new(), None, None)
        .await
        .unwrap();
    task_list
        .claim_task(&blocker.id, "other-worker", false)
        .await
        .unwrap();
    let blocked = task_list
        .create_task("Blocked".into(), String::new(), None, None)
        .await
        .unwrap();
    task_list
        .block_task(&blocker.id, &blocked.id)
        .await
        .unwrap();
    let available = task_list
        .create_task("Available".into(), String::new(), None, None)
        .await
        .unwrap();

    let claimed = claim_first_available_task(&task_list, "worker")
        .await
        .expect("one available task should be claimed");
    assert_eq!(claimed.id, available.id);
    assert_eq!(claimed.owner.as_deref(), Some("worker"));
    assert_eq!(claimed.status, coco_types::TaskListStatus::InProgress);

    let owned_after = task_list.get_task(&owned.id).await.unwrap().unwrap();
    assert_eq!(owned_after.owner.as_deref(), Some("other-worker"));
    let blocked_after = task_list.get_task(&blocked.id).await.unwrap().unwrap();
    assert_eq!(blocked_after.owner, None);
    assert_eq!(blocked_after.status, coco_types::TaskListStatus::Pending);
}

#[tokio::test]
async fn test_claim_first_available_task_treats_completed_blockers_as_resolved() {
    let task_list: coco_tool_runtime::TaskListHandleRef =
        Arc::new(coco_tool_runtime::InMemoryTaskListHandle::new());
    let blocker = task_list
        .create_task("Blocker".into(), String::new(), None, None)
        .await
        .unwrap();
    task_list
        .update_task(
            &blocker.id,
            coco_types::TaskRecordUpdate {
                status: Some(coco_types::TaskListStatus::Completed),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let blocked = task_list
        .create_task("Blocked".into(), String::new(), None, None)
        .await
        .unwrap();
    task_list
        .block_task(&blocker.id, &blocked.id)
        .await
        .unwrap();

    let claimed = claim_first_available_task(&task_list, "worker")
        .await
        .expect("completed blocker should not block claim");
    assert_eq!(claimed.id, blocked.id);
    assert_eq!(claimed.owner.as_deref(), Some("worker"));
    assert_eq!(claimed.status, coco_types::TaskListStatus::InProgress);
}
