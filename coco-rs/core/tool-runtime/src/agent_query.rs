//! Agent query execution trait — drives multi-turn LLM conversations for agents.
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

use coco_messages::Message;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// Configuration for a single agent query turn.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AgentQueryConfig {
    /// System prompt for the agent.
    pub system_prompt: String,
    /// Model to use for inference.
    pub model: String,
    /// Typed runtime model selection. New callers should set this
    /// instead of relying on `model`/`model_role`; those legacy fields
    /// are still accepted at the adapter boundary for older IPC/tests.
    #[serde(default)]
    pub model_selection: coco_types::LlmModelSelection,
    /// Optional cancellation token for this agent query turn. In-process
    /// teammates use a fresh token per prompt so interrupting current
    /// work does not kill the teammate lifecycle.
    #[serde(skip)]
    pub cancel: Option<CancellationToken>,
    /// Maximum turns for this query.
    pub max_turns: Option<i32>,
    /// Context window size (tokens). Defaults to model's max.
    #[serde(default)]
    pub context_window: Option<i64>,
    /// Prompt-cache directive inherited from a parent fork context.
    /// Fork callers preserve the parent's cache-key fields and only set
    /// `skip_cache_write` for fire-and-forget runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<coco_types::PromptCacheConfig>,
    /// Maximum output tokens per turn. Defaults to model's max.
    #[serde(default)]
    pub max_output_tokens: Option<i64>,
    /// Tools available to the agent (names). Empty = inherit parent's
    /// filter; non-empty = subagent restricted to this set. This is a
    /// **registry filter** — tools outside the set are hidden from the
    /// model. Used by `AgentTool` / coordinator / teammate spawners
    /// where the parent intends to narrow the visible toolset.
    ///
    /// Fork-mode skills DO NOT populate this. They go through
    /// [`AgentQueryConfig::extra_permission_rules`] instead (which adds to
    /// `alwaysAllowRules.command` without narrowing tools[]).
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Tools explicitly denied to the agent regardless of allow-list.
    /// Sourced from `AgentDefinition.disallowed_tools`.
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
    /// Runtime permission rules folded into the subagent's
    /// `ToolPermissionContext.{allow,deny,ask}_rules` at fork-engine
    /// construction. Skipped at the JSON boundary because the rule
    /// values carry runtime-only metadata (`PermissionRule` is
    /// `Clone`, but cross-process serialization would round-trip the
    /// rule strings via a different path).
    ///
    #[serde(skip)]
    pub extra_permission_rules: Vec<coco_types::PermissionRule>,
    /// Live teammate permission rules, read by the query engine when it
    /// builds each tool context. Used by agent-team control messages so
    /// team-level allow/deny/ask updates apply to an in-flight turn
    /// without restarting the teammate.
    #[serde(skip)]
    pub live_permission_rules: Option<Arc<RwLock<Vec<coco_types::PermissionRule>>>>,
    /// Live teammate permission mode, read by the query engine when it
    /// builds each tool context (read at permission-check time).
    #[serde(skip)]
    pub live_permission_mode: Option<Arc<RwLock<coco_types::PermissionMode>>>,
    /// Layer 2 tool overrides inherited from the parent. The subagent
    /// **never** widens this set — `ToolOverrides::none()` would
    /// expose tools the active model doesn't actually accept. Skipped
    /// at the JSON boundary; the parent fills it in-process before
    /// handing off.
    #[serde(skip)]
    pub tool_overrides: Option<std::sync::Arc<coco_types::ToolOverrides>>,
    /// Layer 1 feature gates inherited from the parent. Defaults to
    /// `with_defaults()` only when the caller doesn't thread the
    /// parent value through, which silently re-enables features the
    /// user disabled at the top level. Skipped at the JSON boundary
    /// for the same reason as `tool_overrides`.
    #[serde(skip)]
    pub features: Option<std::sync::Arc<coco_types::Features>>,
    /// Parent's `skill_overrides` tiers, threaded so subagents apply
    /// the same listing + Skill tool gates. Falls back to
    /// default-empty tiers (every skill on) when not threaded.
    #[serde(skip)]
    pub skill_overrides: Option<std::sync::Arc<coco_config::SkillOverrideTiers>>,
    /// Parent's Layer 4 tool filter. The adapter intersects this with
    /// the subagent's own `allowed_tools` / `disallowed_tools` via
    /// [`coco_types::ToolFilter::narrow_with`] so the child can never
    /// widen what the parent restricted. Skipped at the JSON boundary
    /// for the same reason as the other inheritance fields.
    #[serde(skip)]
    pub parent_tool_filter: Option<coco_types::ToolFilter>,
    /// Whether to preserve tool use results across compaction.
    #[serde(default)]
    pub preserve_tool_use_results: bool,
    /// Permission mode resolved by the parent (main agent) via the
    /// inheritance rule: `agent.permission_mode if set and parent ∉
    /// {bypass, acceptEdits, auto} else parent.permission_mode`. When
    /// `None`, the adapter defaults to `PermissionMode::Default`.
    #[serde(default)]
    pub permission_mode: Option<coco_types::PermissionMode>,
    /// Branded agent ID. Subagents get per-agent
    /// plan files `{slug}-agent-{id}.md`; setting this flows through to
    /// `ToolUseContext::agent_id` and `session_plan_file` so the Plan-mode
    /// auto-allow + SubAgent reminder variant trigger correctly.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Whether this agent runs as a swarm teammate (spawned via
    /// `TeamCreate`). Controls ExitPlanMode teammate
    /// branch + bypass-permission behavior.
    #[serde(default)]
    pub is_teammate: bool,
    #[serde(default)]
    pub is_in_process_teammate: bool,
    /// Per-role `plan_mode_required` flag.
    /// Controls whether this teammate's ExitPlanMode sends an approval
    /// request to the leader (required) or exits locally (voluntary).
    #[serde(default)]
    pub plan_mode_required: bool,
    /// Session ID for plan file + history scoping. Subagents share the
    /// parent's session_id so `{slug}-agent-{id}.md` resolves against the
    /// same slug cache.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Parent session's bypass-permissions capability. In-process subagents
    /// inherit the parent's capability through this field instead of argv
    /// forwarding — the engine threads it into
    /// `ToolPermissionContext.bypass_available` so child plan-mode
    /// auto-allow and Shift+Tab cycle behave consistently with the
    /// parent. Defaults to `false` so legacy callers stay safe.
    #[serde(default)]
    pub bypass_permissions_available: bool,
    /// Working-directory override for this subagent. Set by worktree
    /// isolation to the freshly-created worktree path, or by explicit
    /// `cwd:` tool input. Child `ToolUseContext.cwd_override` reads
    /// this; relative-path-resolving tools (Glob, Grep, Bash) scope
    /// to the override, absolute-path tools ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd_override: Option<std::path::PathBuf>,
    /// Fork-mode context messages: parent's history prepended to
    /// the child's turn. When non-empty, child runs with
    /// `forkContextMessages` + this prompt. Shared via `Arc<Message>` so the
    /// in-process spawn path doesn't pay a serialize → Value →
    /// deserialize round-trip; cross-process transports serialize
    /// once at the wire boundary instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fork_context_messages: Vec<Arc<Message>>,
    /// Which `ModelRole` this subagent runs under. The adapter
    /// resolves the role's primary + fallback chain from
    /// `ModelRoles` and installs them on the child engine so the
    /// subagent inherits the same capacity-resilience policy its
    /// role is configured with. `None` defers to the factory's
    /// default (typically `ModelRole::Main` or the parent's role).
    ///
    /// Non-Main roles (Explore / Review / Plan) only get a
    /// fallback chain when the role is explicitly configured in
    /// `settings.models.{role}.fallbacks`. No Main-walk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_role: Option<coco_types::ModelRole>,
    /// FileWrite / FileEdit / NotebookEdit are restricted to paths under one
    /// of these roots. Empty = no restriction. Threaded into the child's
    /// `ToolUseContext::allowed_write_roots` so file-mutation tools can
    /// reject out-of-fence paths before they hit disk. Used by memory
    /// extraction / auto-dream subagents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_write_roots: Vec<std::path::PathBuf>,
    /// Reasoning-effort override forwarded from the AgentTool input.
    /// Maps to the engine's
    /// thinking-level configuration. `None` falls back to the
    /// model-role default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<coco_types::ReasoningEffort>,
    /// When true, bypass agent-definition tool rendering and use the
    /// parent's exact tool schemas verbatim (`useExactTools`). Required for prompt-cache prefix sharing in
    /// fork-style spawns.
    #[serde(default)]
    pub use_exact_tools: bool,
    /// Per-agent MCP server allow-list. When non-empty, only tools from these
    /// servers are exposed to the child. Empty = no MCP restriction
    /// from this layer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<String>,
    /// Inline initial-message body override (`initial_prompt`). When set, replaces the
    /// agent-definition's stored prompt body — useful for ad-hoc
    /// subagent spawns that don't match a registered definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_prompt: Option<String>,
    /// Resolved agent definition for this query — threaded from
    /// `AgentSpawnRequest.definition`. The adapter (and downstream
    /// engine factory) consult `definition.model` /
    /// `definition.model_role` for spawn-time identity decisions, and
    /// `definition.system_prompt` / `definition.initial_prompt` for
    /// content. Skipped at the JSON boundary.
    #[serde(skip)]
    pub definition: Option<Arc<coco_types::AgentDefinition>>,
    /// Optional per-spawn permission bridge (D3). When present, the
    /// adapter installs it on the child engine via
    /// `with_permission_bridge`, replacing any parent-inherited
    /// bridge. Worker spawns set this to a [`MailboxPermissionBridge`]
    /// (cross-process) or in-process equivalent so a worker's
    /// permission-deny path forwards to the team leader instead of
    /// failing closed.
    ///
    /// Skipped at the JSON boundary — `Arc<dyn Trait>` doesn't
    /// serialise, and permission routing is purely in-process.
    #[serde(skip)]
    pub permission_bridge: Option<crate::permission_bridge::ToolPermissionBridgeRef>,
    /// Optional `CoreEvent` sink the engine writes to during the
    /// child query. Used by the AgentTool background path to stream
    /// `Stream::TextDelta` events into a per-task output buffer so
    /// `TaskOutput` returns mid-flight text instead of just the
    /// final response. `None` ⇒ events are emitted into a discarded
    /// channel (existing behaviour). Skipped at the JSON boundary
    /// — `mpsc::Sender` doesn't serialise.
    #[serde(skip)]
    pub event_tx: Option<tokio::sync::mpsc::Sender<coco_types::CoreEvent>>,

    /// Per-fork tool-execution gate. Threaded onto the child engine's
    /// `ToolUseContext.can_use_tool` so app/query enforces the policy
    /// before the static permission evaluator. `None` preserves
    /// existing behavior — no callback runs.
    #[serde(skip)]
    pub can_use_tool: Option<crate::can_use_tool::CanUseToolHandleRef>,

    /// When `true`, hook auto-approve cannot bypass the `can_use_tool`
    /// callback (`requireCanUseTool`).
    #[serde(default)]
    pub require_can_use_tool: bool,

    /// Typed fork discriminator for telemetry / log structured fields.
    /// When set, the engine's `query_source_label()` returns this
    /// string so log readers tell apart the 9 fork variants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_label: Option<coco_types::ForkLabel>,

    /// Live message-history sink for the periodic AgentSummary timer.
    /// When `Some`, the child engine publishes its full message history
    /// into this handle after every turn finalize so the timer summarizes
    /// the real transcript instead of the raw output buffer. `None` ⇒ the engine skips the
    /// per-turn snapshot entirely (zero cost on the non-summarized path).
    /// In-process only — the shared buffer is meaningless across IPC, so it
    /// is skipped at the JSON boundary like [`Self::event_tx`].
    #[serde(skip)]
    pub live_transcript: Option<crate::LiveTranscript>,
}

/// Result of a multi-turn agent query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentQueryResult {
    /// Final response text from the agent.
    pub response_text: Option<String>,
    /// Conversation messages produced during the query. Carried as
    /// `Arc<Message>` so the in-process subagent path returns its
    /// final history without a deep clone or JSON round-trip;
    /// remote transports serialize at the wire boundary.
    #[serde(default)]
    pub messages: Vec<Arc<Message>>,
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
    ) -> Result<AgentQueryResult, coco_error::BoxedError>;
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
    ) -> Result<AgentQueryResult, coco_error::BoxedError> {
        Err(Box::new(coco_error::PlainError::new(
            "Agent query execution not available in this context",
            coco_error::StatusCode::Internal,
        )))
    }
}
