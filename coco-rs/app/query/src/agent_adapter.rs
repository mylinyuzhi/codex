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

use crate::engine::QueryEngine;
use crate::engine::QueryEngineConfig;

/// Factory function type for creating QueryEngine instances.
///
/// Each agent query gets a fresh engine with its own config.
/// The factory captures shared state (API client, tool registry, hooks).
pub type QueryEngineFactory = Arc<dyn Fn(QueryEngineConfig) -> QueryEngine + Send + Sync>;

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
        let engine_config = QueryEngineConfig {
            max_turns: config.max_turns.unwrap_or(30),
            max_tokens: None,
            system_prompt: Some(config.system_prompt),
            append_system_prompt: None,
            model_name: config.model,
            fallback_model: None,
            permission_mode: coco_types::PermissionMode::Default,
            context_window: config.context_window.unwrap_or(200_000),
            max_output_tokens: config.max_output_tokens.unwrap_or(16_384),
            max_budget_usd: None,
            streaming_tool_execution: true,
            is_non_interactive: true,
            session_id: String::new(),
            project_dir: None,
            disable_all_hooks: false,
            allow_managed_hooks_only: false,
        };

        let engine = (self.engine_factory)(engine_config);
        let result = engine.run(prompt).await?;

        Ok(AgentQueryResult {
            response_text: Some(result.response_text),
            messages: Vec::new(),
            turns: result.turns,
            input_tokens: result.total_usage.input_tokens,
            output_tokens: result.total_usage.output_tokens,
            tool_use_count: 0,
            cancelled: result.cancelled,
        })
    }
}

#[cfg(test)]
#[path = "agent_adapter.test.rs"]
mod tests;
