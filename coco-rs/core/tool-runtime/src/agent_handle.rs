//! Agent handle trait — async agent operations abstraction for tools.
//!
//! TS: tools/AgentTool/AgentTool.tsx (spawn), tools/shared/spawnMultiAgent.ts (team)
//!
//! **Split design** (same pattern as SideQuery):
//! - Async trait (`AgentHandle`) -> here in `coco-tool`
//! - Implementations -> app/state or executor layer
//! - Tools access via `ToolUseContext.agent`
//!
//! **Dependency flow**:
//! ```text
//! coco-types         (AgentDefinition, AgentIsolation, SubagentType)
//!     |
//! coco-tool          (defines async AgentHandle trait, puts Arc<dyn> on ToolUseContext)
//!     |
//! coco-tools         (AgentTool/SendMessageTool/TeamCreate/TeamDelete call handle methods)
//!     |
//! coco-state         (implements AgentHandle using swarm infrastructure)
//!     |
//! coco-executor      (wires implementation into ToolUseContext)
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

use coco_types::AgentDefinition;
use coco_types::Features;
use coco_types::SubagentRuntimeSnapshot;
use coco_types::ToolFilter;
use coco_types::ToolOverrides;

/// Per-spawn safety constraints applied to a forked agent.
///
/// Surfaces parent-imposed limits the spawn pipeline must enforce on the
/// child — turn caps and write-path whitelists for sandboxed subagents
/// (e.g. memory extraction, auto-dream consolidation). Optional on
/// `AgentSpawnRequest`; absent = inherit parent's defaults.
///
/// TS: `services/extractMemories/extractMemories.ts:createAutoMemCanUseTool`
/// (path whitelist) + `MAX_TURNS = 5` (hard cap) — the two safety knobs
/// the extraction agent installs. Auto-dream uses the same shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpawnConstraints {
    /// Hard cap on agent turn count. Forked memory extraction uses 5.
    /// `None` defers to the engine's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,
    /// FileWrite / FileEdit / NotebookEdit on the child are restricted to
    /// paths that are descendants of one of these roots. Empty = no
    /// restriction. Tools enforce via `ToolUseContext::allowed_write_roots`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_write_roots: Vec<PathBuf>,
}

/// How the runner should construct the child agent's initial state.
///
/// TS parity: `forkSubagent.ts:isForkSubagentEnabled` — when fork is on
/// AND `subagent_type` is omitted, the runner switches from a fresh
/// child to a fork that inherits the parent's full conversation context
/// for prompt-cache sharing. The decision is taken by
/// [`coco_subagent::is_fork_subagent_active`] at the call site (which
/// also enforces coordinator-mode and non-interactive-session
/// short-circuits) and serialised into this enum.
///
/// Default is [`SpawnMode::Fresh`] so callers that don't opt in get the
/// unchanged spawn path.
///
/// `#[non_exhaustive]` — future variants (e.g. `Remote` for CCR
/// dispatch) will be added without a major version bump. Callers must
/// `match` with a wildcard arm or the explicit `Fresh` / `Fork` arms.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SpawnMode {
    /// Conventional subagent spawn — child gets a fresh conversation
    /// derived from its own `AgentDefinition.initial_prompt`.
    #[default]
    Fresh,
    /// Fork mode — child inherits the parent's pre-rendered system
    /// prompt bytes, parent message history, and parent tool pool.
    /// `tool_result` blocks in the inherited history are replaced with
    /// [`coco_subagent::FORK_PLACEHOLDER`] so all fork children produce a
    /// byte-identical API request prefix (prompt-cache sharing).
    ///
    /// The actual runner implementation lives in a future commit
    /// (deferred PR #3); the field carries the contract today so
    /// upstream tooling and IPC schemas stabilise early.
    Fork {
        /// Parent's already-rendered system-prompt bytes — must be
        /// threaded through verbatim, not re-rendered.
        rendered_system_prompt: Vec<u8>,
        /// Parent message history (cloned, not shared).
        parent_messages: Vec<serde_json::Value>,
        /// Whether the child should also inherit the parent's exact
        /// tool pool. TS sets this to true for cache-identical tool
        /// definitions.
        inherit_tool_pool: bool,
    },
    /// Resume — child rehydrates a previously-completed background spawn
    /// from its persisted JSONL transcript. The system prompt is built
    /// fresh from the agent definition (no parent prompt to inherit), and
    /// `tool_result` blocks in the prior history are kept verbatim
    /// (NO `FORK_PLACEHOLDER` rewriting — the child needs the real tool
    /// outputs to continue the conversation). TS:
    /// `tools/AgentTool/resumeAgent.ts::resumeAgentBackground` for
    /// non-fork agent types.
    Resume {
        /// Filtered prior message history. Caller (typically
        /// `SwarmAgentHandle::resume_agent`) is expected to have already
        /// run `coco_subagent::filter_transcript` to drop unresolved
        /// tool uses + orphaned thinking + whitespace-only assistants.
        parent_messages: Vec<serde_json::Value>,
    },
}

/// Request to spawn a subagent.
///
/// TS: AgentToolInput in AgentTool.tsx
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentSpawnRequest {
    /// The task/instruction for the agent.
    pub prompt: String,
    /// Short (3-5 word) description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Agent type to use (e.g., "Explore", "Plan", "general-purpose").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    /// Model override (e.g., "sonnet", "opus", "haiku").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Run in background (fire-and-forget).
    #[serde(default)]
    pub run_in_background: bool,
    /// Whether the spawn should run periodic AgentSummary timers
    /// (TS parity: `AgentTool.tsx:750`'s `enableSummarization`).
    /// Computed at the AgentTool boundary as `is_coordinator_mode
    /// || is_fork_subagent_active || ctx.app_state.agent_progress_summaries_enabled`
    /// so the coordinator (which doesn't see `ctx.app_state`) can
    /// honour the SDK-level opt-in without re-discovering it.
    #[serde(default)]
    pub enable_summarization: bool,
    /// Parent session id — used by the background dispatch path to
    /// scope per-agent transcript / metadata persistence
    /// (`<sessions_dir>/<session_id>/subagents/agent-<id>.*`).
    /// Filled at the AgentTool boundary from
    /// `ctx.session_id_for_history`. Empty when the session id
    /// isn't available (tests / minimal embedding) — persistence
    /// is then a no-op.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session_id: String,
    /// Isolation mode ("worktree" or "remote").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    /// Agent name (for multi-agent teams).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Team name (triggers teammate spawn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    /// Permission mode override (e.g., "plan").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Working directory override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Reasoning effort override (TS `AgentTool.tsx` `effort` input).
    /// Maps to the engine's thinking-level configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Use exact (cache-identical) parent tool definitions instead of
    /// re-rendering the agent's own pool. TS `runAgent.ts:624` —
    /// preserves prompt-cache prefix.
    #[serde(default)]
    pub use_exact_tools: bool,
    /// Per-agent MCP server allow-list. TS `runAgent.ts:50+`,
    /// `AgentTool.tsx:206` — when non-empty, only these MCP servers'
    /// tools are exposed to the child.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<String>,
    /// Per-agent tool deny-list. TS `agentToolUtils.ts:122-160`. Layer
    /// 4 of the filter pipeline; intersected with `parent_tool_filter`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disallowed_tools: Vec<String>,
    /// Hard cap on agent turns. TS `runAgent.ts:624`. `None` = engine
    /// default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,
    /// Inline initial-message text override. TS `loadAgentsDir.ts`
    /// `initial_prompt` field — when set, replaces the default
    /// agent-definition prompt body. Useful for one-off subagent
    /// spawns that don't match a registered agent type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_prompt: Option<String>,
    /// Parent's resolved feature gates, threaded through so the
    /// subagent runs with the same Layer 1 set. Skipped at the JSON
    /// boundary; the parent fills it in-process before handing off.
    /// Subagents only narrow this — never widen.
    #[serde(skip)]
    pub features: Option<Arc<Features>>,
    /// Parent's resolved Layer 2 tool overrides. Same in-process-only
    /// inheritance as `features`. Falling back to `ToolOverrides::none()`
    /// would expose tools the active model rejects, so callers must
    /// thread the parent's value through.
    #[serde(skip)]
    pub tool_overrides: Option<Arc<ToolOverrides>>,
    /// Parent's resolved Layer 4 tool filter. The subagent's own
    /// allow/deny (from `AgentDefinition`) narrows this further via
    /// `ToolFilter::narrow_with`, so a child's `allowed_tools` can
    /// never widen what the parent already restricted. Skipped at the
    /// JSON boundary for the same reason as the other inheritance
    /// fields.
    #[serde(skip)]
    pub parent_tool_filter: Option<ToolFilter>,
    /// Per-spawn safety constraints (turn cap, write-path whitelist).
    /// Used by the memory crate's forked extraction / auto-dream
    /// agents to install a 5-turn cap and memdir-only write fence.
    /// `None` = no extra constraints beyond the engine's defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<AgentSpawnConstraints>,
    /// Parent conversation slice prepended to the child's first turn
    /// when `isolation == Some("fork")`. Each entry is a serialized
    /// `coco_messages::Message` JSON value. Carried as
    /// `serde_json::Value` so the boundary doesn't pull message types
    /// into `coco-tool-runtime`. TS: `AgentTool.tsx:622-632`
    /// `forkContextMessages`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fork_context_messages: Vec<serde_json::Value>,
    /// How to construct the child's initial state. Defaults to
    /// [`SpawnMode::Fresh`]; switched to [`SpawnMode::Fork`] by the
    /// AgentTool callsite when `coco_subagent::is_fork_subagent_active`
    /// returns true and `subagent_type` is omitted (TS parity with
    /// `forkSubagent.ts`).
    #[serde(default)]
    pub spawn_mode: SpawnMode,
    /// Snapshot of the parent's resolved provider+model identity at
    /// spawn time. The runner reads this to detect drift after
    /// `RuntimeConfig` hot-reload and to enforce Fork-mode prompt-cache
    /// parity. Skipped at the JSON boundary — purely an in-process
    /// inheritance hint. `None` means "no parent identity available;
    /// resolve from current runtime" (the legacy/test path).
    ///
    /// Populated by `AgentTool::execute` from the parent's `ApiClient`
    /// fingerprint (via `coco_inference::ProviderClientFingerprint`) at
    /// the production call site once the runtime threads it through
    /// `ToolUseContext`. See `coco_types::SubagentRuntimeSnapshot` for
    /// the full contract and rationale.
    #[serde(skip)]
    pub parent_runtime_snapshot: Option<SubagentRuntimeSnapshot>,
    /// Resolved agent definition for this spawn — when the user
    /// supplies `subagent_type`, `AgentTool::execute` looks the
    /// definition up in `ToolUseContext.agent_catalog` and threads it
    /// through here. The runner reads `definition.model` and
    /// `definition.model_role` via
    /// [`coco_subagent::resolve_subagent_selection`] so the user's
    /// `.md` file actually steers spawn-time identity. Skipped at the
    /// JSON boundary — definitions are resolved per-process and not
    /// portable across runners. `None` falls back to the
    /// `subagent_type → ModelRole` mapping alone.
    #[serde(skip)]
    pub definition: Option<Arc<AgentDefinition>>,
}

/// Response from spawning a subagent.
///
/// TS: AgentTool call result variants
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentSpawnResponse {
    /// Outcome of the spawn.
    pub status: AgentSpawnStatus,
    /// Agent identifier (for async/team agents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Result text (for completed sync agents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Error message (for failed spawns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Total tool uses by the agent.
    #[serde(default)]
    pub total_tool_use_count: i64,
    /// Total tokens consumed (input + output).
    #[serde(default)]
    pub total_tokens: i64,
    /// Input tokens consumed by the agent (prompt + context).
    /// `0` when the underlying engine doesn't report them separately.
    #[serde(default)]
    pub input_tokens: i64,
    /// Output tokens generated by the agent.
    /// `0` when the underlying engine doesn't report them separately.
    #[serde(default)]
    pub output_tokens: i64,
    /// Per-tool invocation counts (e.g. `Write → 3`, `Read → 7`).
    /// Memory telemetry uses this to count `files_written` for the
    /// extraction agent without re-parsing the agent's transcript.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub tool_use_counts: std::collections::HashMap<String, i64>,
    /// Duration in milliseconds.
    #[serde(default)]
    pub duration_ms: i64,
    /// Worktree path (if isolation was used).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<PathBuf>,
    /// Worktree branch name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    /// Output file path for background agents.
    /// TS: getTaskOutputPath(agentId) — returned in async_launched responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_file: Option<PathBuf>,
    /// The original prompt (echoed back in response).
    /// TS: AgentTool output includes prompt field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// Outcome of a spawn request.
///
/// TS: AgentTool return status variants
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSpawnStatus {
    /// Synchronous agent completed successfully.
    Completed,
    /// Background agent launched (poll for result).
    AsyncLaunched,
    /// Teammate spawned in a team.
    TeammateSpawned,
    /// Agent spawn failed. Default so callers that build a response
    /// incrementally start from a safe-by-default state.
    #[default]
    Failed,
}

/// Trait for agent operations from tools.
///
/// Implementations live in the app/state or executor layer. Tools access
/// this via `ToolUseContext.agent`.
#[async_trait::async_trait]
pub trait AgentHandle: Send + Sync {
    /// Spawn a subagent (sync or async).
    ///
    /// TS: AgentTool.call() in AgentTool.tsx
    async fn spawn_agent(&self, request: AgentSpawnRequest) -> Result<AgentSpawnResponse, String>;

    /// Send a message to another agent by name or ID.
    /// Use `"*"` as target to broadcast to all teammates.
    ///
    /// Content may be a plain text string or a serialized structured
    /// message (shutdown_request, shutdown_response, plan_approval_response).
    ///
    /// TS: SendMessageTool routing via agent_name_registry
    async fn send_message(&self, to: &str, content: &str) -> Result<String, String>;

    /// Create a new team with optional description and lead agent type.
    ///
    /// TS: TeamCreateTool → TeamFile creation + AppState update
    async fn create_team(&self, name: &str) -> Result<String, String>;

    /// Delete the active team (read from session context) and release
    /// resources. Fails if non-lead members are still active.
    ///
    /// TS parity: `TeamDeleteTool.ts:21` declares `z.strictObject({})`
    /// — the team name is taken from `appState.teamContext?.teamName`,
    /// not tool input. Implementations should read their own session
    /// state to resolve the team. Returns a human-readable message.
    ///
    /// TS: TeamDeleteTool → cleanup + AppState clear
    async fn delete_team(&self) -> Result<String, String>;

    /// Resume a previously-completed background AgentTool spawn from
    /// its persisted transcript + metadata sidecar. Triggered by
    /// [`SendMessageTool`] when the target is a stopped task (TS
    /// parity: `SendMessageTool.ts:822-844`'s auto-resume path).
    ///
    /// `session_id` scopes the per-agent transcript / metadata
    /// lookup. `prompt` becomes the new user message that drives
    /// the resumed turn.
    ///
    /// Default impl returns an error so legacy handles (no-op /
    /// test stubs) don't need to override.
    async fn resume_agent(
        &self,
        _agent_id: &str,
        _prompt: &str,
        _session_id: &str,
    ) -> Result<AgentSpawnResponse, String> {
        Err("AgentHandle::resume_agent not supported in this context".into())
    }

    /// Query the status of a background agent.
    ///
    /// Returns the agent's current status and result if completed.
    /// TS: checkAgentStatus() in LocalAgentTask
    async fn query_agent_status(&self, agent_id: &str) -> Result<AgentSpawnResponse, String>;

    /// Get the output of a completed background agent.
    ///
    /// TS: getAgentOutput() — reads the output file from a completed agent.
    async fn get_agent_output(&self, agent_id: &str) -> Result<String, String>;

    /// Signal that a foreground agent should move to background execution.
    ///
    /// The agent continues running but unblocks the parent turn.
    /// TS: backgroundSignal + wasBackgrounded logic in AgentTool.tsx
    async fn background_agent(&self, agent_id: &str) -> Result<(), String>;

    // Note: `resolve_skill` was removed in Phase 7 of the agent-loop
    // refactor. Skill resolution now goes through the dedicated
    // `SkillHandle` trait (`skill_handle.rs`); `AgentHandle` is the
    // wrong abstraction for it. See the refactor plan's
    // SkillRuntime section.
}

/// Shared handle type for `ToolUseContext`.
pub type AgentHandleRef = Arc<dyn AgentHandle>;

/// A no-op implementation that returns errors. Used in test/stub contexts.
#[derive(Debug, Clone)]
pub struct NoOpAgentHandle;

#[async_trait::async_trait]
impl AgentHandle for NoOpAgentHandle {
    async fn spawn_agent(&self, _request: AgentSpawnRequest) -> Result<AgentSpawnResponse, String> {
        Err("Agent spawning not available in this context".into())
    }

    async fn send_message(&self, _to: &str, _content: &str) -> Result<String, String> {
        Err("Agent messaging not available in this context".into())
    }

    async fn create_team(&self, _name: &str) -> Result<String, String> {
        Err("Team management not available in this context".into())
    }

    async fn delete_team(&self) -> Result<String, String> {
        Err("Team management not available in this context".into())
    }

    // `resume_agent` uses the trait-level default impl that returns
    // `Err("not supported in this context")`. NoOp doesn't override.

    async fn query_agent_status(&self, _agent_id: &str) -> Result<AgentSpawnResponse, String> {
        Err("Agent status query not available in this context".into())
    }

    async fn get_agent_output(&self, _agent_id: &str) -> Result<String, String> {
        Err("Agent output not available in this context".into())
    }

    async fn background_agent(&self, _agent_id: &str) -> Result<(), String> {
        Err("Agent backgrounding not available in this context".into())
    }
}
