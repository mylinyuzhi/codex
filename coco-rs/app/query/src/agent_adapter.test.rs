use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use coco_tool_runtime::AgentQueryConfig;
use coco_tool_runtime::AgentQueryEngine;

#[test]
fn test_agent_query_config_fork_context_messages_field_round_trips() {
    // Lock in that fork_context_messages is serde-stable — it
    // crosses the coco-tool-runtime → coco-query boundary as JSON via
    // AgentQueryConfig.
    let cfg = AgentQueryConfig {
        system_prompt: "s".into(),
        model: "m".into(),
        max_turns: Some(1),
        context_window: None,
        max_output_tokens: None,
        allowed_tools: Vec::new(),
        preserve_tool_use_results: true,
        permission_mode: None,
        agent_id: None,
        is_teammate: false,
        plan_mode_required: false,
        session_id: None,
        bypass_permissions_available: false,
        cwd_override: None,
        fork_context_messages: vec![serde_json::json!({"type":"user","content":"parent turn 1"})],
        model_role: None,
    };
    let s = serde_json::to_string(&cfg).unwrap();
    let back: AgentQueryConfig = serde_json::from_str(&s).unwrap();
    assert!(back.preserve_tool_use_results);
    assert_eq!(back.fork_context_messages.len(), 1);
}

#[tokio::test]
async fn test_no_op_engine_returns_error() {
    let engine = coco_tool_runtime::NoOpAgentQueryEngine;
    let config = AgentQueryConfig {
        system_prompt: "test".into(),
        model: "test-model".into(),
        max_turns: Some(1),
        context_window: None,
        max_output_tokens: None,
        allowed_tools: Vec::new(),
        preserve_tool_use_results: false,
        permission_mode: None,
        agent_id: None,
        is_teammate: false,
        plan_mode_required: false,
        session_id: None,
        bypass_permissions_available: false,
        cwd_override: None,
        fork_context_messages: Vec::new(),
        model_role: None,
    };
    let result = engine.execute_query("hello", config).await;
    assert!(result.is_err());
}

/// Test-only factory variant that observes the role argument and
/// returns an error via a synthetic `QueryEngine`-less path. We
/// can't return a `QueryEngine` here without building a whole
/// `ApiClient` + `ToolRegistry`; instead we use
/// [`std::panic::catch_unwind`] to trap a controlled panic INSIDE
/// the factory closure, leaving the assertion on `observed` the
/// durable check. This avoids depending on tokio spawn semantics
/// for panic propagation.
fn role_observer() -> (
    Arc<std::sync::Mutex<Option<coco_types::ModelRole>>>,
    Arc<AtomicU32>,
) {
    (
        Arc::new(std::sync::Mutex::new(None)),
        Arc::new(AtomicU32::new(0)),
    )
}

#[tokio::test]
async fn test_adapter_threads_model_role_to_factory() {
    // Contract: `AgentQueryConfig.model_role` flows through the
    // factory unchanged so downstream code can resolve the right
    // primary + fallback chain for the subagent.
    use super::QueryEngineAdapter;
    use super::QueryEngineFactory;

    let (observed, calls) = role_observer();
    let observed_c = observed.clone();
    let calls_c = calls.clone();

    let factory: QueryEngineFactory = Arc::new(move |_cfg, role| {
        *observed_c.lock().unwrap() = role;
        calls_c.fetch_add(1, Ordering::SeqCst);
        // Controlled abort: the factory MUST be driven by adapter
        // in production. We use `resume_unwind` with a sentinel so
        // `catch_unwind` in the driving test traps it cleanly.
        std::panic::resume_unwind(Box::new("observed"));
    });
    let adapter = QueryEngineAdapter::new(factory);

    let cfg = AgentQueryConfig {
        system_prompt: "s".into(),
        model: "m".into(),
        max_turns: Some(1),
        context_window: None,
        max_output_tokens: None,
        allowed_tools: Vec::new(),
        preserve_tool_use_results: false,
        permission_mode: None,
        agent_id: None,
        is_teammate: false,
        plan_mode_required: false,
        session_id: None,
        bypass_permissions_available: false,
        cwd_override: None,
        fork_context_messages: Vec::new(),
        model_role: Some(coco_types::ModelRole::Explore),
    };

    // `catch_unwind` on an async path: wrap `futures::executor` or
    // spawn-and-join. tokio's JoinError carries the panic payload
    // and is the standard test pattern; kept here for async
    // compatibility.
    let handle = tokio::task::spawn(async move { adapter.execute_query("hello", cfg).await });
    let join_err = handle.await.expect_err("factory panic must bubble up");
    assert!(join_err.is_panic());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "factory called exactly once",
    );
    assert_eq!(
        *observed.lock().unwrap(),
        Some(coco_types::ModelRole::Explore),
        "adapter must thread model_role through unchanged",
    );
}

#[tokio::test]
async fn test_adapter_defers_to_factory_default_when_role_none() {
    use super::QueryEngineAdapter;
    use super::QueryEngineFactory;

    // Seed with a sentinel OTHER than None so the observer proves
    // the factory saw exactly `None` (not a leftover value).
    let observed: Arc<std::sync::Mutex<Option<coco_types::ModelRole>>> =
        Arc::new(std::sync::Mutex::new(Some(coco_types::ModelRole::Memory)));
    let observed_c = observed.clone();

    let factory: QueryEngineFactory = Arc::new(move |_cfg, role| {
        *observed_c.lock().unwrap() = role;
        std::panic::resume_unwind(Box::new("observed"));
    });
    let adapter = QueryEngineAdapter::new(factory);

    let cfg = AgentQueryConfig {
        system_prompt: "s".into(),
        model: "m".into(),
        max_turns: Some(1),
        context_window: None,
        max_output_tokens: None,
        allowed_tools: Vec::new(),
        preserve_tool_use_results: false,
        permission_mode: None,
        agent_id: None,
        is_teammate: false,
        plan_mode_required: false,
        session_id: None,
        bypass_permissions_available: false,
        cwd_override: None,
        fork_context_messages: Vec::new(),
        model_role: None,
    };

    let handle = tokio::task::spawn(async move { adapter.execute_query("hello", cfg).await });
    let join_err = handle.await.expect_err("factory panic must bubble up");
    assert!(join_err.is_panic());
    assert_eq!(
        *observed.lock().unwrap(),
        None,
        "None must flow verbatim so the factory applies its default",
    );
}
