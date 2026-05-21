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

use coco_messages::Message;
use coco_types::AgentDefinition;
use coco_types::Features;
use coco_types::SubagentRuntimeSnapshot;
use coco_types::ToolFilter;
use coco_types::ToolOverrides;

use crate::task_list_handle::TeamTaskListRouterRef;

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
///
/// **No `Serialize`/`Deserialize`** — `Fork` carries
/// `Arc<SubagentRuntimeSnapshot>` which is meaningless across an IPC
/// boundary (the receiving runtime has its own snapshot). The field is
/// `#[serde(skip)]` on [`AgentSpawnRequest`] so the wire form ignores
/// it entirely.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub enum SpawnMode {
    /// Conventional subagent spawn — child gets a fresh conversation
    /// derived from its own `AgentDefinition.initial_prompt`.
    #[default]
    Fresh,
    /// Fork mode — child inherits the parent's pre-rendered system
    /// prompt, parent message history, and parent tool pool.
    /// `tool_result` blocks in the inherited history are replaced with
    /// [`coco_subagent::FORK_PLACEHOLDER`] so all fork children produce a
    /// byte-identical API request prefix (prompt-cache sharing).
    ///
    /// Runner: [`coco_coordinator::agent_handle::spawn::spawn_subagent`]
    /// matches on this variant and threads `rendered_system_prompt`
    /// into `AgentQueryConfig.system_prompt` verbatim, wraps
    /// `request.prompt` with [`coco_subagent::build_fork_child_message`]
    /// for `<fork-boilerplate>` recursion-detection, and rewrites the
    /// `parent_messages` `tool_result` blocks via
    /// [`coco_subagent::build_fork_context`].
    ///
    /// Tool-pool inheritance is decided by
    /// [`AgentSpawnRequest::use_exact_tools`] (TS
    /// `runAgent.ts:624 cacheIdenticalTools`); fork mode does NOT
    /// carry its own toggle.
    Fork {
        /// Parent's already-rendered system prompt — threaded through
        /// verbatim, not re-rendered. `String` (not `Vec<u8>`) because
        /// the wire form is always UTF-8 text; converting to bytes
        /// would only invite a fallible roundtrip that hides
        /// corruption behind `unwrap_or_default`.
        rendered_system_prompt: String,
        /// Parent message history. Shared via `Arc<Message>` so the
        /// fork-context build only rewrites tool-result bodies; the
        /// rest of the slice is a cheap Arc clone of the parent's
        /// authoritative history.
        parent_messages: Vec<Arc<Message>>,
        /// Parent's resolved provider+model identity at the moment of
        /// fork. **Non-optional by design** — fork mode's entire
        /// purpose is prompt-cache parity, which requires sending a
        /// byte-identical request prefix. That parity requires
        /// pinning to the parent's exact `(provider, api, model_id,
        /// base_url, wire_api)` regardless of what
        /// `RuntimeConfig::resolve_model_roles()` would return now.
        ///
        /// The spawn path uses this to populate the env block AND
        /// the `AgentQueryConfig.model` for the actual API call;
        /// reading live runtime config would break cache parity.
        parent_snapshot: Arc<SubagentRuntimeSnapshot>,
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
        parent_messages: Vec<Arc<Message>>,
    },
}

/// Request to spawn a subagent.
///
/// TS: AgentToolInput in AgentTool.tsx
///
/// **Deferred refactor — split into 4 sub-structs**: the type
/// currently carries 27 fields covering four distinct concerns
/// (model-visible input, spawn-mode identity, policy/inheritance,
/// routing/telemetry). The plan is to nest these under
/// `AgentSpawnInput`/`AgentSpawnIdentity`/`AgentSpawnPolicy`/
/// `AgentSpawnRouting` so each construction site doesn't navigate a
/// 27-field flat literal. Deferred because the cascade touches
/// every `request.X` read across `coordinator/agent_handle/*` and
/// `memory/service/{extract,dream,session}.rs` (≥ 50 sites), and the
/// refactor is pure code-quality with no TS-behavior delta — best
/// landed as its own focused PR. Tracked in
/// `core/tool-runtime/CLAUDE.md` "Deferred refactors".
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
    // `model` and `model_role` deliberately ABSENT from this struct.
    // Both are operator-only static configuration:
    //   - Per-agent: `.md` frontmatter `model:` / `model_role:` on
    //     `AgentDefinition` — resolved at spawn time by
    //     `coco_subagent::resolve_subagent_selection` reading from
    //     `request.definition`.
    //   - Internal-fork override: `AgentSpawnConstraints.forced_model_role`
    //     (memory crate uses this to pin `ModelRole::Memory` on
    //     extract / dream / session-memory forks).
    //
    // The LLM cannot pick either of these — AgentTool's
    // `input_schema()` doesn't expose them. Catalog-only principle:
    // static configuration is the source of truth for model routing.
    // See the root CLAUDE.md "Multi-Provider Boundaries" rule.
    /// Run in background (fire-and-forget).
    #[serde(default)]
    pub run_in_background: bool,
    /// Auto-detach a foreground spawn after N milliseconds. When set
    /// to `Some(d)` and `run_in_background == false`, the runtime
    /// fires [`crate::TaskController::signal_detach`] after `d` ms of
    /// foreground execution; the parent's awaiter unblocks with
    /// `AsyncLaunched` and the engine keeps running detached.
    ///
    /// TS parity: `AgentTool.tsx:826` passes `autoBackgroundMs:
    /// getAutoBackgroundMs() || undefined` into
    /// `registerAgentForeground`. `getAutoBackgroundMs` returns
    /// `120_000` ms when `CLAUDE_AUTO_BACKGROUND_TASKS` env or the
    /// `tengu_auto_background_agents` GrowthBook flag is on, else `0`
    /// (disabled).
    ///
    /// `None` = no auto-detach (the default; only explicit user-initiated
    /// `signal_detach` will background the task).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_background_ms: Option<u64>,
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
    // Note: the following fields are NOT on `AgentSpawnRequest`. They
    // were dead pass-through slots — TS doesn't expose them in the
    // AgentTool input schema, and no Rust caller ever set them; the
    // coordinator now reads them directly from `AgentDefinition` via
    // `request.definition` when building RunnerConfig / QueryConfig.
    // Single source of truth, no shadowing.
    //
    // - `effort` → `AgentDefinition.effort`
    // - `use_exact_tools` → `AgentDefinition.use_exact_tools`
    // - `mcp_servers` → `AgentDefinition.mcp_servers` (mapped via
    //   `AgentMcpServerSpec::name()`)
    // - `disallowed_tools` → `AgentDefinition.disallowed_tools`
    // - `max_turns` → `AgentDefinition.max_turns` (or
    //   `AgentSpawnConstraints.max_turns` when the constraints layer
    //   provides a tighter cap — memory forks set this)
    // - `initial_prompt` → `AgentDefinition.initial_prompt`
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
    /// when `isolation == Some("fork")`. Shared via `Arc<Message>`
    /// — in-process spawns reuse parent allocations; remote transports
    /// serialize once at the wire boundary. TS: `AgentTool.tsx:622-632`
    /// `forkContextMessages`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fork_context_messages: Vec<Arc<Message>>,
    /// How to construct the child's initial state. Defaults to
    /// [`SpawnMode::Fresh`]; switched to [`SpawnMode::Fork`] by the
    /// AgentTool callsite when `coco_subagent::is_fork_subagent_active`
    /// returns true and `subagent_type` is omitted (TS parity with
    /// `forkSubagent.ts`).
    ///
    /// **Skipped at the JSON boundary** because the runtime form holds
    /// `Arc<SubagentRuntimeSnapshot>` inside `Fork`, which is
    /// meaningless across IPC. AgentTool reconstructs the right
    /// variant on the receiving side from in-process state.
    #[serde(skip)]
    pub spawn_mode: SpawnMode,
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
    /// Suppress per-message transcript persistence for this spawn.
    /// TS parity (`utils/forkedAgent.ts` `runForkedAgent({skipTranscript:
    /// true})` — used by extract/auto-dream/session-memory forks so
    /// the background subagent's tool-uses don't pollute the user's
    /// main JSONL transcript and don't race the main thread's
    /// transcript writer.
    #[serde(default)]
    pub skip_transcript: bool,

    /// Per-fork tool-execution gate. When `Some`, threaded onto the
    /// child engine's `ToolUseContext.can_use_tool` so app/query
    /// enforces the policy before static permission evaluation.
    /// Skipped at the JSON boundary — callbacks aren't portable across
    /// runners.
    /// TS parity: `utils/forkedAgent.ts` `runForkedAgent({canUseTool})`.
    #[serde(skip)]
    pub can_use_tool: Option<crate::can_use_tool::CanUseToolHandleRef>,

    /// When `true`, hook auto-approve cannot bypass the
    /// [`Self::can_use_tool`] callback — speculation needs this so
    /// overlay path-rewrites always run. TS: `requireCanUseTool` on
    /// the subagent context.
    #[serde(default)]
    pub require_can_use_tool: bool,

    /// Typed discriminator for telemetry / logs (`tengu_fork_agent_query`
    /// `forkLabel`). When set, the engine's `query_source_label()`
    /// returns this string so log readers can tell apart the 9 fork
    /// variants without grepping callsites. TS:
    /// `utils/forkedAgent.ts` `runForkedAgent({forkLabel})`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_label: Option<coco_types::ForkLabel>,
    /// Whether the parent session is non-interactive/headless. Team
    /// backend selection uses this to force in-process teammates.
    #[serde(default)]
    pub is_non_interactive: bool,
    /// `tool_use_id` of the `Agent(...)` invocation that produced this
    /// spawn. Threaded into the background task's `<tool-use-id>` tag
    /// so the model correlates completion notifications back to the
    /// original AgentTool call. TS parity: `AgentTool.tsx` passes
    /// `toolUseContext.toolUseId` into `registerAgentForeground` /
    /// `registerAsyncAgent`. Filled at the `AgentTool::execute`
    /// boundary from `ctx.tool_use_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// Agent id of the *invoker* — the agent that called `AgentTool`,
    /// **not** the newly-spawned subagent. Used as the `agent_id`
    /// filter on the `CommandQueue` so a teammate only receives
    /// completion notifications for tasks it itself spawned. `None`
    /// for main-thread spawns. TS parity: `AgentTool.tsx` /
    /// `BashTool.tsx:910` pass `toolUseContext.agentId`. Filled at the
    /// `AgentTool::execute` boundary from `ctx.agent_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invoking_agent_id: Option<String>,
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
    /// Cache-read tokens (TS `cache_read_input_tokens`) — the portion
    /// of the input that hit the prompt cache. Memory's extract / dream
    /// telemetry surfaces this as the cache hit-rate metric so we can
    /// measure whether forked-agent prompt-cache sharing is working.
    /// `0` when the underlying engine doesn't report it.
    #[serde(default)]
    pub cache_read_tokens: i64,
    /// Cache-creation tokens (TS `cache_creation_input_tokens`) — the
    /// portion of the input that wrote into the prompt cache. Memory
    /// telemetry pairs this with `cache_read_tokens` for hit-rate.
    #[serde(default)]
    pub cache_creation_tokens: i64,
    /// Absolute file paths the agent wrote during this spawn, in call
    /// order. Populated by the spawn driver from observed
    /// `Write` / `Edit` / `NotebookEdit` tool_use blocks. Memory
    /// telemetry filters this to exclude the `MEMORY.md` index when
    /// reporting `files_written` (TS parity:
    /// `extractMemories.ts:465-467` — `writtenPaths.filter(p =>
    /// basename(p) !== ENTRYPOINT_NAME)`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths_written: Vec<PathBuf>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamCreateAllowedPath {
    pub path: String,
    pub tool_name: String,
    pub added_by: String,
    pub added_at: i64,
}

/// Typed TeamCreate request. This deliberately carries the session and
/// task-list routing context that the string-shaped API could not express.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CreateTeamRequest {
    pub requested_name: String,
    pub leader_agent_id: Option<String>,
    pub leader_session_id: String,
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_paths: Vec<TeamCreateAllowedPath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_model: Option<String>,
    #[serde(skip)]
    pub task_list_router: Option<TeamTaskListRouterRef>,
}

impl std::fmt::Debug for CreateTeamRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateTeamRequest")
            .field("requested_name", &self.requested_name)
            .field("leader_agent_id", &self.leader_agent_id)
            .field("leader_session_id", &self.leader_session_id)
            .field("cwd", &self.cwd)
            .field("allowed_paths", &self.allowed_paths)
            .field("leader_model", &self.leader_model)
            .field("task_list_router", &self.task_list_router.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateTeamResult {
    pub team_name: String,
    pub lead_agent_id: String,
    pub task_list_id: String,
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
    async fn create_team(&self, request: CreateTeamRequest) -> Result<CreateTeamResult, String>;

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

    /// Interrupt an in-process teammate's current turn without stopping
    /// the teammate lifecycle.
    ///
    /// TS: `currentWorkAbortController` in `inProcessRunner.ts` plus
    /// Escape handling in `useBackgroundTaskNavigation.ts`.
    async fn interrupt_agent_current_work(&self, _agent_id: &str) -> Result<bool, String> {
        Err("AgentHandle::interrupt_agent_current_work not supported in this context".into())
    }

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

    async fn create_team(&self, _request: CreateTeamRequest) -> Result<CreateTeamResult, String> {
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
}
