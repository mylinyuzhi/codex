//! Agent query execution trait — drives multi-turn LLM conversations for agents.
//!
//! TS: tools/AgentTool/runAgent.ts (248 lines)
//!
//! **Split design** (same pattern as SideQuery, AgentHandle):
//! - Trait definition → here in `coco-tool`
//! - Implementation → `coco-query` (QueryEngine-based adapter)
//! - Consumer → `coco-state` (swarm_runner_loop uses it to drive teammate loops)
//!
//! **Dependency flow**:
//! ```text
//! coco-tool    (defines AgentQueryEngine trait)
//!     ↓
//! coco-query   (QueryEngine implements trait via adapter)
//!     ↓
//! coco-state   (InProcessTeammateRunner uses Arc<dyn AgentQueryEngine>)
//! ```

use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

/// Configuration for a single agent query turn.
///
/// TS: Parameters passed to runAgent() in runAgent.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentQueryConfig {
    /// System prompt for the agent.
    pub system_prompt: String,
    /// Model to use for inference.
    pub model: String,
    /// Maximum turns for this query.
    pub max_turns: Option<i32>,
    /// Context window size (tokens). Defaults to model's max.
    #[serde(default)]
    pub context_window: Option<i64>,
    /// Maximum output tokens per turn. Defaults to model's max.
    #[serde(default)]
    pub max_output_tokens: Option<i64>,
    /// Tools available to the agent (names).
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Whether to preserve tool use results across compaction.
    #[serde(default)]
    pub preserve_tool_use_results: bool,
}

/// Result of a multi-turn agent query.
///
/// TS: Return value from runAgent() generator + finalizeAgentTool()
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentQueryResult {
    /// Final response text from the agent.
    pub response_text: Option<String>,
    /// Conversation messages produced during the query.
    #[serde(default)]
    pub messages: Vec<serde_json::Value>,
    /// Number of turns executed.
    pub turns: i32,
    /// Input tokens consumed.
    pub input_tokens: i64,
    /// Output tokens produced.
    pub output_tokens: i64,
    /// Number of tool invocations.
    pub tool_use_count: i64,
    /// Whether the agent was cancelled.
    #[serde(default)]
    pub cancelled: bool,
}

/// Trait for executing multi-turn agent queries.
///
/// Implementations drive the LLM conversation loop:
/// prompt → model → tool calls → tool results → repeat.
///
/// TS: runAgent() async generator in runAgent.ts
#[async_trait::async_trait]
pub trait AgentQueryEngine: Send + Sync {
    /// Execute a multi-turn agent query.
    ///
    /// Runs the prompt through the LLM, executes tool calls,
    /// and loops until the model stops or max_turns is reached.
    async fn execute_query(
        &self,
        prompt: &str,
        config: AgentQueryConfig,
    ) -> anyhow::Result<AgentQueryResult>;
}

/// Shared handle type for dependency injection.
pub type AgentQueryEngineRef = Arc<dyn AgentQueryEngine>;

/// No-op implementation for testing.
#[derive(Debug, Clone)]
pub struct NoOpAgentQueryEngine;

#[async_trait::async_trait]
impl AgentQueryEngine for NoOpAgentQueryEngine {
    async fn execute_query(
        &self,
        _prompt: &str,
        _config: AgentQueryConfig,
    ) -> anyhow::Result<AgentQueryResult> {
        anyhow::bail!("Agent query execution not available in this context")
    }
}
