use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::*;

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
    ) -> anyhow::Result<AgentQueryResult> {
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
    ) -> anyhow::Result<AgentQueryResult> {
        anyhow::bail!("API error: rate limited")
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
        model: None,
        max_turns: Some(5),
        allowed_tools: vec!["Read".into()],
        fork_context_messages: vec![],
        preserve_tool_use_results: true,
        bypass_permissions_available: false,
    };
    let cloned = config;
    assert_eq!(cloned.system_prompt, "test");
    assert_eq!(cloned.max_turns, Some(5));
}

#[test]
fn test_poll_interval() {
    assert_eq!(POLL_INTERVAL_MS, 500);
}
