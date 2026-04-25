//! Agent query adapter — bridges QueryEngine to AgentQueryEngine trait.
//!
//! TS: runAgent() in tools/AgentTool/runAgent.ts drives the query loop
//! for subagents. This module adapts the existing QueryEngine to provide
//! the same capability via the AgentQueryEngine trait.
//!
//! **Dependency flow**:
//! ```text
//! coco-tool  (defines AgentQueryEngine trait)
//!     ↓
//! coco-query (this adapter implements it via QueryEngine)
//!     ↓
//! coco-state (SwarmAgentHandle / InProcessTeammateRunner consumes it)
//! ```

use std::sync::Arc;

use coco_tool::AgentQueryConfig;
use coco_tool::AgentQueryEngine;
use coco_tool::AgentQueryResult;
use coco_types::ModelRole;

use crate::engine::QueryEngine;
use crate::engine::QueryEngineConfig;

/// Factory function type for creating QueryEngine instances.
///
/// Each agent query gets a fresh engine with its own config plus an
/// optional `ModelRole` that the factory uses to select the right
/// primary `ApiClient` + fallback chain. `None` defaults to the
/// parent session's model (TS parity: `runAgent.ts` inherits the
/// parent client unless the agent definition specifies a model).
/// The factory captures shared state (tool registry, hooks, retry
/// config) and resolves per-role clients from `RuntimeConfig`.
pub type QueryEngineFactory =
    Arc<dyn Fn(QueryEngineConfig, Option<ModelRole>) -> QueryEngine + Send + Sync>;

/// Adapter that wraps QueryEngine to implement AgentQueryEngine.
///
/// Each subagent gets its own `QueryEngineAdapter` with a dedicated
/// QueryEngine instance configured for the agent's model, tools, and budget.
pub struct QueryEngineAdapter {
    /// Factory function to create QueryEngine instances per query.
    engine_factory: QueryEngineFactory,
}

impl QueryEngineAdapter {
    pub fn new(engine_factory: QueryEngineFactory) -> Self {
        Self { engine_factory }
    }
}

#[async_trait::async_trait]
impl AgentQueryEngine for QueryEngineAdapter {
    async fn execute_query(
        &self,
        prompt: &str,
        config: AgentQueryConfig,
    ) -> anyhow::Result<AgentQueryResult> {
        // Resolve the subagent's permission mode. Parent is expected to
        // have applied the TS inheritance rule before calling; we just
        // parse/fall back. TS: runAgent.ts:412-434.
        let permission_mode = config
            .permission_mode
            .as_deref()
            .and_then(|s| {
                serde_json::from_value::<coco_types::PermissionMode>(serde_json::json!(s)).ok()
            })
            .unwrap_or(coco_types::PermissionMode::Default);
        let engine_config = QueryEngineConfig {
            max_turns: config.max_turns.unwrap_or(30),
            max_tokens: None,
            system_prompt: Some(config.system_prompt),
            append_system_prompt: None,
            model_name: config.model,
            permission_mode,
            // Inherit the parent session's bypass capability. TS
            // parity: `spawnUtils.ts:53` / `spawnMultiAgent.ts:223`
            // forward `--dangerously-skip-permissions` to spawned
            // child processes; the in-process analog is this field.
            bypass_permissions_available: config.bypass_permissions_available,
            context_window: config.context_window.unwrap_or(200_000),
            max_output_tokens: config.max_output_tokens.unwrap_or(16_384),
            max_budget_usd: None,
            streaming_tool_execution: true,
            is_non_interactive: true,
            thinking_level: None,
            session_id: config.session_id.unwrap_or_default(),
            project_dir: None,
            allow_rules: Default::default(),
            deny_rules: Default::default(),
            ask_rules: Default::default(),
            // Propagate the subagent's cwd_override (set by worktree
            // isolation or explicit `cwd:` input) so the child
            // engine's ToolContextFactory installs it onto every
            // ToolUseContext. Absolute-path tools ignore it; Glob /
            // Grep / Bash operate inside the override.
            cwd_override: config.cwd_override.clone(),
            plans_directory: None,
            agent_id: config.agent_id,
            is_teammate: config.is_teammate,
            plan_mode_required: config.plan_mode_required,
            plan_mode_settings: coco_config::PlanModeSettings::default(),
            disable_all_hooks: false,
            allow_managed_hooks_only: false,
            enable_token_budget_continuation: false,
            auto_compact_enabled: true,
            system_reminder: coco_config::SystemReminderConfig::default(),
            tool_config: coco_config::ToolConfig::default(),
            sandbox_config: coco_config::SandboxConfig::default(),
            memory_config: coco_config::MemoryConfig::default(),
            shell_config: coco_config::ShellConfig::default(),
            web_fetch_config: coco_config::WebFetchConfig::default(),
            web_search_config: coco_config::WebSearchConfig::default(),
        };

        // Role resolution: the adapter threads the subagent's role
        // through to the factory so the correct primary+fallback
        // chain is installed. `None` defers to the factory's
        // default (typically the parent session's Main role).
        let role = config.model_role;
        let engine = (self.engine_factory)(engine_config, role);

        // Fork mode: if the parent surfaced context messages, use
        // `run_with_messages` so the child's first turn sees the
        // parent's history prepended. TS parity:
        // `AgentTool.tsx:627-630` passes `forkContextMessages:
        // toolUseContext.messages` for `isForkPath`.
        let result = if !config.fork_context_messages.is_empty() {
            // Deserialize each message JSON back into typed
            // `Message`. Any entry that fails deserialization is
            // dropped with a warn — fork context is best-effort
            // (the child will simply lack that message).
            let mut messages: Vec<coco_types::Message> = Vec::new();
            for (i, v) in config.fork_context_messages.iter().enumerate() {
                match serde_json::from_value::<coco_types::Message>(v.clone()) {
                    Ok(m) => messages.push(m),
                    Err(e) => {
                        tracing::warn!(
                            index = i,
                            error = %e,
                            "fork_context_messages[i] failed to deserialize; dropping"
                        );
                    }
                }
            }
            // Append the new user prompt after the fork history.
            messages.push(coco_messages::create_user_message(prompt));
            let (tx, _rx) = tokio::sync::mpsc::channel::<crate::CoreEvent>(16);
            engine.run_with_messages(messages, tx).await?
        } else {
            engine.run(prompt).await?
        };

        // Count ToolResult messages as a proxy for tool_use_count —
        // every committed tool_use produces exactly one tool_result
        // per I1, so this tracks TS `runAgent.ts`'s
        // `toolUseCount` increment on each assistant tool_use block.
        let tool_use_count = result
            .final_messages
            .iter()
            .filter(|m| matches!(m, coco_types::Message::ToolResult(_)))
            .count() as i64;
        // Serialize the final history so the caller (SwarmAgentHandle,
        // teammate runner) can route it through transcript / audit
        // pipelines. `serde_json::Value` is the agreed boundary type
        // on `AgentQueryResult` because this hop crosses the
        // `coco-tool` → `coco-state` layer.
        let messages = result
            .final_messages
            .iter()
            .map(|m| serde_json::to_value(m).unwrap_or_default())
            .collect();

        Ok(AgentQueryResult {
            response_text: Some(result.response_text),
            messages,
            turns: result.turns,
            input_tokens: result.total_usage.input_tokens,
            output_tokens: result.total_usage.output_tokens,
            tool_use_count,
            cancelled: result.cancelled,
        })
    }
}

#[cfg(test)]
#[path = "agent_adapter.test.rs"]
mod tests;
