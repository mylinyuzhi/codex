use std::sync::Arc;
use std::time::Duration;

use coco_hooks::HookEvaluationResult;
use coco_query::QueryEngineConfig;
use coco_query::hook_llm::HookAgentRunRequest;
use coco_query::hook_llm::HookAgentRunner;
use coco_tool_runtime::ToolRegistry;
use tokio_util::sync::CancellationToken;

use crate::session_runtime::SessionRuntime;

const MAX_AGENT_HOOK_TURNS: i32 = 50;

pub struct SessionRuntimeHookAgentRunner {
    runtime: Arc<SessionRuntime>,
}

impl std::fmt::Debug for SessionRuntimeHookAgentRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionRuntimeHookAgentRunner").finish()
    }
}

impl SessionRuntimeHookAgentRunner {
    pub fn new(runtime: Arc<SessionRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait]
impl HookAgentRunner for SessionRuntimeHookAgentRunner {
    async fn run(&self, request: HookAgentRunRequest) -> HookEvaluationResult {
        let result =
            tokio::time::timeout(request.timeout, run_agent(self.runtime.clone(), request)).await;
        match result {
            Err(_elapsed) => HookEvaluationResult::Cancelled,
            Ok(Ok(output)) => output,
            Ok(Err(error)) => HookEvaluationResult::NonBlockingError { error },
        }
    }
}

async fn run_agent(
    runtime: Arc<SessionRuntime>,
    request: HookAgentRunRequest,
) -> Result<HookEvaluationResult, String> {
    let tools = scoped_tool_registry(&runtime)?;
    let hooks = scoped_hook_registry()?;
    let mut config = runtime.current_engine_config().await;
    configure_hook_agent(&mut config, &request);

    let cancel = CancellationToken::new();
    let engine = runtime
        .build_engine_from_config_with_registries(config, cancel, tools, Some(hooks))
        .await
        .with_model_runtime_source(request.model_source.clone());

    let query_result = engine
        .run(&request.prompt)
        .await
        .map_err(|e| format!("hook agent engine run: {e}"))?;

    parse_structured_output(query_result.structured_output)
}

fn scoped_tool_registry(runtime: &SessionRuntime) -> Result<Arc<ToolRegistry>, String> {
    let registry = Arc::new(ToolRegistry::new());
    for tool in runtime.registered_tools() {
        registry.register(tool);
    }
    coco_tools::register_structured_output_tool(&registry, hook_agent_schema())?;
    Ok(registry)
}

fn scoped_hook_registry() -> Result<Arc<coco_hooks::HookRegistry>, String> {
    let registry = Arc::new(coco_hooks::HookRegistry::new());
    registry
        .register_function_hook(
            format!("hook-agent-structured-output-{}", uuid::Uuid::new_v4()),
            coco_types::HookEventType::Stop,
            None,
            Duration::from_millis(5_000),
            Arc::new(coco_query::structured_output_enforcement::StructuredOutputEnforcement),
            format!(
                "You MUST call the {} tool to complete this request. Call this tool now.",
                coco_types::ToolName::StructuredOutput.as_str()
            ),
        )
        .map_err(|e| format!("failed to register hook-agent StructuredOutput Stop hook: {e}"))?;
    Ok(registry)
}

fn configure_hook_agent(config: &mut QueryEngineConfig, request: &HookAgentRunRequest) {
    config.model_id = request.model_id.clone();
    config.permission_mode = coco_types::PermissionMode::Default;
    config.max_turns = Some(MAX_AGENT_HOOK_TURNS);
    config.total_token_budget = None;
    config.streaming_tool_execution = false;
    config.is_non_interactive = true;
    config.avoid_permission_prompts = true;
    config.query_source_override = Some(coco_types::ForkLabel::HookAgent.as_str().to_string());
    config.fork_label = Some(coco_types::ForkLabel::HookAgent);
    config.session_id = format!("hook-agent-{}", uuid::Uuid::new_v4());
}

fn hook_agent_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["ok"],
        "properties": {
            "ok": { "type": "boolean" },
            "reason": { "type": "string" }
        }
    })
}

fn parse_structured_output(
    structured_output: Option<serde_json::Value>,
) -> Result<HookEvaluationResult, String> {
    let Some(value) = structured_output else {
        return Ok(HookEvaluationResult::Cancelled);
    };
    let Some(ok) = value.get("ok").and_then(serde_json::Value::as_bool) else {
        return Err("hook agent StructuredOutput missing boolean `ok`".to_string());
    };
    if ok {
        return Ok(HookEvaluationResult::Ok);
    }
    let reason = value
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Agent hook condition was not met")
        .to_string();
    Ok(HookEvaluationResult::Blocking { reason })
}

pub async fn install(runtime: Arc<SessionRuntime>) {
    let runner: coco_query::hook_llm::HookAgentRunnerRef =
        Arc::new(SessionRuntimeHookAgentRunner::new(runtime.clone()));
    runtime.attach_hook_agent_runner(runner).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_structured_output_ok_true() {
        let result = parse_structured_output(Some(serde_json::json!({"ok": true}))).unwrap();
        assert!(matches!(result, HookEvaluationResult::Ok));
    }

    #[test]
    fn parse_structured_output_ok_false_blocks() {
        let result =
            parse_structured_output(Some(serde_json::json!({"ok": false, "reason": "bad"})))
                .unwrap();
        match result {
            HookEvaluationResult::Blocking { reason } => assert_eq!(reason, "bad"),
            other => panic!("expected Blocking, got {other:?}"),
        }
    }

    #[test]
    fn parse_structured_output_missing_output_cancels() {
        let result = parse_structured_output(None).unwrap();
        assert!(matches!(result, HookEvaluationResult::Cancelled));
    }
}
