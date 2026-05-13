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
            messages: vec![serde_json::json!({"role": "assistant", "content": self.response})],
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
        plan_mode_required: false,
        hooks: None,
        orchestration_ctx: None,
    }
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
                serde_json::json!({"role": "user", "content": "ask"}),
                serde_json::json!({"role": "assistant", "content": self.response}),
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
        messages: Vec<serde_json::Value>,
        total_tokens: i64,
    ) -> crate::Result<Vec<serde_json::Value>> {
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
