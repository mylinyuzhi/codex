//! The agent loop — heart of the system.
//!
//! TS: QueryEngine.ts + query.ts
//!
//! State transitions tracked via ContinueReason to enable tests to verify
//! recovery paths without inspecting message contents.

use crate::budget::BudgetDecision;
use crate::command_queue::CommandQueue;
use crate::emit::emit_protocol;
use crate::emit::emit_stream;
use crate::session_state::SessionStateTracker;
use crate::tool_call_runner::ToolCallRunner;
use coco_context::FileHistoryState;
use coco_hooks::HookRegistry;
use coco_inference::ApiClient;
use coco_inference::QueryParams;
use coco_messages::CostTracker;
use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_system_reminder::count_human_turns;
use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use coco_types::TokenUsage;
use coco_types::ToolAppState;

use crate::helpers::budget_pct_used;
use crate::helpers::convert_to_assistant_content;
use crate::helpers::extract_last_assistant_text;
use crate::helpers::should_continue_for_budget;

use coco_llm_types::AssistantContentPart;
use coco_llm_types::ReasoningPart;
use coco_llm_types::TextPart;
use coco_llm_types::ToolCallPart;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing::warn;

// Items from `crate::engine_helpers` used directly by `run_session_loop`.
// `pub(crate) use` (rather than plain `use`) re-exposes them as members of
// this module so `engine.test.rs` can keep resolving them via `super::name`.
// `emit_model_fallback_notice` lived here until the `open_turn_stream`
// extraction moved every call site into `engine_recovery.rs`; the
// re-export was removed.
pub(crate) use crate::engine_helpers::extract_streaming_result_text;

pub use crate::config::ContinueReason;
pub use crate::config::QueryEngineConfig;
pub use crate::config::QueryResult;
pub use crate::config::SessionBootstrap;
use crate::engine_loop_state::LoopAccumulator;
use crate::engine_loop_state::LoopConstants;
use crate::engine_loop_state::LoopTurnState;

/// Maximum consecutive capacity errors (529 / 503 / Overloaded /
/// RateLimited) tolerated on the active model slot before
/// `ModelRuntime::advance` walks to the next fallback slot.
///
/// TS parity: `MAX_529_RETRIES = 3` in `services/api/withRetry.ts:54`.
/// Previously declared inline as a function-local `const` inside
/// `run_session_loop`; promoted to module scope so the value is
/// addressable from helpers (and tests) without re-declaring the same
/// number.
pub(crate) const MAX_CONSECUTIVE_CAPACITY_ERRORS: i32 = 3;

/// Last-compact tracker for `RecompactionInfo` population. Set by
/// `try_full_compact` after a successful compact and read by the next
/// compaction to derive `is_recompaction` / `turns_since_previous`.
/// TS parity: `services/compact/autoCompact.ts:51-60 AutoCompactTrackingState`
/// + `query.ts:521 tracking = { compacted: true, turnId, turnCounter: 0 }`.
///
/// Field-by-field mirror of TS — read directly, no subtraction needed:
/// - `run_id` ≡ TS `turnId` (UUID, generated per compact).
/// - `turn_counter` ≡ TS `turnCounter` (resets to 0 on each compact,
///   bumped +1 per subsequent turn at `engine_finalize_turn.rs`).
#[derive(Debug, Clone)]
pub(crate) struct LastCompactState {
    /// Turns elapsed since the previous compact. `0` immediately after
    /// the compact lands; incremented on each `finalize_turn_post_tools`.
    /// Read directly as `RecompactionInfo.turns_since_previous`.
    pub(crate) turn_counter: i64,
    /// UUID-shaped id of the previous compaction (boundary marker uuid).
    /// Surfaced through `coco_query::compact_track` tracing log; Rust's
    /// substitute for `tengu_post_autocompact_turn` analytics.
    pub(crate) run_id: String,
}

/// The query engine — orchestrates multi-turn agent conversations.
///
/// Fields are `pub(crate)` so the impl block can be split across sibling
/// modules (`engine_builder`, `engine_session`, `engine_compaction`, …)
/// without leaking internal state to the public API. External callers see
/// only the methods exposed by these impls, never the fields directly.
pub struct QueryEngine {
    pub(crate) config: QueryEngineConfig,
    pub(crate) client: Arc<ApiClient>,
    /// Ordered fallback `ApiClient` chain. When non-empty,
    /// [`run_session_loop`](Self::run_session_loop) builds a
    /// per-session multi-slot [`ModelRuntime`] that walks slots in
    /// order on capacity-error streaks. Install via
    /// [`Self::with_fallback_client`] (one tier) or
    /// [`Self::with_fallback_clients`] (chain).
    pub(crate) fallback_clients: Vec<Arc<ApiClient>>,
    /// Pre-resolved `ApiClient` for `ModelRole::Plan`, used to swap
    /// the active client when `permission_mode == Plan`. `None` means
    /// "no swap installed" — the engine stays on Main for every turn,
    /// matching the pre-feature behaviour.
    ///
    /// TS parity behaviour: `getRuntimeMainLoopModel`
    /// (utils/model/model.ts:145). TS encodes the swap via
    /// `opusplan`/`haiku` aliases on the user's main model setting;
    /// coco-rs encodes it as the generic `ModelRole::Plan` config slot,
    /// so the swap works for any provider.
    ///
    /// Set via [`Self::with_plan_role_client`] from
    /// `SessionRuntime::wire_engine` after pre-resolving
    /// `client_for_role(ModelRole::Plan)`. Forks / subagents leave this
    /// `None` so their main loop stays on the parent-inherited client.
    pub(crate) plan_role_client: Option<Arc<ApiClient>>,
    /// Optional half-open recovery policy. Empty = sticky
    /// fallback (post-switch the session stays on the fallback
    /// for the remainder). When set, the engine periodically
    /// probes the primary at turn entry and switches back on
    /// success. Install via [`Self::with_recovery_policy`].
    pub(crate) recovery_policy: Option<coco_config::FallbackRecoveryPolicy>,
    pub(crate) tools: Arc<ToolRegistry>,
    pub(crate) cancel: CancellationToken,
    pub(crate) hooks: Option<Arc<HookRegistry>>,
    /// Captures `is_async` hook output so the reminder pipeline can
    /// deliver it on later turns. Wired by `engine_builder` from the
    /// shared `SessionRuntime`-owned `AsyncHookRegistry`. `None` means
    /// async hooks fired through this engine are dropped — the
    /// session-runtime path stays the canonical wiring.
    pub(crate) async_hook_registry: Option<Arc<coco_hooks::async_registry::AsyncHookRegistry>>,
    /// LLM-driven hook handler used by `Prompt` / `Agent` hook
    /// handlers. Wired by `engine_builder` from the
    /// `SessionRuntime`-owned `Arc<dyn HookLlmHandle>`. `None` means
    /// LLM-driven handlers fall back to passthrough text — orchestration
    /// already handles that path with a `tracing::warn!`.
    pub(crate) hook_llm_handle: Option<Arc<dyn coco_hooks::HookLlmHandle>>,
    /// Shared role-client cache. When set, `finalize_turn_post_tools`
    /// spawns a `ModelRole::Fast` side-fork after each tool batch to
    /// generate a tool-use summary for SDK / mobile UI consumers
    /// (TS `generateToolUseSummary` → coco-rs
    /// [`crate::tool_use_summary::generate_tool_use_summary`]).
    ///
    /// `None` ⇒ no tool-use summaries are generated. Wired by
    /// `engine_builder::with_role_client_cache` from `SessionRuntime`.
    pub(crate) role_client_cache: Option<Arc<coco_inference::RoleClientCache>>,
    /// Pending tool-use-summary `JoinHandle` produced by
    /// [`finalize_turn_post_tools`](Self::finalize_turn_post_tools).
    /// Awaited at the top of the *next* `run_session_loop` iteration
    /// so the summary surfaces to SDK consumers as a
    /// `ServerNotification::ToolUseSummary` just before the next API
    /// call. TS parity: `query.ts:1055-1060`.
    ///
    /// `Arc<Mutex>` (not session-loop local) because the spawn site
    /// (`finalize_turn_post_tools`) and await site (`run_session_loop`
    /// iteration top) sit on different `&self` boundaries; the lock
    /// is uncontended in practice — at most one entry transition per
    /// turn.
    pub(crate) pending_tool_use_summary: Arc<
        tokio::sync::Mutex<
            Option<tokio::task::JoinHandle<Option<coco_types::ToolUseSummaryParams>>>,
        >,
    >,
    /// Mid-turn command queue for steering. Carries both human-typed
    /// prompts (via `QueueOrigin::Human`) and teammate / task-notification
    /// messages (via `QueueOrigin::Coordinator` / `QueueOrigin::TaskNotification`).
    pub(crate) command_queue: CommandQueue,
    /// Session-level file read state for @mention dedup and changed-file detection.
    pub(crate) file_read_state: Option<Arc<RwLock<coco_context::FileReadState>>>,
    /// File history for checkpoint/rewind.
    /// TS: fileHistoryState in AppState + callbacks in toolUseContext.
    pub(crate) file_history: Option<Arc<RwLock<FileHistoryState>>>,
    /// Config home directory for file history backup storage.
    pub(crate) config_home: Option<std::path::PathBuf>,
    /// One-shot SessionStarted payload; emitted at the first turn entry.
    pub(crate) session_bootstrap: Option<SessionBootstrap>,
    /// Optional permission bridge for routing `PermissionDecision::Ask`
    /// outcomes to an external authority (swarm leader or SDK client).
    /// `None` uses the engine's fallback auto-allow behavior.
    pub(crate) permission_bridge: Option<coco_tool_runtime::ToolPermissionBridgeRef>,
    /// Auto-mode state + rules for the 2-stage LLM classifier. When active,
    /// tool calls that return `PermissionDecision::Ask` are first run through
    /// `can_use_tool_in_auto_mode` — Allow/Deny short-circuits the permission
    /// bridge; None falls through to interactive approval. TS: classifier flow
    /// in `utils/permissions/classifierDecision.ts`.
    pub(crate) auto_mode_state: Option<Arc<coco_permissions::AutoModeState>>,
    pub(crate) denial_tracker: Option<Arc<tokio::sync::Mutex<coco_permissions::DenialTracker>>>,
    pub(crate) auto_mode_rules: coco_permissions::AutoModeRules,
    /// Shared cross-turn app state (typed) — carries flags like
    /// `needs_plan_mode_exit_attachment` set by `ExitPlanModeTool`.
    /// Attached via [`Self::with_app_state`]; absent on engines that
    /// don't need this signalling.
    pub(crate) app_state: Option<Arc<RwLock<ToolAppState>>>,
    /// Mailbox handle for swarm teammate messaging. `None` resolves to
    /// `NoOpMailboxHandle` in [`ToolContextFactory::build`]; swarm spawn paths
    /// install a real handle via [`Self::with_mailbox`].
    pub(crate) mailbox: Option<coco_tool_runtime::MailboxHandleRef>,
    /// Per-recipient pending-message store. `None` resolves to
    /// `NoOpPendingMessageStore` in [`ToolContextFactory::build`].
    /// Production wires an `InMemoryPendingMessageStore` here AND on
    /// the SwarmAdapter — single source of truth across SendMessage
    /// push and `agent_pending_messages` reminder drain.
    pub(crate) pending_messages: Option<coco_tool_runtime::PendingMessageStoreRef>,
    /// MCP handle used at prompt-rendering time to read the connected
    /// server set so `AgentTool::prompt` can pre-filter agents whose
    /// `required_mcp_servers` aren't yet ready. `None` ⇒ filter is
    /// skipped (the renderer treats absent MCP as "no filter
    /// applies"). Wired by `session_runtime` via
    /// [`Self::with_mcp_handle`].
    pub(crate) mcp_handle: Option<coco_tool_runtime::McpHandleRef>,
    /// LSP handle for code-intelligence operations exposed to tools
    /// (`LSPTool`). `None` ⇒ `ToolContextFactory` substitutes
    /// `NoOpLspHandle`, which reports `is_connected() = false` so
    /// `LspTool::is_enabled` filters the tool out of the model's tool
    /// list (TS parity:
    /// `LSPTool.isEnabled() = isLspConnected()`). Wired by
    /// `session_runtime` via [`Self::with_lsp_handle`].
    pub(crate) lsp_handle: Option<coco_tool_runtime::LspHandleRef>,
    /// Agent-runtime handle for `AgentTool` (subagent spawn / team
    /// management / background signalling). `None` resolves to
    /// `NoOpAgentHandle` in [`ToolContextFactory::build`]; the CLI /
    /// SDK / TUI runners install a real handle via
    /// [`Self::with_agent_handle`] so `AgentTool` calls reach the
    /// swarm runtime. TS parity: `runAgent.ts` is reachable from any
    /// model call; Rust sessions that skip installation intentionally
    /// restrict Agent tools to model-visible errors.
    pub(crate) agent_handle: Option<coco_tool_runtime::AgentHandleRef>,
    /// Tool-result budget replacement state (TS
    /// `ContentReplacementState`). Threaded through every per-turn
    /// `apply_tool_result_budget` call so seen_ids freeze across
    /// turns (TS contract: a result, once seen, is never re-evaluated
    /// for replacement). The `per_message_chars` field is overwritten
    /// per call from the live `compact.tool_result_budget` so the
    /// budget reflects hot-reloaded config.
    pub(crate) tool_result_replacement_state:
        coco_tool_runtime::tool_result_storage::ContentReplacementStateRef,
    /// Skill-runtime handle for `SkillTool`. Phase 7 routed skills
    /// off `AgentHandle::resolve_skill` onto this dedicated trait.
    /// `None` resolves to `NoOpSkillHandle` in the factory, which
    /// returns `SkillInvocationError::Unavailable` — the runner
    /// surfaces that as a clean model-visible error rather than
    /// panicking.
    pub(crate) skill_handle: Option<coco_tool_runtime::SkillHandleRef>,
    /// Skill-emitted Command-source permission rules scoped to **this
    /// engine instance**. Since `QueryEngine` is rebuilt fresh per user
    /// message (`SessionRuntime::build_engine` is called from every
    /// TUI / SDK / headless / fork driver), engine-scoped = user-msg-scoped.
    /// This mirrors TS `query()`'s `getAppState` closure-captured
    /// `appState.alwaysAllowRules.command`:
    ///
    /// - **Within one user message** — every turn's
    ///   `ToolContextFactory::build` reads this Arc and merges its
    ///   contents into `ToolPermissionContext.allow_rules` under
    ///   [`coco_types::PermissionRuleSource::Command`], so a skill
    ///   invoked on turn N's auto-allow rules are honored on turn N+1.
    /// - **Across user messages** — the engine drops, the Arc count
    ///   goes to zero, the next user message's engine starts with a
    ///   fresh empty store.
    /// - **Subagent forks** — each forked engine builds its own
    ///   Arc; rules emitted inside a subagent skill cannot leak to
    ///   the parent.
    ///
    /// Shared by `Arc` with [`Self::permission_rule_handle`] (which
    /// writes into it) and with the per-batch `ToolContextFactory`
    /// (which reads it for the merge). See `engine_live_rules` module.
    pub(crate) live_command_rules: Arc<RwLock<Vec<coco_types::PermissionRule>>>,
    /// Handle the executor installs on every batch so tool-emitted
    /// `permission_updates` flow into [`Self::live_command_rules`].
    /// Defaults to an [`crate::engine_live_rules::EngineLiveRulesHandle`]
    /// constructed from the same Arc; tests/standalone may override via
    /// [`Self::with_permission_rule_handle`] to install a `NoOp` for
    /// isolation.
    pub(crate) permission_rule_handle: coco_tool_runtime::PermissionRuleHandleRef,
    /// Snapshot of the agent-definition catalog (T7). Surfaced on
    /// every `ToolUseContext` so AgentTool can resolve
    /// `subagent_type → AgentDefinition` and thread the definition
    /// through `AgentSpawnRequest.definition`. `None` ⇒ AgentTool
    /// falls back to subagent_type→role mapping alone.
    pub(crate) agent_catalog: Option<std::sync::Arc<coco_subagent::AgentCatalogSnapshot>>,
    /// Post-turn cache-safe parameter slot (D8). Populated in
    /// `finalize_turn_post_tools` with the parameters that drove the
    /// turn's last request, so post-turn forks (`/btw`,
    /// `promptSuggestion`, `postTurnSummary`) can share the parent's
    /// prompt cache by sending byte-identical request prefixes. Read
    /// via [`Self::last_cache_safe_params`]; cleared by
    /// [`Self::clear_cache_safe_params`] on `/clear` regen.
    ///
    /// `Arc<RwLock<...>>` so observers (TUI status, transcript
    /// recorder) can read the slot without contending with the
    /// engine's writer side.
    pub(crate) last_cache_safe_params:
        std::sync::Arc<tokio::sync::RwLock<Option<coco_types::CacheSafeParams>>>,
    /// Optional dispatcher for one-shot forked queries (D1/D2). When
    /// installed, post-turn forks (`/btw`, `promptSuggestion`,
    /// `postTurnSummary`) drive a *fresh* engine via this dispatcher
    /// rather than mutating the parent. Built and installed by the
    /// CLI bootstrap — TUI/SDK runners share the same instance.
    /// `None` ⇒ post-turn forks degrade to no-op (a placeholder is
    /// surfaced where appropriate; the parent loop continues).
    pub(crate) fork_dispatcher: Option<crate::forked_agent::ForkDispatcherRef>,
    /// Session-scoped abort slot for the in-flight prompt-suggestion
    /// fork. TS parity: `services/PromptSuggestion/promptSuggestion.ts`
    /// module-level `currentAbortController`. When the engine spawns
    /// a new suggestion fork, it cancels the previous in-flight one
    /// so rapid `/clear` cycles don't accumulate fork tasks burning
    /// tokens. `None` ⇒ no abort slot wired (test contexts).
    pub(crate) current_suggestion_abort:
        Option<std::sync::Arc<tokio::sync::Mutex<Option<tokio_util::sync::CancellationToken>>>>,
    /// Background task runtime — the [`TaskHandle`] consumed by
    /// `TaskGet` / `TaskOutput` / `TaskStop` / `TaskList` and by the
    /// AgentTool background dispatch (P2'+ TaskManager wiring).
    /// `None` resolves to `NoOpTaskHandle`; CLI bootstrap installs the
    /// production `TaskRuntime` shared with `SwarmAgentHandle` so a
    /// bg spawn registered by the latter is addressable through the
    /// former.
    pub(crate) task_handle: Option<coco_tool_runtime::BackgroundTaskHandleRef>,
    /// Persistent task-list store (V2, `TaskCreate`/`TaskUpdate`/etc.).
    /// `None` resolves to `NoOpTaskListHandle` — the V2 tools then
    /// return errors on write, matching TS's "no store configured"
    /// behavior. Install via [`Self::with_task_list`].
    pub(crate) task_list: Option<coco_tool_runtime::TaskListHandleRef>,
    /// Router for switching leader task tools onto a team task list.
    pub(crate) team_task_list_router: Option<coco_tool_runtime::TeamTaskListRouterRef>,
    /// Per-agent ephemeral todo store (V1, `TodoWrite`). Defaults to
    /// an in-memory instance when absent.
    pub(crate) todo_list: Option<coco_tool_runtime::TodoListHandleRef>,
    /// Bundle of per-subsystem reminder sources. Populated by CLI /
    /// SDK callers via [`Self::with_reminder_sources`]. Empty default
    /// ⇒ cross-crate reminders silently skip (matches TS behavior
    /// when the corresponding manager isn't initialized).
    pub(crate) reminder_sources: coco_system_reminder::ReminderSources,
    /// Channel for silent attachment events produced by owner crates
    /// (hooks, permissions, commands, core/tool-runtime, skills). Drained at the
    /// head of each outer-loop iteration so the `Message::Attachment`
    /// entries land in history before prompt build.
    ///
    /// Sender cloned to [`Self::attachment_emitter`] for plumbing into
    /// owner crates; receiver is drained by `drain_attachment_inbox`.
    pub(crate) attachment_tx: tokio::sync::mpsc::UnboundedSender<coco_messages::AttachmentMessage>,
    pub(crate) attachment_rx: Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<coco_messages::AttachmentMessage>>,
    >,
    /// Observers notified after each successful full compaction. Each
    /// crate that owns post-compact-invalidated state (file caches, skill
    /// state, memory caches) registers itself once at startup. Empty
    /// default ⇒ no observers fire (TS parity for skipped subsystems).
    /// Implements the TS `runPostCompactCleanup` god-function as a
    /// pluggable registry.
    pub(crate) compaction_observers: Arc<coco_compact::CompactionObserverRegistry>,
    /// Wall-clock millis of the most recent assistant message. Drives
    /// `evaluate_time_based_trigger` so a long inactivity gap can clear
    /// stale tool results before the next API call. TS parity:
    /// `lastAssistantMessageTimestamp` in microCompact.ts.
    pub(crate) last_assistant_ms: Arc<std::sync::atomic::AtomicI64>,
    /// In-memory `lastSummarizedMessageId` carried by the session-memory
    /// path. Set after extraction completes; cleared after any compaction
    /// (the kept-tail UUIDs are no longer the same anchor).
    /// TS: sessionMemoryUtils.ts module-level `let`.
    pub(crate) last_summarized_message_id: Arc<std::sync::Mutex<Option<uuid::Uuid>>>,
    /// Pre-extracted session memory summary text. Empty string disables
    /// the SM-first compact path. TS: contents of `getSessionMemoryPath()`.
    pub(crate) session_memory_text: Arc<tokio::sync::RwLock<String>>,
    /// Optional handle to the consolidated session-memory service.
    /// When present, `try_session_memory_compact` reads its cached
    /// body and calls `wait_for_extraction()` to avoid racing the
    /// in-flight forked-agent extractor. TS: waitForSessionMemoryExtraction.
    /// This is the same `Arc` as `memory_runtime.session_memory`
    /// when both are populated — wired by `SessionRuntime` for
    /// direct access without an `Option<MemoryRuntime>` hop.
    pub(crate) session_memory_service: Option<Arc<coco_memory::SessionMemoryService>>,
    /// Auto-memory runtime — extraction / dream / 9-section session
    /// memory / recall ranker. Distinct from
    /// `session_memory_service` above (which is the compact-side
    /// summary state). When `None`, the memory subsystem stays inert
    /// — typically only set when `Feature::AutoMemory` is enabled and
    /// the CLI bootstrap built the runtime. Turn-end hooks fan out
    /// through this one runtime.
    pub(crate) memory_runtime: Option<Arc<coco_memory::MemoryRuntime>>,
    /// Reactive (PTL) compaction circuit breaker. Tracks consecutive
    /// failures so we stop hammering the same recovery path after
    /// `MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES`.
    pub(crate) reactive_state: Arc<tokio::sync::Mutex<coco_compact::ReactiveCompactState>>,
    /// Auto compaction circuit breaker. Manual compaction is excluded;
    /// successful auto/session-memory compaction resets this state.
    pub(crate) auto_compact_state: Arc<tokio::sync::Mutex<coco_compact::ReactiveCompactState>>,
    /// Optional handle to the running-task manager — when present,
    /// `try_full_compact` snapshots running async agents and re-emits
    /// them as post-compact `task_status` attachments (TS:
    /// `createAsyncAgentAttachmentsIfNeeded`). `None` ⇒ no running-task
    /// awareness; the caller (CLI/SDK) wires this on construction.
    pub(crate) running_tasks: Option<Arc<coco_tasks::running::TaskManager>>,
    /// Last-compact tracker — feeds `RecompactionInfo` (TS parity:
    /// `query.ts:521 tracking = { compacted, turnId, turnCounter: 0 }`).
    /// `LastCompactState.turn_counter` resets to 0 each compact and bumps
    /// +1 per turn in `finalize_turn_post_tools` — read directly with no
    /// subtraction.
    pub(crate) last_compact_state: Arc<std::sync::Mutex<Option<LastCompactState>>>,
    /// Pre-rendered skill snapshot for in-band post-compact attachment.
    /// Caller (CLI/SDK runner) refreshes this whenever invoked skills
    /// change. TS calls `getInvokedSkillsForAgent()` inline; we keep it
    /// caller-driven so this crate doesn't import system-reminder types.
    pub(crate) post_compact_skills: Arc<std::sync::RwLock<Vec<coco_compact::PostCompactSkill>>>,
    /// Runtime sink for SessionStart hook side effects produced during
    /// compact context restoration. The app layer owns file watchers and
    /// other process-global effects; query only reports typed output.
    pub(crate) session_start_hook_side_effect_sink:
        Option<crate::session_start_hooks::SessionStartHookSideEffectSinkRef>,
    /// Staged context-collapse ledger. Pre-staged ranges drain into
    /// commits on PTL recovery so a 413 doesn't have to truncate the
    /// head. `None` ⇒ feature disabled (default). TS:
    /// `services/contextCollapse/index.ts` module-level state.
    pub(crate) staged_ledger: Option<Arc<tokio::sync::Mutex<coco_compact::StagedCompactLedger>>>,
    /// Persistent session id used as the `sessionId` for any
    /// staged-collapse commit/snapshot entries. Lazily set when
    /// staged_ledger is installed.
    pub(crate) staged_session_id: uuid::Uuid,
    /// Most-recently @mentioned absolute file paths, capped at
    /// `MENTION_PRIORITY_CAPACITY`. `try_full_compact` reads this set
    /// to boost mentioned files in the post-compact restoration list
    /// (mentioned files survive even when not the most-recently-read).
    /// Self-designed augmentation; TS has no mention-aware re-injection.
    pub(crate) recently_mentioned_paths:
        Arc<tokio::sync::RwLock<std::collections::VecDeque<std::path::PathBuf>>>,
    /// One-shot `context_management` payload set by `do_reactive_compact`
    /// when the active provider supports server-side edits (Anthropic).
    /// Consumed and cleared by the next QueryParams build so the API can
    /// clear tool results in-place without breaking the prompt cache —
    /// avoiding the client-side mutation that `api_microcompact` would
    /// otherwise perform.
    pub(crate) pending_reactive_context_management:
        Arc<tokio::sync::Mutex<Option<serde_json::Value>>>,
    /// One-shot post-compaction signal. Set to `true` whenever
    /// `do_reactive_compact` / full-compaction / SM-compaction succeeds;
    /// consumed (swap-to-false) by the next `engine_turn_reminders` build.
    /// Mirrors TS `getUnifiedTaskAttachments(ctx)` post-compact emission
    /// surface — only the immediately-following turn surfaces background
    /// task status reminders.
    pub(crate) pending_just_compacted: Arc<std::sync::atomic::AtomicBool>,
    /// Transcript writer used for marble-origami persistence and the
    /// per-turn user/assistant JSONL append. `None` disables
    /// persistence (in-memory ledger only). Caller wires this via
    /// `with_transcript_store`.
    pub(crate) transcript_store: Option<Arc<coco_session::TranscriptStore>>,
    /// Shared session-level usage tracker. QueryEngine is rebuilt per
    /// user message, so the runtime owns this and wires it into every
    /// engine instance.
    pub(crate) session_usage_tracker: Option<Arc<tokio::sync::Mutex<CostTracker>>>,
    /// Serializes update/snapshot/write for session usage so concurrent
    /// engines cannot overwrite a newer snapshot with an older one.
    pub(crate) session_usage_write_lock: Option<Arc<tokio::sync::Mutex<()>>>,
    /// Session id (string form) used for transcript path resolution.
    /// Distinct from `staged_session_id` because TranscriptStore keys
    /// off the session id string used by the rest of the system.
    pub(crate) transcript_session_id: Option<String>,
    /// Dedup set of message UUIDs already written to the transcript
    /// JSONL during this session. Lives on `SessionRuntime` and is
    /// cloned into every per-turn engine via `with_transcript_dedup`
    /// so the same set survives across engine instances. `None`
    /// disables per-turn message persistence (only marble-origami
    /// metadata is written). TS parity: `Project.recordTranscript`
    /// dedups by uuid in `utils/sessionStorage.ts:1408`.
    pub(crate) transcript_dedup:
        Option<Arc<tokio::sync::Mutex<std::collections::HashSet<uuid::Uuid>>>>,
    /// Engine-side delivery channel for nested-memory entries surfaced
    /// by [`crate::engine_attachments::QueryEngine::drain_nested_memory_triggers`]
    /// at end-of-batch. Consumed by `engine_turn_reminders` right before
    /// building `TurnReminderInput` — the generator renders these into
    /// `<system-reminder>Contents of {path}:\n\n{content}</system-reminder>`.
    ///
    /// Populated by the trigger drain; bypasses the no-op
    /// [`crate::reminder_adapters::MemoryAdapter::nested_memories`].
    pub(crate) pending_nested_memory: std::sync::Arc<
        tokio::sync::Mutex<Vec<coco_system_reminder::generators::memory::NestedMemoryInfo>>,
    >,
    /// Session-level dedup set for nested-memory paths. Mirrors TS
    /// `loadedNestedMemoryPathsRef` (`REPL.tsx:1964-1967`) — once a
    /// memory file is injected in this session, subsequent file reads
    /// in the same subtree won't re-inject it. Cleared on conversation
    /// reset via
    /// [`crate::engine_attachments::QueryEngine::clear_loaded_nested_memory_paths`].
    pub(crate) loaded_nested_memory_paths:
        std::sync::Arc<tokio::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>>,
    /// Sync hook event buffer shared with `SessionRuntime` so that
    /// SessionStart and UserPromptSubmit hook output surfaces as
    /// per-turn `hook_*` reminders. `None` disables sync-event
    /// surfacing — the orchestration layer's push then becomes a
    /// no-op. Installed on per-turn engines via
    /// [`Self::with_sync_hook_buffer`].
    pub(crate) sync_hook_buffer: Option<coco_hooks::SyncHookEventBuffer>,
}

// `new`, every `with_*` builder, and small accessor methods live in
// `crate::engine_builder`. Public entry points (`run`, `run_with_events`,
// `run_with_messages`), session lifecycle helpers
// (`emit_session_started`, `build_session_*_params`, `orchestration_ctx`),
// and the `forward_hook_events` bridge live in `crate::engine_session`.
// The compaction methods live in `crate::engine_compaction` /
// `crate::engine_finalize_turn`.

impl QueryEngine {
    /// Run the multi-turn agent loop.
    ///
    /// `cycle_turn_id` is the per-cycle wire id supplied by the
    /// runner; it's used on every `TurnEnded` emission this function
    /// makes (Completed / Interrupted / MaxTurnsReached /
    /// BudgetExhausted). `None` only when the caller didn't pass an
    /// `event_tx` — in that case no wire emit happens.
    ///
    /// Returns `(Result<QueryResult>, TokenUsage)`. The second tuple
    /// element is the accumulated token usage at the moment the
    /// loop exited — populated even on `Err` so the caller can
    /// surface partial usage on the `TurnEnded(Failed)` wire emit.
    pub(crate) async fn run_session_loop(
        &self,
        turn_messages: Vec<std::sync::Arc<Message>>,
        event_tx: Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
        state_tracker: &SessionStateTracker,
        hook_tx_opt: Option<tokio::sync::mpsc::Sender<coco_hooks::HookExecutionEvent>>,
        history: &mut MessageHistory,
        cycle_turn_id: Option<coco_types::TurnId>,
    ) -> (Result<QueryResult, coco_error::BoxedError>, TokenUsage) {
        let mut total_usage = TokenUsage::default();
        let result = self
            .run_session_loop_inner(
                turn_messages,
                event_tx,
                state_tracker,
                hook_tx_opt,
                history,
                cycle_turn_id,
                &mut total_usage,
            )
            .await;
        (result, total_usage)
    }

    #[allow(clippy::too_many_arguments)]
    // Internal helper. Argument count is intentional — splitting into
    // a context struct adds indirection for a function with one
    // call site and would obscure that each argument has distinct
    // lifetime / ownership semantics (one `&mut`, one `&`, three
    // owned, two `Option<owned>`).
    async fn run_session_loop_inner(
        &self,
        turn_messages: Vec<std::sync::Arc<Message>>,
        event_tx: Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
        state_tracker: &SessionStateTracker,
        hook_tx_opt: Option<tokio::sync::mpsc::Sender<coco_hooks::HookExecutionEvent>>,
        history: &mut MessageHistory,
        cycle_turn_id: Option<coco_types::TurnId>,
        total_usage: &mut TokenUsage,
    ) -> Result<QueryResult, coco_error::BoxedError> {
        // ── Loop state, grouped by lifecycle. Mirrors TS `query.ts:204-217`
        //    `State` adapted to Rust's borrow model — see
        //    `engine_loop_state.rs` for the field-by-field rationale and
        //    `init_loop_state` for the bundled construction site.
        let (mut acc, mut turn_state, mut services, consts) = self
            .init_loop_state(turn_messages, &event_tx, history)
            .await;

        loop {
            if self.cancel.is_cancelled() {
                // Single writer for the user-cancel marker (dedup +
                // typed `SystemMessage::UserInterruption` + emit
                // `MessageAppended`). `in_flight_tool_calls = false`
                // here because this branch fires before any tool
                // execution started for the current turn. See
                // `engine-tui-unified-transcript-plan.md` §7.1 /
                // `history_sync::finalize_user_cancel`.
                crate::history_sync::finalize_user_cancel(
                    history, /*in_flight_tool_calls*/ false, &event_tx,
                )
                .await;
                // NOTE: we do NOT emit `TurnEnded(Interrupted)` from the
                // engine. The runner layer (`tui_runner` / `sdk_runner`)
                // owns the cancel-reason source of truth — a TUI
                // `/compact` cancel sets `SystemPreempt` in the runner's
                // `OnceLock<CancelReason>` while `UserCommand::Interrupt`
                // sets `UserCancel`. Engine has no visibility into which
                // arm tripped `self.cancel`, so emitting from here would
                // force a hardcoded reason and defeat the architecture.
                // The runner reads `result.cancelled` and emits a single
                // `TurnEnded(Interrupted)` with the correct reason.
                return Ok(make_query_result(
                    &consts,
                    acc,
                    turn_state,
                    String::new(),
                    /*cancelled*/ true,
                    /*budget_exhausted*/ false,
                    Some("cancelled".into()),
                    history.to_vec(),
                ));
            }

            // Drain the prior turn's tool-use-summary side-fork (TS
            // `query.ts:1055-1060` — await `pendingToolUseSummary` at
            // the top of the next iteration). 2s hard cap; never
            // blocks the new turn for more than that. Silent no-op
            // when no pending handle exists (first iteration, or
            // previous turn had no tool batch).
            self.drain_pending_tool_use_summary(&event_tx).await;

            // Budget check before each turn
            match turn_state.budget.check(turn_state.turn) {
                BudgetDecision::Stop { reason } => {
                    warn!(%reason, "budget stop");
                    let hit_max_turns =
                        self.config.max_turns > 0 && turn_state.turn >= self.config.max_turns;
                    if hit_max_turns {
                        let payload = coco_messages::MaxTurnsReachedPayload {
                            max_turns: self.config.max_turns,
                            turn_count: turn_state.turn,
                        };
                        acc.run_artifacts.max_turns_reached = Some(payload.clone());
                        crate::history_sync::history_push_and_emit(
                            history,
                            Message::Attachment(
                                coco_messages::AttachmentMessage::silent_max_turns_reached(payload),
                            ),
                            &event_tx,
                        )
                        .await;
                    }
                    // Wire-protocol terminator. Two outcomes: `MaxTurnsReached`
                    // when the turn budget is exhausted, `BudgetExhausted` for
                    // the generic 90%/diminishing-returns token-budget stop.
                    // Uses the runner-supplied cycle id so this pairs with
                    // the runner's TurnStarted. `budget_tokens` is the
                    // configured ceiling — `None` when no explicit max
                    // was set (the 90%-of-window heuristic still drove
                    // the stop; emitting 0 would be a lie).
                    if let Some(id) = cycle_turn_id.as_ref() {
                        // `*total_usage` deref is intentional — TokenUsage
                        // is Copy. If a future refactor ever drops Copy
                        // (Vec / String field), the resulting compile
                        // error tells you to update both this emit AND
                        // the make_query_result call below; don't paper
                        // over with `.clone()` without checking the
                        // sibling site.
                        let outcome_params = if hit_max_turns {
                            coco_types::TurnEndedParams::max_turns_reached(
                                id.clone(),
                                Some(*total_usage),
                                self.config.max_turns,
                            )
                        } else {
                            coco_types::TurnEndedParams::budget_exhausted(
                                id.clone(),
                                Some(*total_usage),
                                total_usage.input_tokens.total + total_usage.output_tokens.total,
                                self.config.total_token_budget,
                            )
                        };
                        let _ = emit_protocol(
                            &event_tx,
                            crate::ServerNotification::TurnEnded(outcome_params),
                        )
                        .await;
                    }
                    let last_text = extract_last_assistant_text(history);
                    return Ok(make_query_result(
                        &consts,
                        acc,
                        turn_state,
                        last_text,
                        /*cancelled*/ false,
                        /*budget_exhausted*/ true,
                        Some(
                            if hit_max_turns {
                                "max_turns"
                            } else {
                                "budget_exhausted"
                            }
                            .into(),
                        ),
                        history.to_vec(),
                    ));
                }
                BudgetDecision::Nudge { message } => {
                    info!(%message, "budget nudge");
                    // No direct ServerNotification for budget nudge; emit as non-retryable Error
                    // so SDK consumers can surface the warning.
                    let _delivered = emit_protocol(
                        &event_tx,
                        crate::ServerNotification::Error(coco_types::ErrorParams {
                            message,
                            category: Some("budget".into()),
                            retryable: false,
                        }),
                    )
                    .await;
                }
                BudgetDecision::Continue => {}
            }

            turn_state.turn += 1;
            let turn_id = format!("turn-{}", turn_state.turn);

            // Consume the one-shot `pending_clear_message_history` flag
            // set by `ExitPlanModeTool` when the user picked "clear
            // context" in the multi-choice exit dialog. We drain it
            // here at turn entry (after `turn += 1` so the log line
            // below reports the cleared state) so the cleared history
            // is what every downstream subsystem (reminders, prompt
            // build, API call) observes.
            //
            // TS parity: `ExitPlanModePermissionRequest.tsx:383`
            // sets `initialMessage.clearContext = true`, and the REPL
            // wipes context before starting the new session. Rust
            // mirrors the intent — at the next turn the model sees a
            // fresh transcript.
            if let Some(state_handle) = self.app_state.as_ref() {
                let drained = {
                    let mut w = state_handle.write().await;
                    std::mem::take(&mut w.pending_clear_message_history)
                };
                if drained {
                    // I-1 (Authority): every transcript mutation must
                    // emit so TUI's TranscriptView + SDK NDJSON
                    // observers stay coherent. Plan-mode exit doesn't
                    // rotate session_id — `MessageTruncated { 0 }` is
                    // the right signal (vs. SessionResetForResume).
                    crate::history_sync::history_clear_and_emit(history, &event_tx).await;
                    info!(
                        turn = turn_state.turn,
                        "plan-mode exit cleared conversation history"
                    );
                }
            }

            // The `turn` canonical anchor cannot be a single async span guard
            // here: this loop body has many `.await` points, and
            // `EnteredSpan` is sync-only. Per-step turn correlation is
            // provided via the `turn` / `turn_id` fields stamped on each
            // structured event below — pivots on `turn_id` reconstruct the
            // turn without an enclosing span.
            // `turn` is the per-`run_session_loop` iteration (resets on each
            // user submit — matches TS `query.ts` `turnCount`).
            // `session_turn` is the monotonic human-turn count across the
            // whole conversation, useful for cross-submit log correlation
            // (matches TS `getPlanModeAttachmentTurnCount`'s human-only count).
            // `last_compact_run_id` + `turns_since_last_compact` are the TS
            // `tracking.turnId` + `tracking.turnCounter` payload — read
            // directly from `LastCompactState`, no subtraction. `None`
            // until the first compact lands.
            let session_turn = count_human_turns(history.as_slice());
            let (last_compact_run_id, turns_since_last_compact) = self
                .last_compact_state
                .lock()
                .ok()
                .and_then(|g| g.clone())
                .map(|prev| (Some(prev.run_id), Some(prev.turn_counter)))
                .unwrap_or((None, None));
            info!(
                turn = turn_state.turn,
                turn_id = %turn_id,
                cycle_turn_id = ?cycle_turn_id.as_ref().map(coco_types::TurnId::as_str),
                session_turn,
                last_compact_run_id = ?last_compact_run_id,
                turns_since_last_compact = ?turns_since_last_compact,
                history_len = history.len(),
                active_model = services.runtime.current_model_id(),
                "turn start (per-round; cycle TurnStarted was emitted by run_internal_with_messages)"
            );
            // No wire emit here — TurnStarted is once-per-cycle, fired
            // by the runner / `engine_session::run_internal_with_messages`
            // before this loop starts. `turn_id` above is the
            // per-round id used only for log correlation.

            // Turn-start reminder pipeline (Phase D.3) — runs the five-phase
            // reminder build / orchestrate / bookkeeping / inject sequence
            // and returns the `app_state` snapshot used by `build_tool_definitions`
            // below. The full implementation lives in
            // `crate::engine_turn_reminders` to keep this loop legible.
            let app_state_snapshot = self
                .run_turn_reminder_pipeline(crate::engine_turn_reminders::TurnReminderContext {
                    history: &mut *history,
                    plan_reminder: &mut services.plan,
                    orchestrator: &services.reminders,
                    last_user_input_uuid: &mut turn_state.reminder_last_user_input_uuid,
                    total_usage: &acc.total_usage,
                    cost_tracker: &acc.cost_tracker,
                    todo_key: &consts.todo_key,
                    context_window: consts.context_window,
                    effective_window: consts.effective_window,
                    event_tx: &event_tx,
                })
                .await;

            // Build prompt from history. `BuiltPrompt` carries the
            // post-budget `Arc<Vec<Arc<Message>>>` snapshot so every
            // tool ctx in this turn observes byte-identical history.
            // TS parity: `query.ts:548` sets `toolUseContext.messages`
            // to the same `messagesForQuery` after `applyToolResultBudget`.
            let crate::engine_prompt::BuiltPrompt {
                prompt,
                messages_snapshot,
            } = self.build_prompt(history).await;
            let tool_defs = self.build_tool_definitions(&app_state_snapshot).await;

            // StreamRequestStart has no direct protocol equivalent; it was
            // previously only used for test classification. The model_id is
            // already carried in SessionStarted at session init.

            // Call LLM via streaming. TextDelta/ThinkingDelta events fire
            // as the model generates, not post-hoc — so SDK consumers and the
            // TUI see tokens land in real-time. Tool calls are accumulated
            // into ordered buffers and dispatched after the stream finishes
            // (mid-stream tool dispatch is a follow-up — see PR-E1 Phase 2).
            //
            // TS reference: query.ts:659-845 (streaming loop + tool exec).
            // `max_tokens` is filled in after `active_client` resolves
            // below (post plan-swap candidate selection) so the escalate
            // ceiling reads from the actual model that will receive the
            // request. See [`engine_recovery::effective_max_tokens`].
            // Anthropic-only `context_management`: ask the API to clear
            // old tool results / thinking blocks server-side without
            // breaking the prompt cache. Strategy list is built from the
            // resolved `compact.api_native` config; for non-Anthropic
            // clients `supports_server_side_context_edits` returns
            // `false` and the field stays `None`.
            //
            // Reactive PTL recovery may have queued a one-shot aggressive
            // payload (`pending_reactive_context_management`) — that
            // overrides the steady-state config for exactly one call. We
            // consume it here so the next call falls back to steady-state.
            let context_management = if self.client.supports_server_side_context_edits() {
                let mut pending = self.pending_reactive_context_management.lock().await;
                if let Some(v) = pending.take() {
                    Some(v)
                } else {
                    drop(pending);
                    let opts = coco_compact::ApiContextOptions::from_config(
                        &self.config.compact.api_native,
                        /*has_thinking*/ self.config.thinking_level.is_some(),
                        /*is_redact_thinking_active*/ false,
                        /*clear_all_thinking*/ false,
                    );
                    let strategies = coco_compact::get_api_context_management(&opts);
                    coco_compact::encode_anthropic_context_management(&strategies)
                }
            } else {
                None
            };

            let query_source = self.query_source_label();
            // Wall-clock gap to the most recent assistant message.
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let last_ms = self
                .last_assistant_ms
                .load(std::sync::atomic::Ordering::Acquire);
            let time_since_last_assistant_ms = if last_ms > 0 {
                Some((now_ms - last_ms).max(0))
            } else {
                None
            };

            let mut params = QueryParams {
                prompt,
                max_tokens: None,
                // Per-call thinking override resolved at session
                // bootstrap (parent: `/effort` slash command, agents:
                // `AgentQueryConfig.effort` → adapter parses into
                // `ThinkingLevel`). `None` falls through to the model's
                // `ModelInfo.default_thinking_level` inside
                // `build_call_options`. Cloning is cheap — `ThinkingLevel`
                // is `Option<ReasoningEffort> + Option<i32> + small map`.
                thinking_level: self.config.thinking_level.clone(),
                fast_mode: false,
                tools: if tool_defs.is_empty() {
                    None
                } else {
                    Some(tool_defs)
                },
                tool_choice: None,
                context_management,
                query_source: Some(query_source.to_string()),
                agent_id: self.config.agent_id.clone(),
                time_since_last_assistant_ms,
                // Main-loop turn — agentic baseline beta on, prompt-cache
                // strategy resolved at session bootstrap; defaults if not
                // wired yet (Phase 2 of prompt-cache rollout).
                agentic: true,
                cache: self.config.prompt_cache.clone(),
                // Main-loop agent calls never want generation stop
                // sequences — those are reserved for helper calls
                // (e.g. the auto-mode classifier's stage-1 `</block>`
                // terminator).
                stop_sequences: None,
                // Native structured-output is a side-query / helper
                // concept; the main agent loop never asks the model
                // to emit a single JSON object.
                response_format: None,
            };

            // ── Phase 9: Streaming tool scheduling ──
            //
            // When `config.streaming_tool_execution = true`, safe
            // tools start executing the moment their input buffer
            // completes, rather than waiting for the whole stream
            // to finish. The `StreamingHandle` owns the inflight
            // JoinSet and the gate that preserves TS parity
            // (`canExecuteTool`: no safe-during-unsafe mid-stream).
            //
            // We build the shared ctx (Arc'd so spawned tasks can
            // hold owned clones) + `StreamingHandle` here, ahead of
            // the stream loop. When streaming is off, the whole
            // block is an unused `None` and the legacy batch path
            // below handles execution post-Finish.
            let streaming_enabled = self.config.streaming_tool_execution;
            let streaming_ctx: Option<Arc<ToolUseContext>> = if streaming_enabled {
                let current_supports_tool_reference = services
                    .runtime
                    .current_client()
                    .model_info()
                    .is_some_and(|info| {
                        info.has_capability(coco_types::Capability::ServerSideToolReference)
                    });
                let current_supports_client_side_tool_search = services
                    .runtime
                    .current_client()
                    .model_info()
                    .is_some_and(|info| {
                        info.has_capability(coco_types::Capability::ClientSideToolSearch)
                    });
                let base = self
                    .tool_context_factory(hook_tx_opt.as_ref())
                    .build(crate::tool_context::ToolContextOverrides {
                        user_message_id: Some(consts.user_uuid.clone()),
                        progress_tx: Some(services.progress_tx.clone()),
                        current_model_id: Some(services.runtime.current_model_id().to_string()),
                        current_model_supports_tool_reference: current_supports_tool_reference,
                        current_model_supports_client_side_tool_search:
                            current_supports_client_side_tool_search,
                        messages_snapshot: Some(messages_snapshot.clone()),
                    })
                    .await;
                Some(Arc::new(base))
            } else {
                None
            };
            let mut streaming_handle = streaming_ctx.as_ref().map(|ctx_arc| {
                let executor_base = coco_tool_runtime::StreamingToolExecutor::new();
                let executor_with_state = match self.app_state.as_ref() {
                    Some(state) => executor_base.with_app_state(state.clone()),
                    None => executor_base,
                };
                let executor = Arc::new(
                    executor_with_state
                        .with_permission_rule_handle(self.permission_rule_handle.clone()),
                );
                let ctx_for_closure = ctx_arc.clone();
                let hooks_for_closure = self.hooks.clone();
                let orchestration_for_closure = self.orchestration_ctx();
                let hook_tx_for_closure = hook_tx_opt.clone();
                executor.streaming_handle(move |prepared, _runtime| {
                    let ctx = ctx_for_closure.clone();
                    let hooks = hooks_for_closure.clone();
                    let orchestration_ctx = orchestration_for_closure.clone();
                    let hook_tx = hook_tx_for_closure.clone();
                    Box::pin(async move {
                        let effective_input = prepared.parsed_input.clone();
                        let call_ctx = ctx.clone_for_tool_call(prepared.tool_use_id.clone());
                        let execute_result = tokio::select! {
                            r = prepared.tool.execute(effective_input.clone(), &call_ctx) => r,
                            () = call_ctx.cancel.cancelled() => Err(coco_tool_runtime::ToolError::Cancelled),
                        };
                        crate::tool_outcome_builder::build_outcome_from_execution(
                            crate::tool_outcome_builder::RunOneTail {
                                tool_use_id: prepared.tool_use_id.clone(),
                                tool_id: prepared.tool_id.clone(),
                                tool_name: prepared.tool.name().to_string(),
                                model_index: prepared.model_index,
                                tool: prepared.tool,
                                effective_input,
                                execute_result,
                                hooks: hooks.as_ref(),
                                orchestration_ctx,
                                hook_tx: hook_tx.as_ref(),
                                tool_result_session_dir: ctx.tool_result_session_dir.clone(),
                            },
                        )
                        .await
                    })
                        as std::pin::Pin<
                            Box<
                                dyn std::future::Future<
                                        Output = coco_tool_runtime::UnstampedToolCallOutcome,
                                    > + Send,
                            >,
                        >
                })
            });
            let mut streaming_model_index: usize = 0;

            let api_start = std::time::Instant::now();
            // Half-open recovery probe: if a policy is configured
            // and the backoff window elapsed since the last
            // fallback switch, swap to primary for this turn. The
            // probe uses the same call path as a normal turn — no
            // side-channel ping — so success keeps the response
            // AND any cache-warming the provider did. Probe state
            // is owned by ModelRuntime (see `probe_in_flight`),
            // not here — the engine only decides when to start
            // and when to finalize.
            match services
                .runtime
                .attempt_probe_if_due(std::time::Instant::now())
            {
                crate::model_runtime::ProbeDecision::Skip => {}
                crate::model_runtime::ProbeDecision::Probe => {
                    info!(
                        probe_target = services.runtime.current_model_id(),
                        "probing primary via half-open recovery",
                    );
                }
            }
            let was_probing = services.runtime.probe_in_flight();
            // Route through ModelRuntime so post-fallback / probe
            // calls reach the active provider. When no fallback is
            // configured this is identical to `self.client.query_stream`.
            //
            // Plan-mode swap (TS parity `getRuntimeMainLoopModel`,
            // utils/model/model.ts:145): when `permission_mode == Plan`
            // and the user pre-configured a Plan-role client, route
            // this turn through it instead of the Main client. The
            // context-size guard mirrors TS's `exceeds200kTokens` bypass
            // — when the most recent assistant message's total context
            // would overflow the Plan model's window, stay on Main to
            // avoid truncation. The threshold is configurable via
            // `PlanModeSettings.plan_model_fallback_threshold_tokens`.
            let live_permission_mode = match self.app_state.as_ref() {
                Some(state) => state
                    .read()
                    .await
                    .permission_mode
                    .unwrap_or(self.config.permission_mode),
                None => self.config.permission_mode,
            };
            let plan_swap_candidate = if live_permission_mode == coco_types::PermissionMode::Plan
                && !crate::engine_helpers::most_recent_assistant_exceeds(
                    history.as_slice(),
                    self.config
                        .plan_mode_settings
                        .plan_model_fallback_threshold_tokens,
                ) {
                self.plan_role_client.as_ref()
            } else {
                None
            };
            let active_client = match plan_swap_candidate {
                Some(plan_client) => plan_client.clone(),
                None => services.runtime.current_client(),
            };

            // Single-source per-call `max_tokens`: drives the QueryParams
            // for the upcoming `open_turn_stream` call AND the C15 gate
            // below. `Some(N)` only during a Phase-1 escalate retry
            // (model opted in via `ModelInfo.max_output_tokens_escalate`);
            // `None` on every other call — the inference layer falls
            // through to the active model's baseline `max_output_tokens`.
            // Computed here (not at QueryParams construction) so the
            // ModelInfo lookup tracks the actually-active client even
            // when plan-mode swaps the request to a Plan-role model.
            let effective_max_tokens =
                crate::engine_recovery::effective_max_tokens(&active_client, &turn_state);
            params.max_tokens = effective_max_tokens;

            // Pre-API blocking-limit gate (Finding C15). Prevents the
            // request from hitting the API when the estimated prompt
            // already exceeds the active model's context window minus
            // the reserved output budget. Without this gate, the
            // request would 4xx and route into reactive compaction,
            // which may also fail (the history is already over the
            // window for this model). Surfaces a synthetic api_error
            // and exits with `stop_reason = "blocking_limit"` instead.
            match self.check_blocking_limit(
                history,
                &active_client,
                &turn_state,
                effective_max_tokens,
            ) {
                crate::engine_recovery::BlockingLimitDecision::Block {
                    estimated_tokens,
                    context_window,
                } => {
                    warn!(
                        estimated_tokens,
                        context_window,
                        provider = active_client.provider(),
                        model_id = active_client.model_id(),
                        "pre-API blocking limit hit — estimated prompt exceeds model context",
                    );
                    crate::history_sync::history_push_and_emit(
                        history,
                        crate::helpers::build_blocking_limit_api_error_message(
                            estimated_tokens,
                            context_window,
                        ),
                        &event_tx,
                    )
                    .await;
                    // Wire-protocol terminator. SDK iterators / TUI
                    // state machines key on a terminal `TurnEnded`
                    // notification; without this they block on
                    // `events()`. The other Ok-early-return paths emit
                    // their own terminators too: cancel is left to the
                    // runner's `TurnEnded(Interrupted)`, and budget
                    // exhaustion emits `TurnEnded(MaxTurnsReached |
                    // BudgetExhausted)`. `engine_session.rs`'s
                    // `TurnEnded(Failed)` only fires on `Err` results, so
                    // this Ok-return must emit one explicitly. Uses the
                    // runner-supplied cycle id so it pairs with the
                    // runner's `TurnStarted`; `Provider` matches the
                    // `StatusCode::ContextWindowExceeded` category so a
                    // client-side block and a provider-reported overflow
                    // land under the same wire category.
                    if let Some(id) = cycle_turn_id.as_ref() {
                        let _ = emit_protocol(
                            &event_tx,
                            crate::ServerNotification::TurnEnded(
                                coco_types::TurnEndedParams::failed(
                                    id.clone(),
                                    Some(*total_usage),
                                    coco_types::ErrorPayload {
                                        message: format!(
                                            "blocking_limit: estimated {estimated_tokens} tokens \
                                             exceeds active model context window {context_window} \
                                             (provider={}, model={})",
                                            active_client.provider(),
                                            active_client.model_id(),
                                        ),
                                        code: coco_types::ErrorCode::Provider,
                                    },
                                ),
                            ),
                        )
                        .await;
                    }
                    return Ok(make_query_result(
                        &consts,
                        acc,
                        turn_state,
                        String::new(),
                        /*cancelled*/ false,
                        /*budget_exhausted*/ false,
                        Some("blocking_limit".into()),
                        history.to_vec(),
                    ));
                }
                crate::engine_recovery::BlockingLimitDecision::Proceed => {}
            }

            tracing::debug!(
                turn = turn_state.turn,
                turn_id = %turn_id,
                provider = active_client.provider(),
                model_id = active_client.model_id(),
                max_tokens = ?effective_max_tokens,
                tool_count = params.tools.as_ref().map(Vec::len).unwrap_or(0),
                prompt_messages = params.prompt.len(),
                agentic = params.agentic,
                probing = was_probing,
                "opening LLM stream"
            );
            let mut rx = match self
                .open_turn_stream(
                    &active_client,
                    &params,
                    was_probing,
                    &mut services,
                    &mut turn_state,
                    &mut *history,
                    &event_tx,
                    &turn_id,
                )
                .await
            {
                Ok(rx) => rx,
                Err(crate::engine_recovery::StreamErrorOutcome::Continue) => continue,
                Err(crate::engine_recovery::StreamErrorOutcome::Bail(err)) => return Err(err),
            };

            // ── Phase 9: stream consume ──
            //
            // Drive the per-turn LLM stream to completion via the
            // `engine_stream_consume::consume_stream` extension
            // method. Returns `StreamConsumed { accumulators, outcome }`
            // where `outcome` is the typed three-variant
            // `StreamOutcome`. Cancellation is observed via
            // `self.cancel.is_cancelled()` post-call — the token
            // itself is the canonical signal.
            let consumed = self
                .consume_stream(
                    &mut rx,
                    &event_tx,
                    &mut *history,
                    hook_tx_opt.as_ref(),
                    &mut streaming_handle,
                    streaming_ctx.as_ref(),
                    &mut streaming_model_index,
                    state_tracker,
                    &turn_id,
                    &consts,
                    &services,
                    &mut acc,
                    &turn_state,
                )
                .await;
            let crate::engine_stream_consume::StreamConsumed {
                response_text,
                tool_order,
                tool_buffers,
                outcome,
            } = consumed;

            let api_elapsed_ms = api_start.elapsed().as_millis() as i64;
            acc.api_time_ms += api_elapsed_ms;

            // Cancellation mid-stream: drain staged tool plans (their
            // pending_early `tool_result` rows must pair with a
            // synthetic `tool_use` assistant message for I1
            // adjacency), write the canonical user-cancel marker, and
            // `continue` so the top-of-loop cancel check builds the
            // proper `QueryResult { cancelled: true }`. See
            // `engine_stream_consume::cancel_epilogue` for details.
            // This also catches the late-cancel case: cancel token
            // observable post-`consume_stream` even if the loop
            // returned a `Finished`/`Errored` outcome (cancel set
            // racing with `break;` after event arm).
            if self.cancel.is_cancelled() {
                self.cancel_epilogue(
                    &mut streaming_handle,
                    &tool_order,
                    &tool_buffers,
                    &mut *history,
                    &event_tx,
                    &services,
                    &consts,
                )
                .await;
                continue;
            }

            // Branch on outcome: `Errored` routes through the recovery
            // dispatcher (may `continue` or surface a fatal error);
            // `Finished` and `PrematureClose` both fall through to
            // the success path — `PrematureClose` is modeled as a
            // clean turn with empty snapshot + `stop_reason: None`.
            let (snapshot, usage, parsed_stop_reason) = match outcome {
                crate::engine_stream_consume::StreamOutcome::Errored { message } => {
                    match self
                        .handle_stream_error(
                            message,
                            &mut services,
                            &mut turn_state,
                            &mut *history,
                            &event_tx,
                            &mut streaming_handle,
                            &turn_id,
                        )
                        .await
                    {
                        crate::engine_recovery::StreamErrorOutcome::Continue => continue,
                        crate::engine_recovery::StreamErrorOutcome::Bail(err) => return Err(err),
                    }
                }
                crate::engine_stream_consume::StreamOutcome::Finished {
                    snapshot,
                    usage,
                    stop_reason,
                } => (snapshot, usage, Some(stop_reason)),
                crate::engine_stream_consume::StreamOutcome::PrematureClose => (
                    std::sync::Arc::new(coco_inference::AssistantTurnSnapshot::default()),
                    coco_types::TokenUsage::default(),
                    None,
                ),
            };

            // Capacity streak was already cleared at stream-open
            // success (the `Ok(rx) => ...` arm above sets
            // `consecutive_capacity_errors = 0`). Reaching this point
            // means the stream opened cleanly, so a second reset here
            // would be a no-op writing `0 = 0`. Kept as a load-bearing
            // invariant comment instead of dead code (Finding **R9**).

            acc.total_usage += usage;
            // Mirror into the wrapper-owned `&mut total_usage` slot so
            // `run_session_loop` can surface accumulated usage on the
            // Err path (where `acc` is dropped without ever building a
            // QueryResult) and so the once-per-cycle `TurnEnded` emits
            // that read `*total_usage` agree with `acc.total_usage`. On
            // the Ok paths the value rides home inside
            // `QueryResult.total_usage` via `make_query_result`.
            *total_usage = acc.total_usage;
            turn_state.budget.record_usage(&usage);
            // Record usage against the currently-active logical model
            // identity (post-fallback value if a switch has happened),
            // not the provider wire alias after `api_model_name`.
            let identity = active_client.model_identity();
            let provider = identity.provider.clone();
            let model_id = identity.model_id.clone();
            acc.cost_tracker
                .record_usage(&provider, &model_id, usage, api_elapsed_ms);
            self.record_session_usage(&event_tx, &provider, &model_id, usage, api_elapsed_ms)
                .await;

            // **Finding R11** — stamp the wall-clock "last assistant
            // message" timestamp the moment we know the LLM produced
            // a response (Finish or PrematureClose with content). Drives
            // (a) `time_since_last_assistant_ms` on next-turn
            // `QueryParams` and (b)
            // `coco_compact::evaluate_time_based_trigger` inside
            // `finalize_turn_post_tools`. The setter
            // [`Self::stamp_assistant_now`] was defined but had zero
            // callers, so time-based microcompact silently never
            // triggered and the QueryParams field was always `None`.
            // Stamped here rather than at the various `history_push_*`
            // sites because (i) recovery + ContentFilter + clean paths
            // all converge here after usage is recorded, and (ii)
            // stream-Error paths (`handle_stream_*_error` → Continue or
            // Bail) deliberately don't stamp — no model output, no
            // signal.
            self.stamp_assistant_now();

            // Reconstruct assistant content from the per-turn snapshot
            // accumulated inside `coco-inference::process_stream_with_config`.
            // Each `TurnPart` carries its own `provider_metadata`, so
            // Gemini `thoughtSignature` / Anthropic `signature` /
            // OpenAI `encrypted_content` survive intact and round-trip
            // back to the model on the next turn. On the
            // `PrematureClose` path `snapshot` is the empty default —
            // `assistant_content_from_snapshot` returns
            // `(vec![], vec![])` and the downstream commit lands as
            // an empty assistant message (mirrors the pre-enum
            // `unwrap_or_default()` behavior).
            let (content_parts, tool_calls) = assistant_content_from_snapshot(
                &snapshot,
                crate::tool_input_normalizer::ToolInputNormalizationContext {
                    session_id: Some(&self.config.session_id),
                    plans_dir: consts.plans_dir.as_deref(),
                    agent_id: self.config.agent_id.as_deref(),
                    cwd: None,
                },
            );
            let assistant_msg = Message::Assistant(coco_messages::AssistantMessage {
                message: LlmMessage::Assistant {
                    content: content_parts
                        .into_iter()
                        .map(convert_to_assistant_content)
                        .collect(),
                    provider_options: None,
                },
                uuid: uuid::Uuid::new_v4(),
                model: model_id.clone(),
                stop_reason: parsed_stop_reason,
                usage: Some(usage),
                cost_usd: None,
                // Streaming-path response.id is not currently plumbed
                // through `StreamEvent::Finish`. The marker uses the
                // anchor index (set by `MessageHistory::anchor_api_response`
                // after this push), so `request_id` is purely for trace
                // diagnostics today — `None` here is non-load-bearing.
                request_id: None,
                api_error: None,
            });

            // Recovery dispatch (Finding C2 / C4 / C22 / N1):
            // - `withheld_opt = Some(reason)` ⇒ the stream finished
            //   with a recoverable stop reason; the dispatcher decides
            //   how to act and consumes `assistant_msg` for whichever
            //   pushes the recovery branch needs.
            // - ContentFilter ⇒ refusal is a terminal policy decision,
            //   handled inline below (push assistant + synthetic, fall
            //   through to no-tool-calls).
            // - Otherwise ⇒ clean stream, normal commit with usage
            //   anchor.
            //
            // Multi-provider: `withhold_reason_for_stop` matches the
            // typed `StopReason` directly; every `vercel-ai-*` adapter
            // (Anthropic / OpenAI / Google / ByteDance / OpenAI-compat)
            // maps its raw signal to the same enum at the seam — no
            // wire-string sniffing reaches this layer.
            let withheld_opt = if tool_calls.is_empty() {
                parsed_stop_reason.and_then(crate::engine_stream_consume::withhold_reason_for_stop)
            } else {
                None
            };

            if let Some(withheld) = withheld_opt {
                match self
                    .run_post_stream_recovery(
                        withheld,
                        assistant_msg,
                        &mut *history,
                        &event_tx,
                        &mut turn_state,
                        // Pass the post-plan-swap client so escalate
                        // decisions read the same `ModelInfo` the next
                        // iteration's retry will hit. Without this,
                        // plan-mode could fire Phase-1 against the
                        // Main role's escalate ceiling while the actual
                        // retry runs through the Plan role's smaller
                        // (or no-escalate) `ModelInfo`.
                        &active_client,
                    )
                    .await
                {
                    crate::engine_recovery::RecoveryDisposition::Continue(transition) => {
                        turn_state.transition = Some(transition);
                        continue;
                    }
                    crate::engine_recovery::RecoveryDisposition::TerminateExhausted => {
                        // Dispatcher pushed assistant_msg + the
                        // synthetic api_error. Falling through to the
                        // no-tool-calls terminal lets Stop hooks (with
                        // the `isApiErrorMessage` short-circuit from
                        // commit 5) close out the turn.
                    }
                }
            } else if tool_calls.is_empty()
                && parsed_stop_reason == Some(coco_messages::StopReason::ContentFilter)
            {
                // Refusal — terminal policy decision; retry can't
                // change it. Push partial response (may contain the
                // model's refusal text) + the synthetic api_error
                // (carries the user-facing explanation), then fall
                // through to the no-tool-calls exit. TS parity:
                // `services/api/claude.ts:2258-2264` synthesizes at
                // the stream layer; coco-rs synthesizes here because
                // `coco-inference` is provider-agnostic.
                // Multi-provider: refusal / safety / recitation /
                // content_filter all unify to `StopReason::ContentFilter`
                // at the `vercel-ai-*` seam.
                warn!(
                    turn = turn_state.turn,
                    turn_id = %turn_id,
                    "content-filter / refusal — emitting api_error message and ending turn"
                );
                crate::history_sync::history_push_and_emit(history, assistant_msg, &event_tx).await;
                crate::history_sync::history_push_and_emit(
                    history,
                    crate::helpers::build_abnormal_stop_api_error_message(
                        coco_messages::StopReason::ContentFilter,
                        crate::engine_recovery::effective_max_tokens(&active_client, &turn_state),
                    ),
                    &event_tx,
                )
                .await;
            } else {
                // Atomic push + marker anchor: clean API response, so
                // we commit and anchor in one operation. Recovery and
                // ContentFilter branches above use the plain
                // `history_push_and_emit` so partial responses do not
                // anchor the marker.
                crate::history_sync::history_push_assistant_with_usage_and_emit(
                    history,
                    assistant_msg,
                    usage,
                    identity.clone(),
                    &event_tx,
                )
                .await;
            }

            // Streaming commit point: flush the StreamingHandle to
            // drain inflight safe tools, run queued unsafe tools
            // serially, apply patches in model-index order, and
            // push each outcome's ordered_messages into history.
            //
            // I12 note: outcomes surface in real completion order —
            // a slow earlier tool doesn't block a fast later one —
            // but `app_state_patch` apply is post-batch in
            // model-index order under one write lock (matches TS
            // `toolOrchestration.ts:54-62`).
            //
            // `streaming_executed` is the control-flow signal for
            // "this turn's tools all ran via streaming": we still
            // go through `finalize_turn_post_tools` + loop to the
            // next LLM call (unless the model produced no
            // tool_calls at all, in which case we fall through to
            // the `tool_calls.is_empty()` branch as before).
            let streaming_executed = streaming_ctx.is_some() && !tool_calls.is_empty();
            let mut streaming_control_prevent: Option<String> = None;
            // Collect ToolUseCompleted events to emit AFTER
            // commit_flush returns — the on_outcome callback is
            // synchronous (FnMut) and can't `.await`.
            let mut streaming_commits: Vec<(Vec<Message>, String, String, String, bool)> =
                Vec::new();
            if let Some(handle) = streaming_handle.take()
                && streaming_executed
            {
                let prevent_slot = &mut streaming_control_prevent;
                let commits_ref = &mut streaming_commits;
                let structured_slot = &mut acc.run_artifacts.structured_output;
                let attempts_slot = &mut acc.run_artifacts.structured_output_attempts;
                handle
                    .commit_flush(0, |outcome| {
                        let call_id = outcome.tool_use_id().to_string();
                        let tool_name_str = outcome.tool_id().to_string();
                        let is_error = outcome.error_kind().is_some();
                        let output_text = extract_streaming_result_text(outcome.ordered_messages());
                        if let Some(reason) = outcome.prevent_continuation()
                            && prevent_slot.is_none()
                        {
                            *prevent_slot = Some(reason.to_string());
                        }
                        let parts = outcome.into_parts();
                        if matches!(
                            parts.tool_id,
                            coco_types::ToolId::Builtin(coco_types::ToolName::StructuredOutput)
                        ) {
                            *attempts_slot = attempts_slot.saturating_add(1);
                        }
                        if let Some(data) = parts.structured_output.clone() {
                            *structured_slot = Some(data);
                        }
                        commits_ref.push((
                            parts.ordered_messages,
                            call_id,
                            tool_name_str,
                            output_text,
                            is_error,
                        ));
                    })
                    .await;
            }
            for (ordered_messages, call_id, tool_name, output, is_error) in streaming_commits {
                for msg in ordered_messages {
                    crate::history_sync::history_push_and_emit(history, msg, &event_tx).await;
                }
                let _ = emit_stream(
                    &event_tx,
                    crate::AgentStreamEvent::ToolUseCompleted {
                        call_id,
                        name: tool_name,
                        output,
                        is_error,
                    },
                )
                .await;
            }

            // If no tool calls, we're done — unless token-budget-continuation
            // is enabled and we're well under budget: inject a nudge and loop.
            // TS: `query.ts:1308-1340` `feature('TOKEN_BUDGET')` path.
            if tool_calls.is_empty() {
                // Structured-output retry-cap terminal. Mirrors TS
                // `QueryEngine.ts:1005-1047`: when the agent has
                // already attempted `MAX_STRUCTURED_OUTPUT_RETRIES`
                // (default 5) StructuredOutput calls without producing
                // a schema-conforming payload, terminate with the
                // `error_max_structured_output_retries` subtype rather
                // than letting the Stop function hook block forever.
                //
                // The matching "you MUST call StructuredOutput now"
                // re-prompt lives in the registered
                // `StructuredOutputEnforcement` function hook (see
                // `coco_cli::headless::inject_structured_output_tool_if_requested`).
                // That hook runs in the Stop block below; if it blocks,
                // the engine injects its `error_message` and re-loops
                // via the existing `StopHookBlocking` path. So the
                // engine here only has to enforce the *terminal* cap.
                if self.tools.get_by_name("StructuredOutput").is_some()
                    && acc.run_artifacts.structured_output.is_none()
                {
                    let max_retries = crate::config::max_structured_output_retries();
                    if acc.run_artifacts.structured_output_attempts >= max_retries {
                        warn!(
                            attempts = acc.run_artifacts.structured_output_attempts,
                            max_retries, "structured output retry cap exceeded"
                        );
                        // No fabricated stop_reason: the retry-cap exit
                        // path didn't necessarily resolve a model finish
                        // reason. Emit whatever was parsed (often `None`)
                        // and let consumers branch on it.
                        self.emit_successful_turn_completed(
                            &event_tx,
                            history,
                            usage,
                            cycle_turn_id.clone(),
                            parsed_stop_reason,
                        )
                        .await;
                        return Ok(make_query_result(
                            &consts,
                            acc,
                            turn_state,
                            response_text,
                            /*cancelled*/ false,
                            /*budget_exhausted*/ false,
                            Some("error_max_structured_output_retries".into()),
                            history.to_vec(),
                        ));
                    }
                }

                // TS `handleStopHooks` saves cache-safe params and
                // starts promptSuggestion before executing Stop hooks.
                // Keep transcript flush in the same helper so any
                // assistant text is resumable even if a Stop hook blocks
                // and the process exits before the retry turn.
                self.flush_successful_turn_state(&mut *history).await;
                self.maybe_spawn_prompt_suggestion_after_stop(&event_tx)
                    .await;

                // Stop hooks via the dispatcher (Finding C3 + TS parity
                // `query/stopHooks.ts`). The dispatcher owns the
                // `isApiErrorMessage` short-circuit that prevents the
                // error → block → retry → error death spiral, and
                // returns a typed `StopHookDecision` for the four
                // possible outcomes.
                let stop_decision = self
                    .run_stop_hooks(
                        &mut *history,
                        &event_tx,
                        hook_tx_opt.as_ref(),
                        &mut turn_state,
                        &response_text,
                    )
                    .await;
                match stop_decision {
                    crate::engine_stop_hooks::StopHookDecision::Prevented => {
                        // Note: no second `flush_successful_turn_state` here —
                        // the pre-`run_stop_hooks` flush at the entry of this
                        // no-tool-calls block (above) already wrote the
                        // transcript tail + cache-safe params; the `Prevented`
                        // branch of the dispatcher pushes nothing new, so a
                        // repeat flush would just overwrite the cache snapshot
                        // and re-walk the dedup set for zero benefit.
                        self.emit_successful_turn_completed(
                            &event_tx,
                            history,
                            usage,
                            cycle_turn_id.clone(),
                            parsed_stop_reason,
                        )
                        .await;
                        return Ok(make_query_result(
                            &consts,
                            acc,
                            turn_state,
                            response_text,
                            /*cancelled*/ false,
                            /*budget_exhausted*/ false,
                            Some("stop_hook_prevented".into()),
                            history.to_vec(),
                        ));
                    }
                    crate::engine_stop_hooks::StopHookDecision::BlockedContinueLoop => {
                        // Dispatcher pushed feedback + flushed transcript +
                        // set `turn_state.transition = StopHookBlocking` +
                        // `turn_state.stop_hook_active = true`.
                        continue;
                    }
                    crate::engine_stop_hooks::StopHookDecision::SkippedApiError { error_type } => {
                        // C3 death-spiral guard fired — last assistant
                        // message is api_error. Fall through to end-turn
                        // emit; skip token-budget continuation (no point
                        // in retrying into another error).
                        //
                        // Finding **R1** — use the api_error's typed
                        // `error_type` as the QueryResult.stop_reason so
                        // SDK consumers see the specific code
                        // (`prompt_too_long` / `max_output_tokens` /
                        // `content_filter` / `invalid_request` / …)
                        // instead of the generic `end_turn_api_error`
                        // bucket. TS parity:
                        // `query.ts:1175 return { reason: 'prompt_too_long' }`
                        // / `query.ts:1182` / etc. `None` means the
                        // synthesis site didn't classify — fall back to
                        // the legacy label.
                        let stop_reason = error_type.unwrap_or_else(|| "end_turn_api_error".into());
                        info!(
                            turn = turn_state.turn,
                            stop_reason = %stop_reason,
                            "ending turn early — last message is api_error (C3 guard)"
                        );
                        self.emit_successful_turn_completed(
                            &event_tx,
                            history,
                            usage,
                            cycle_turn_id.clone(),
                            parsed_stop_reason,
                        )
                        .await;
                        return Ok(make_query_result(
                            &consts,
                            acc,
                            turn_state,
                            response_text,
                            /*cancelled*/ false,
                            /*budget_exhausted*/ false,
                            Some(stop_reason),
                            history.to_vec(),
                        ));
                    }
                    crate::engine_stop_hooks::StopHookDecision::Continue => {
                        // No hooks or all passed; fall through to token-budget
                        // continuation check.
                    }
                }

                if self.config.enable_token_budget_continuation
                    && should_continue_for_budget(&turn_state.budget)
                {
                    let pct = budget_pct_used(&turn_state.budget);
                    let nudge = format!(
                        "Token budget continuation: you've used {pct}% of the turn budget. \
                         Keep going — don't summarize or recap, just continue the work."
                    );
                    crate::history_sync::history_push_and_emit(
                        history,
                        coco_messages::create_meta_message(&nudge),
                        &event_tx,
                    )
                    .await;
                    turn_state.budget.record_continuation();
                    turn_state.transition = Some(ContinueReason::TokenBudgetContinuation);
                    // TS parity query.ts:1332-1336 — token-budget
                    // continuation is a fresh user-prompt-style attempt
                    // and resets every per-incident counter:
                    //   * `stopHookActive: undefined` (R2)
                    //   * `maxOutputTokensRecoveryCount: 0` (R4)
                    //   * `hasAttemptedReactiveCompact: false` (R4 —
                    //     mirrored by clearing the `ReactiveCompactState`
                    //     circuit-breaker so the next overflow gets a
                    //     fresh 3-attempt budget).
                    //
                    // TS `maxOutputTokensOverride: undefined` (query.ts:1334)
                    // has no Rust counterpart — the slot field was deleted
                    // when escalate became a derived property of
                    // `ModelInfo.max_output_tokens_escalate` + the per-turn
                    // `transition`. The next iteration's transition will be
                    // `TokenBudgetContinuation`, not `MaxOutputTokensEscalate`,
                    // so `effective_max_tokens` returns `None` naturally.
                    turn_state.stop_hook_active = false;
                    turn_state.max_tokens_recovery_count = 0;
                    {
                        let mut state = self.reactive_state.lock().await;
                        state.reset();
                    }
                    info!(turn = turn_state.turn, pct, "token budget continuation");
                    continue;
                }
                info!(
                    turn = turn_state.turn,
                    response_chars = response_text.len(),
                    tokens_in = usage.input_tokens.total,
                    tokens_out = usage.output_tokens.total,
                    "no tool calls, conversation complete"
                );
                self.emit_successful_turn_completed(
                    &event_tx,
                    history,
                    usage,
                    cycle_turn_id.clone(),
                    parsed_stop_reason,
                )
                .await;
                return Ok(make_query_result(
                    &consts,
                    acc,
                    turn_state,
                    response_text,
                    /*cancelled*/ false,
                    /*budget_exhausted*/ false,
                    Some("end_turn".into()),
                    history.to_vec(),
                ));
            }

            // Note: queued steering messages are drained at end-of-turn
            // (`engine_finalize_turn::finalize_turn_post_tools` →
            // `drain_command_queue_into_history` with priority=Later, which
            // is the upper bound and includes Now / Next / Later items).
            // An earlier mid-turn `Now`-only drain lived here, but it ran
            // BEFORE the non-streaming `ToolCallRunner` produced
            // tool_results — inserting a User message between the
            // assistant's `tool_use` and its matching `tool_result` and
            // breaking pairing on providers that enforce it (Anthropic
            // 400). The end-of-turn drain happens after tools complete on
            // both streaming + non-streaming paths and still surfaces
            // queued items on the very next API call. TS parity:
            // `query.ts:1547-1643` snapshots the queue post-tool, yields
            // attachments, then dequeues — all after tool_results are in
            // place.

            // Streaming-executed fast path: the StreamingHandle
            // already ran every tool and pushed their
            // ordered_messages into history. Skip the non-streaming
            // runner, but still run finalize_turn_post_tools so
            // the command-queue drain / auto-compact / TurnCompleted
            // emission happens, then continue the loop.
            if streaming_executed {
                // Drain end-of-batch nested-memory triggers BEFORE
                // finalize so the next turn's reminder build picks
                // them up. TS parity:
                // `getNestedMemoryAttachments` runs in
                // `getAttachmentMessages` between tool batch and
                // next API call.
                if let Some(ref c) = streaming_ctx {
                    self.drain_nested_memory_triggers(c).await;
                }
                let continuation = if streaming_control_prevent.is_some() {
                    crate::engine_finalize_turn::TurnContinuation::Terminal
                } else {
                    crate::engine_finalize_turn::TurnContinuation::Continuing
                };
                self.finalize_turn_post_tools(
                    &mut *history,
                    &event_tx,
                    usage,
                    continuation,
                    cycle_turn_id.clone(),
                    parsed_stop_reason,
                )
                .await;
                if let Some(ref c) = streaming_ctx {
                    self.drain_dynamic_skill_triggers(c, &mut *history, &event_tx)
                        .await;
                }
                if let Some(stop_reason) = streaming_control_prevent {
                    return Ok(make_query_result(
                        &consts,
                        acc,
                        turn_state,
                        response_text,
                        /*cancelled*/ false,
                        /*budget_exhausted*/ false,
                        Some(stop_reason),
                        history.to_vec(),
                    ));
                }
                turn_state.transition = Some(ContinueReason::NextTurn);
                continue;
            }

            // Execute tool calls via StreamingToolExecutor (batch partitioning).
            // User-message id flows through the factory so the file-history
            // snapshot keys on the turn's triggering message, not a later
            // tool result. The factory installs a `QueryHookHandle` into
            // `ToolUseContext` when hooks are configured so tool callbacks
            // that need PreToolUse/PostToolUse use the same pipeline as the
            // runner.
            let ctx_supports_tool_reference = services
                .runtime
                .current_client()
                .model_info()
                .is_some_and(|info| {
                    info.has_capability(coco_types::Capability::ServerSideToolReference)
                });
            let ctx_supports_client_side_tool_search = services
                .runtime
                .current_client()
                .model_info()
                .is_some_and(|info| {
                    info.has_capability(coco_types::Capability::ClientSideToolSearch)
                });
            let ctx = self
                .tool_context_factory(hook_tx_opt.as_ref())
                .build(crate::tool_context::ToolContextOverrides {
                    user_message_id: Some(consts.user_uuid.clone()),
                    progress_tx: Some(services.progress_tx.clone()),
                    current_model_id: Some(services.runtime.current_model_id().to_string()),
                    current_model_supports_tool_reference: ctx_supports_tool_reference,
                    current_model_supports_client_side_tool_search:
                        ctx_supports_client_side_tool_search,
                    messages_snapshot: Some(messages_snapshot.clone()),
                })
                .await;

            let tool_run_outcome = ToolCallRunner {
                event_tx: &event_tx,
                history: &mut *history,
                ctx: &ctx,
                tool_calls: &tool_calls,
                turn: turn_state.turn,
                tools: &self.tools,
                hooks: self.hooks.as_ref(),
                orchestration_ctx: self.orchestration_ctx(),
                hook_tx_opt: hook_tx_opt.as_ref(),
                permission_denials: &mut acc.permission_denials,
                state_tracker,
                permission_bridge: self.permission_bridge.as_ref(),
                session_id: &self.config.session_id,
                cancel: &self.cancel,
                auto_mode_state: self.auto_mode_state.as_ref(),
                denial_tracker: self.denial_tracker.as_ref(),
                client: &self.client,
                auto_mode_rules: &self.auto_mode_rules,
                app_state: self.app_state.as_ref(),
                permission_rule_handle: &self.permission_rule_handle,
            }
            .run()
            .await;
            // Drain end-of-batch nested-memory triggers BEFORE finalize
            // so the next turn's reminder build picks them up. TS:
            // `getNestedMemoryAttachments` runs between batch + next
            // API call.
            self.drain_nested_memory_triggers(&ctx).await;
            if let Some(data) = tool_run_outcome.structured_output.clone() {
                acc.run_artifacts.structured_output = Some(data);
            }
            acc.run_artifacts.structured_output_attempts = acc
                .run_artifacts
                .structured_output_attempts
                .saturating_add(tool_run_outcome.structured_output_attempts);
            let continuation = if tool_run_outcome.continue_after_tools {
                crate::engine_finalize_turn::TurnContinuation::Continuing
            } else {
                crate::engine_finalize_turn::TurnContinuation::Terminal
            };
            self.finalize_turn_post_tools(
                &mut *history,
                &event_tx,
                usage,
                continuation,
                cycle_turn_id.clone(),
                parsed_stop_reason,
            )
            .await;
            self.drain_dynamic_skill_triggers(&ctx, &mut *history, &event_tx)
                .await;
            if !tool_run_outcome.continue_after_tools {
                return Ok(make_query_result(
                    &consts,
                    acc,
                    turn_state,
                    response_text,
                    /*cancelled*/ false,
                    /*budget_exhausted*/ false,
                    tool_run_outcome.stop_reason_override,
                    history.to_vec(),
                ));
            }
            turn_state.transition = Some(ContinueReason::NextTurn);
        }
    }
}

// Free helpers, `StreamingToolCallBuffer`, and `ProgressThrottle` live in
// `crate::engine_helpers`; re-imported at the top of this module so tests
// can still resolve them via `super::name`. Other impl blocks for
// `QueryEngine` live in:
// - `crate::engine_builder` — `new()` + every `with_*` builder + accessors;
// - `crate::engine_session` — public entry points + lifecycle + hook forwarder;
// - `crate::engine_compaction` — `try_full_compact` / SM / `run_manual_compact`;
// - `crate::engine_finalize_turn` — tail-of-turn compact ladder + reactive recovery;
// - `crate::engine_prompt` — `build_prompt` / tool-defs / context factory / date-change;
// - `crate::engine_turn_reminders` — per-turn reminder pipeline.

/// Rebuild the assistant `Vec<AssistantContentPart>` plus the
/// `Vec<ToolCallPart>` view of completed tool calls from a per-turn
/// snapshot produced by `coco-inference::process_stream_with_config`.
///
/// Walks `snapshot.parts` in emission order so the resulting vector
/// preserves the original text↔reasoning↔tool sequence (matters for
/// Gemini-3, which interleaves freely). Each part carries its own
/// `provider_metadata` — signatures and equivalents round-trip verbatim.
///
/// Tool-call filter: include when `is_input_complete || is_complete`.
/// Some providers (and the synthetic mock helper) only emit
/// `ToolInputStart`/`Delta`/`End` without the canonical `ToolCall(tc)`
/// close, so requiring `is_complete` would silently drop valid tool
/// calls. Malformed JSON is skipped with a `tracing::warn!`,
/// matching the previous behavior on the buffer-driven reconstruction
/// path.
pub(crate) fn assistant_content_from_snapshot(
    snapshot: &coco_inference::AssistantTurnSnapshot,
    normalizer_ctx: crate::tool_input_normalizer::ToolInputNormalizationContext<'_>,
) -> (Vec<AssistantContentPart>, Vec<ToolCallPart>) {
    let mut content_parts: Vec<AssistantContentPart> = Vec::with_capacity(snapshot.parts.len());
    let mut tool_calls: Vec<ToolCallPart> = Vec::new();

    for part in &snapshot.parts {
        match part {
            coco_inference::TurnPart::Text(t) => {
                if t.text.is_empty() {
                    continue;
                }
                content_parts.push(AssistantContentPart::Text(TextPart {
                    text: t.text.clone(),
                    provider_metadata: t.provider_metadata.clone(),
                }));
            }
            coco_inference::TurnPart::Reasoning(r) => {
                if r.text.is_empty() {
                    continue;
                }
                content_parts.push(AssistantContentPart::Reasoning(ReasoningPart {
                    text: r.text.clone(),
                    provider_metadata: r.provider_metadata.clone(),
                }));
            }
            coco_inference::TurnPart::ToolCall(tc) => {
                if !(tc.is_input_complete || tc.is_complete) {
                    warn!(tool_call_id = %tc.id, "tool call did not complete");
                    continue;
                }
                // Parse with repair, falling back to `Value::Object({})`
                // on failure. See streaming-path commentary above —
                // schema validation reports specific missing
                // fields rather than a generic "JSON broken", and the
                // tool_use/tool_result pairing invariant is preserved
                // because we never drop the call.
                let parsed_input = crate::tool_input_parse::parse_tool_arguments_or_empty(
                    &tc.input_json,
                    &tc.tool_name,
                );
                let input = crate::tool_input_normalizer::normalize_observable_tool_input(
                    &tc.tool_name,
                    parsed_input,
                    normalizer_ctx,
                );
                // Carry the wire-level `invalid` + `invalid_reason`
                // through reconstruction. Provider adapters set these
                // when wire parsing detects an unrecoverable JSON parse
                // (Anthropic streaming `content_block_stop` flush,
                // etc.); without this carry-through the agent loop's
                // synthetic `<tool_use_error>` wrap would fall back to
                // a generic message.
                let tcp = ToolCallPart {
                    tool_call_id: tc.id.clone(),
                    tool_name: tc.tool_name.clone(),
                    input,
                    provider_executed: tc.provider_executed,
                    invalid: tc.invalid,
                    invalid_reason: tc.invalid_reason.clone(),
                    provider_metadata: tc.provider_metadata.clone(),
                };
                content_parts.push(AssistantContentPart::ToolCall(tcp.clone()));
                tool_calls.push(tcp);
            }
            // File / ReasoningFile / Source / Custom / ToolApprovalRequest
            // are not yet round-tripped through assistant history. Adding
            // them here is a follow-up; for now they're emission-only via
            // the live UI stream events. Drop with a trace.
            other => {
                tracing::trace!(?other, "snapshot variant not yet reconstructed");
            }
        }
    }

    (content_parts, tool_calls)
}

/// Per-run side-channel collectors filled at emission sites so finalize
/// doesn't need to scan history.
///
/// `structured_output` is set when a tool returns `with_structured_output(...)`
/// (via `UnstampedToolCallOutcome.structured_output`). `max_turns_reached` is
/// set when the engine hits the configured `max_turns` cap. Both feed
/// `QueryResult` — and from there `SessionResultParams` — without re-deriving
/// state from `history`, which mid-run compaction can replace.
#[derive(Default, Clone)]
pub(crate) struct RunArtifacts {
    pub structured_output: Option<serde_json::Value>,
    pub max_turns_reached: Option<coco_messages::MaxTurnsReachedPayload>,
    /// Count of `StructuredOutput` tool invocations made by the model
    /// during this query (successful + failed). Used to enforce
    /// `COCO_MAX_STRUCTURED_OUTPUT_RETRIES` and to decide whether to
    /// re-inject the "you MUST call this tool" nudge when the model
    /// tries to end the turn without a successful structured response.
    /// TS parity: `countToolCalls(messages, SYNTHETIC_OUTPUT_TOOL_NAME) - initialStructuredOutputCalls`
    /// in `QueryEngine.ts:1004-1014`.
    pub structured_output_attempts: u32,
}

/// Pure constructor for [`QueryResult`], factored out of `run_session_loop`.
/// Consumes the loop's [`LoopAccumulator`] / [`LoopTurnState`] (terminal
/// callers always `return` immediately after invoking) and borrows
/// [`LoopConstants`] for the `consts.started_at` read.
#[allow(clippy::too_many_arguments)]
fn make_query_result(
    consts: &LoopConstants,
    acc: LoopAccumulator,
    turn_state: LoopTurnState,
    response_text: String,
    cancelled: bool,
    budget_exhausted: bool,
    stop_reason: Option<String>,
    final_messages: Vec<std::sync::Arc<Message>>,
) -> QueryResult {
    QueryResult {
        response_text,
        turns: turn_state.turn,
        total_usage: acc.total_usage,
        cost_tracker: acc.cost_tracker,
        cancelled,
        budget_exhausted,
        last_continue_reason: turn_state.transition,
        duration_ms: consts.started_at.elapsed().as_millis() as i64,
        duration_api_ms: acc.api_time_ms,
        stop_reason,
        permission_denials: acc.permission_denials,
        final_messages,
        structured_output: acc.run_artifacts.structured_output,
        max_turns_reached: acc.run_artifacts.max_turns_reached,
    }
}

#[cfg(test)]
#[path = "engine.test.rs"]
mod tests;

#[cfg(test)]
#[path = "engine_live_rules_scoping.test.rs"]
mod engine_live_rules_scoping_tests;
