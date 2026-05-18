//! The agent loop — heart of the system.
//!
//! TS: QueryEngine.ts + query.ts
//!
//! State transitions tracked via ContinueReason to enable tests to verify
//! recovery paths without inspecting message contents.

use crate::budget::BudgetDecision;
use crate::budget::BudgetTracker;
use crate::command_queue::CommandQueue;
use crate::emit::emit_protocol;
use crate::emit::emit_stream;
use crate::session_state::SessionStateTracker;
use crate::tool_call_runner::ToolCallRunner;
use coco_config::EnvKey;
use coco_config::env;
use coco_context::FileHistoryState;
use coco_hooks::HookRegistry;
use coco_hooks::orchestration;
use coco_inference::ApiClient;
use coco_inference::QueryParams;
use coco_inference::StreamEvent;
use coco_messages::CostTracker;
use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_system_reminder::SystemReminderOrchestrator;
use coco_tool_runtime::ToolRegistry;
use coco_tool_runtime::ToolUseContext;
use coco_types::TokenUsage;
use coco_types::ToolAppState;

use crate::helpers::budget_pct_used;
use crate::helpers::convert_to_assistant_content;
use crate::helpers::extract_last_assistant_text;
use crate::helpers::should_continue_for_budget;

use coco_inference::AssistantContentPart;
use coco_inference::ReasoningPart;
use coco_inference::TextPart;
use coco_inference::ToolCallPart;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing::warn;

// Items from `crate::engine_helpers` used directly by `run_session_loop`.
// `pub(crate) use` (rather than plain `use`) re-exposes them as members of
// this module so `engine.test.rs` can keep resolving them via `super::name`.
pub(crate) use crate::engine_helpers::ProgressThrottle;
pub(crate) use crate::engine_helpers::StreamingToolCallBuffer;
pub(crate) use crate::engine_helpers::drain_one_progress;
pub(crate) use crate::engine_helpers::emit_model_fallback_notice;
pub(crate) use crate::engine_helpers::extract_streaming_result_text;
pub(crate) use crate::engine_helpers::is_capacity_error_message;
// Test-only re-export — `engine.test.rs` references this directly via
// `super::classify_progress_payload`, but no production call site does.
#[cfg(test)]
pub(crate) use crate::engine_helpers::classify_progress_payload;

pub use crate::config::ContinueReason;
use crate::config::ESCALATED_MAX_TOKENS;
use crate::config::MAX_OUTPUT_TOKENS_RECOVERY_LIMIT;
pub use crate::config::QueryEngineConfig;
pub use crate::config::QueryResult;
pub use crate::config::SessionBootstrap;

/// Last-compact tracker for `RecompactionInfo` population. Captured by
/// `try_full_compact` after a successful compact and read by the next
/// compaction to derive `is_recompaction` / `turns_since_previous`.
/// TS parity: `compact.ts:317-323 RecompactionInfo`.
#[derive(Debug, Clone)]
pub(crate) struct LastCompactState {
    /// Turn number at which the previous compaction completed.
    pub(crate) turn_id: i64,
    /// Run id of the previous compaction (UUID-shaped). Mirrors TS
    /// `previousCompactTurnId`. Currently used only for transcript
    /// observability since Rust has no `tengu_compact` analytics.
    #[allow(dead_code)]
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
    /// generate a `ToolUseSummaryMessage` for SDK / mobile UI consumers
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
            Option<tokio::task::JoinHandle<Option<coco_messages::ToolUseSummaryMessage>>>,
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
    /// Session-scoped tool-input schema validator. One instance per
    /// engine so compiled validators cache across turns. Plan I3's
    /// Rust-side tightening — preparer runs this on both model
    /// input and any PreToolUse hook-rewritten input.
    pub(crate) tool_schema_validator: coco_tool_runtime::ToolSchemaValidator,
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
    pub(crate) task_handle: Option<coco_tool_runtime::TaskHandleRef>,
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
    /// `compact.ts:317-323`). Set after each successful full compact.
    /// `turn_id` is a monotonic per-engine counter so `turns_since_previous`
    /// can be derived without external clocks.
    pub(crate) last_compact_state: Arc<std::sync::Mutex<Option<LastCompactState>>>,
    /// Monotonic turn counter incremented on every `finalize_turn_post_tools`.
    /// Used to compute `RecompactionInfo.turns_since_previous`.
    pub(crate) turn_counter: Arc<std::sync::atomic::AtomicI64>,
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
    pub(crate) async fn run_session_loop(
        &self,
        turn_messages: Vec<Message>,
        event_tx: Option<tokio::sync::mpsc::Sender<crate::CoreEvent>>,
        state_tracker: &SessionStateTracker,
        hook_tx_opt: Option<tokio::sync::mpsc::Sender<coco_hooks::HookExecutionEvent>>,
        history: &mut MessageHistory,
    ) -> Result<QueryResult, coco_error::BoxedError> {
        let start_time = std::time::Instant::now();
        let mut api_time_ms: i64 = 0;
        let mut total_usage = TokenUsage::default();
        let mut cost_tracker = CostTracker::new();

        // Build the per-session ModelRuntime. When the caller
        // installed fallback clients via `with_fallback_client(s)`,
        // the runtime holds a multi-slot chain and walks it on
        // capacity-error streaks via `advance()`.
        //
        // Fallback trigger (TS parity, `services/api/withRetry.ts:335`):
        // after `MAX_529_RETRIES` consecutive `Overloaded` (529/503)
        // responses from the active slot, the next turn advances to
        // the next slot. The engine tracks consecutive capacity
        // errors because provider-layer retries are internal to the
        // vercel-ai crates — this counter only ticks when the retry
        // layer gives up and surfaces an error to us.
        let mut model_runtime = crate::model_runtime::ModelRuntime::new(
            self.client.clone(),
            self.fallback_clients.clone(),
        );
        if let Some(policy) = self.recovery_policy {
            model_runtime = model_runtime.with_recovery_policy(policy);
        }
        /// TS: `MAX_529_RETRIES = 3` in `services/api/withRetry.ts:54`.
        const MAX_CONSECUTIVE_CAPACITY_ERRORS: i32 = 3;
        let mut consecutive_capacity_errors: i32 = 0;
        // TS `input`-parameter parity: tracks the UUID of the last user
        // message that has already been handed to the UserPrompt-tier
        // reminders. Prevents duplicate `at_mentioned_files` /
        // `agent_mentions` / `ultrathink_effort` emissions across
        // tool-result iterations of the same human turn.
        let mut reminder_last_user_input_uuid: Option<uuid::Uuid> = None;
        let mut turn = 0;
        let mut last_continue_reason: Option<ContinueReason> = None;
        // TS `stop_hook_active`: set to true once a Stop hook has
        // blocked the loop, so subsequent Stop firings advertise the
        // re-entry to the hook. Mirrors `query.ts handleStopHooks()`.
        let mut stop_hook_active = false;
        // max-output-tokens recovery state (TS: query.ts State.maxOutputTokensOverride + maxOutputTokensRecoveryCount)
        let mut max_tokens_override: Option<i64> = None;
        let mut max_tokens_recovery_count: i32 = 0;
        let mut budget = BudgetTracker::new(
            self.config.max_tokens,
            self.config.max_turns,
            /*max_continuations*/ 3,
        );
        // The "current turn" user message id is the LAST user message in
        // `turn_messages`. In single-turn mode the list is
        // `[user_msg, attachment, ...]` and the first (and only) user
        // message is also the last. In multi-turn SDK mode the list is
        // `[prior_history..., new_user_msg]`, so the LAST user message
        // is the current turn's prompt — which is what file history
        // snapshots should key on.
        let user_msg_uuid = turn_messages
            .iter()
            .rev()
            .find_map(|m| match m {
                Message::User(u) => Some(u.uuid.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        for msg in turn_messages {
            history.push(msg);
        }

        // NOTE: `SessionStarted` + `SessionStateChanged(Running)` + the
        // hook → CoreEvent forwarder are set up by the outer
        // `run_internal_with_messages` BEFORE calling this function, so
        // SDK consumers see them even if the session loop errors out
        // before its first turn. See TS `runHeadless()` which initializes
        // the init message at the very top of the entry function.

        // ── Progress-event forwarder ──
        //
        // Spawn one drain task per session. Tools send `ToolProgress`
        // updates through `ctx.progress_tx`; the drain fans them out
        // to:
        //
        //   1. `TuiOnlyEvent::ToolProgress { tool_use_id, data }` —
        //      every event, unthrottled, carries the raw payload for
        //      the TUI to render progress bars or byte counts.
        //
        //   2. `ServerNotification::ToolProgress(ToolProgressParams)` —
        //      TS-parity wire event. Only emitted for
        //      `bash_progress` / `powershell_progress` payload types
        //      and throttled to ≤1 per 30 s per
        //      `parent_tool_use_id` (or `tool_use_id` if the parent
        //      is absent), matching `utils/queryHelpers.ts:99-189`.
        //
        // TS parity: `onProgress` in `StreamingToolExecutor` loops
        // progress yielded from the tool generator back to the
        // streaming UI; `normalizeMessage` throttles the SDK-facing
        // version separately. Rust collapses both into one drain
        // task because there's no separate normalization stage.
        //
        // Lifecycle: the tx is cloned into every `ToolUseContext`
        // built for this session. When the session loop exits, the
        // last tx clone (owned here) drops, the rx closes, and the
        // drain task finishes naturally — no explicit await needed.
        let (progress_tx_session, mut progress_rx_session) =
            tokio::sync::mpsc::unbounded_channel::<coco_tool_runtime::ToolProgress>();
        let progress_event_tx = event_tx.clone();
        let _progress_drain = tokio::spawn(async move {
            let mut throttle = ProgressThrottle::new();
            while let Some(progress) = progress_rx_session.recv().await {
                drain_one_progress(&progress_event_tx, progress, &mut throttle).await;
            }
        });

        // Create file history snapshot for this user message.
        // TS: fileHistoryMakeSnapshot() in handlePromptSubmit.ts + QueryEngine.ts
        if let (Some(fh), Some(ch)) = (&self.file_history, &self.config_home) {
            let mut fh = fh.write().await;
            if let Err(e) = fh
                .make_snapshot(&user_msg_uuid, ch, &self.config.session_id)
                .await
            {
                warn!("file history make_snapshot failed: {e}");
            }
        }

        // Permission denials accumulated across all tool calls in this session.
        // Populated on each `PermissionDecision::Deny` branch and flushed
        // into `SessionResultParams.permission_denials` via the `make_query_result`
        // closure. Matches TS `QueryEngine.permissionDenials` wrapper
        // behavior (QueryEngine.ts:244-271).
        let mut permission_denials: Vec<coco_types::PermissionDenialInfo> = Vec::new();

        // Plan-mode reminder tracker — injects the system-reminder at the
        // start of every turn while plan mode is active and on the turn
        // following an ExitPlanMode approval. TS: normalizeAttachmentForAPI
        // cases `plan_mode` / `plan_mode_exit` / `plan_mode_reentry`.
        // Plan/workflow / phase-4 / agent-count values are fed into the
        // orchestrator's `TurnReminderInput` below. `PlanModeReminder` is
        // now the per-turn side-effect driver (mode reconcile + mailbox
        // polling + leader-pending-approvals) and no longer owns
        // workflow state.
        let plans_dir = crate::plan_mode_reminder::PlanModeReminder::resolve_plans_dir(
            self.config_home.as_deref(),
            self.config.project_dir.as_deref(),
            self.config.plans_directory.as_deref(),
        );
        let mut plan_reminder = crate::plan_mode_reminder::PlanModeReminder::new(
            self.config.permission_mode,
            Some(self.config.session_id.clone()),
            self.config.agent_id.clone(),
            plans_dir.clone(),
            self.app_state.clone(),
        );
        // Wire mailbox for swarm polling if identity is set and a mailbox
        // handle is installed. Agent + team names come from env vars
        // (set by the swarm spawner); mirror `swarm_identity::get_agent_name`
        // env fallback. We keep the env read here rather than threading
        // via ctx because the reminder is engine-level (no ToolUseContext).
        // Env namespace is `COCO_*` — see swarm_constants.
        let agent_name_env = env::env_opt(EnvKey::CocoAgentName);
        let team_name_env = env::env_opt(EnvKey::CocoTeamName);
        if let (Some(mbox), Some(agent), Some(team)) =
            (self.mailbox.clone(), agent_name_env, team_name_env)
        {
            plan_reminder = plan_reminder.with_mailbox(
                mbox,
                agent,
                team,
                self.config.is_teammate && self.config.plan_mode_required,
            );
        }
        // Install the protocol-event sink so leader-pending-approval
        // polling can surface `PlanApprovalRequested` to the TUI in
        // addition to injecting the LLM-prompt attachment. Absent sink
        // (SDK-only / headless) means the overlay simply never fires.
        if let Some(tx) = event_tx.clone() {
            plan_reminder = plan_reminder.with_event_sink(tx);
        }

        // System-reminder orchestrator — owns reminder emission for the
        // whole session. The orchestrator is Send+Sync and accumulates
        // per-attachment throttle state across turns.
        //
        // `plan_reminder` above is retained for non-reminder side effects
        // (mode reconciliation, teammate mailbox polling, leader-pending-
        // approvals), called per turn via `turn_start_side_effects_only`.
        // The reminder emission itself (plan/auto/todo/task/critical/
        // compaction/date-change) moves here.
        // Settings-driven reminder config (TS `settings.json` →
        // `coco_config::Settings.system_reminder`). Cloned because the
        // orchestrator owns its own copy for the session — subsequent
        // settings reloads won't retroactively disable reminders until
        // the next engine build.
        let reminder_config = self.config.system_reminder.clone();
        let reminder_orchestrator =
            SystemReminderOrchestrator::new(reminder_config).with_default_generators();
        // Todo-list lookup key: TS `agentId ?? sessionId`.
        let reminder_todo_key = self
            .config
            .agent_id
            .clone()
            .unwrap_or_else(|| self.config.session_id.clone());
        // Model context window — exposed to the compaction reminder
        // generator. Effective = 90% of window (reserve 10% for output),
        // matching the same approximation `coco-compact` uses.
        let reminder_context_window = self.config.context_window;
        let reminder_effective_window = (reminder_context_window * 9) / 10;

        loop {
            if self.cancel.is_cancelled() {
                // TS parity: append `[Request interrupted by user]`
                // unless the mid-stream cancel branch (line ~1702)
                // already appended it for this turn. Detection: the
                // last message is a User whose text matches one of
                // the interrupt markers — mirrors TS's idempotent
                // `createUserInterruptionMessage` placement.
                if !last_message_is_interrupt_marker(history) {
                    history.push(coco_messages::create_user_interruption_message(
                        /*for_tool_use*/ false,
                    ));
                }
                return Ok(make_query_result(
                    String::new(),
                    turn,
                    total_usage,
                    cost_tracker,
                    /*cancelled*/ true,
                    /*budget_exhausted*/ false,
                    last_continue_reason,
                    start_time,
                    api_time_ms,
                    Some("cancelled".into()),
                    permission_denials,
                    history.messages.clone(),
                ));
            }

            // Drain the prior turn's tool-use-summary side-fork (TS
            // `query.ts:1055-1060` — await `pendingToolUseSummary` at
            // the top of the next iteration). 2s hard cap; never
            // blocks the new turn for more than that. Silent no-op
            // when no pending handle exists (first iteration, or
            // previous turn had no tool batch).
            self.drain_pending_tool_use_summary(&mut *history, &event_tx)
                .await;

            // Budget check before each turn
            match budget.check(turn) {
                BudgetDecision::Stop { reason } => {
                    warn!(%reason, "budget stop");
                    let last_text = extract_last_assistant_text(history);
                    return Ok(make_query_result(
                        last_text,
                        turn,
                        total_usage,
                        cost_tracker,
                        /*cancelled*/ false,
                        /*budget_exhausted*/ true,
                        last_continue_reason,
                        start_time,
                        api_time_ms,
                        Some("budget_exhausted".into()),
                        permission_denials,
                        history.messages.clone(),
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

            turn += 1;
            let turn_id = format!("turn-{turn}");

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
                    history.clear();
                    info!(turn, "plan-mode exit cleared conversation history");
                }
            }

            // The `turn` canonical anchor cannot be a single async span guard
            // here: this loop body has many `.await` points, and
            // `EnteredSpan` is sync-only. Per-step turn correlation is
            // provided via the `turn` / `turn_id` fields stamped on each
            // structured event below — pivots on `turn_id` reconstruct the
            // turn without an enclosing span.
            info!(
                turn,
                turn_id = %turn_id,
                history_len = history.messages.len(),
                active_model = model_runtime.current_model_id(),
                "turn start"
            );
            let _delivered = emit_protocol(
                &event_tx,
                crate::ServerNotification::TurnStarted(coco_types::TurnStartedParams {
                    turn_id: Some(turn_id.clone()),
                    turn_number: turn,
                }),
            )
            .await;

            // Turn-start reminder pipeline (Phase D.3) — runs the five-phase
            // reminder build / orchestrate / bookkeeping / inject sequence
            // and returns the `app_state` snapshot used by `build_tool_definitions`
            // below. The full implementation lives in
            // `crate::engine_turn_reminders` to keep this loop legible.
            let app_state_snapshot = self
                .run_turn_reminder_pipeline(crate::engine_turn_reminders::TurnReminderContext {
                    history: &mut *history,
                    plan_reminder: &mut plan_reminder,
                    orchestrator: &reminder_orchestrator,
                    last_user_input_uuid: &mut reminder_last_user_input_uuid,
                    total_usage: &total_usage,
                    cost_tracker: &cost_tracker,
                    todo_key: &reminder_todo_key,
                    context_window: reminder_context_window,
                    effective_window: reminder_effective_window,
                })
                .await;

            // Build prompt from history
            let prompt = self.build_prompt(history).await;
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
            // Escalation takes the MAX of the override and the user config so
            // we never DOWNGRADE a user-configured higher limit (e.g. user
            // set 128k, override says 64k → keep 128k, already sufficient).
            let effective_max_tokens = match (max_tokens_override, self.config.max_tokens) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (Some(v), None) | (None, Some(v)) => Some(v),
                (None, None) => None,
            };
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

            let params = QueryParams {
                prompt,
                max_tokens: effective_max_tokens,
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
                let current_supports_tool_reference = model_runtime
                    .current_client()
                    .model_info()
                    .is_some_and(|info| {
                        info.has_capability(coco_types::Capability::ServerSideToolReference)
                    });
                let current_supports_client_side_tool_search = model_runtime
                    .current_client()
                    .model_info()
                    .is_some_and(|info| {
                        info.has_capability(coco_types::Capability::ClientSideToolSearch)
                    });
                let base = self
                    .tool_context_factory(hook_tx_opt.as_ref())
                    .build(crate::tool_context::ToolContextOverrides {
                        user_message_id: Some(user_msg_uuid.clone()),
                        progress_tx: Some(progress_tx_session.clone()),
                        current_model_id: Some(model_runtime.current_model_id().to_string()),
                        current_model_supports_tool_reference: current_supports_tool_reference,
                        current_model_supports_client_side_tool_search:
                            current_supports_client_side_tool_search,
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
            match model_runtime.attempt_probe_if_due(std::time::Instant::now()) {
                crate::model_runtime::ProbeDecision::Skip => {}
                crate::model_runtime::ProbeDecision::Probe => {
                    info!(
                        probe_target = model_runtime.current_model_id(),
                        "probing primary via half-open recovery",
                    );
                }
            }
            let was_probing = model_runtime.probe_in_flight();
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
                    &history.messages,
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
                None => model_runtime.current_client(),
            };
            tracing::debug!(
                turn,
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
            let mut rx = match active_client.query_stream(&params).await {
                Ok(rx) => {
                    // Success resets the capacity-error streak —
                    // isolated 529s must not accumulate across turns.
                    consecutive_capacity_errors = 0;
                    if let Some(app_state) = self.app_state.as_ref() {
                        crate::engine_helpers::clear_rate_limit_observation(
                            app_state,
                            active_client.provider(),
                        )
                        .await;
                    }
                    tracing::debug!(
                        turn,
                        turn_id = %turn_id,
                        provider = active_client.provider(),
                        model_id = active_client.model_id(),
                        "LLM stream opened"
                    );
                    // Probe succeeded at stream-open — clear
                    // recovery state and announce the switch-back.
                    if was_probing {
                        let recovered = model_runtime.current_model_id().to_string();
                        model_runtime.finalize_probe(
                            crate::model_runtime::ProbeOutcome::Success,
                            std::time::Instant::now(),
                        );
                        emit_model_fallback_notice(
                            &event_tx,
                            /*original*/ "",
                            &recovered,
                            &self.config.session_id,
                            crate::model_runtime::ModelFallbackReason::ProbeRecovery,
                        )
                        .await;
                    }
                    rx
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    // Probe failure: transparently revert to the
                    // fallback, then retry the turn. A probe is
                    // OPTIONAL — failing one must NOT surface as
                    // a user-visible error; the session behaves
                    // exactly as if no probe had been attempted.
                    if was_probing {
                        model_runtime.finalize_probe(
                            crate::model_runtime::ProbeOutcome::Failure,
                            std::time::Instant::now(),
                        );
                        warn!(
                            active = model_runtime.current_model_id(),
                            error = %err_msg,
                            "probe failed at stream-open; reverting to fallback and retrying",
                        );
                        // Don't tick the capacity streak — probe
                        // and streak are independent signals.
                        // `continue` reruns the turn from the top
                        // using the reverted fallback slot.
                        continue;
                    }
                    if matches!(
                        &e,
                        coco_inference::InferenceError::ContextWindowExceeded { .. }
                    ) || err_msg.contains("prompt_too_long")
                        || err_msg.contains("context_length")
                    {
                        last_continue_reason = Some(
                            self.handle_context_overflow(
                                &mut *history,
                                &event_tx,
                                &mut budget,
                                "stream_open",
                            )
                            .await,
                        );
                        continue;
                    }
                    let is_capacity = matches!(
                        &e,
                        coco_inference::InferenceError::Overloaded { .. }
                            | coco_inference::InferenceError::RateLimited { .. }
                    ) || is_capacity_error_message(&err_msg);
                    if is_capacity {
                        // Phase 7c: record per-provider rate-limit
                        // observation onto `ToolAppState.rate_limits`
                        // BEFORE the retry/fallback decision flow so
                        // post-turn forks (prompt-suggestion) see the
                        // throttle even on the first 429. Selectivity
                        // is the read-side filter — every entry in
                        // the map is keyed by the provider that
                        // observed the error.
                        if let Some(app_state) = self.app_state.as_ref() {
                            let retry_after_ms = match &e {
                                coco_inference::InferenceError::RateLimited {
                                    retry_after_ms,
                                    ..
                                }
                                | coco_inference::InferenceError::Overloaded {
                                    retry_after_ms,
                                    ..
                                } => *retry_after_ms,
                                _ => None,
                            };
                            crate::engine_helpers::record_rate_limit_observation(
                                app_state,
                                active_client.provider(),
                                active_client.fingerprint().api,
                                retry_after_ms,
                            )
                            .await;
                        }
                        consecutive_capacity_errors += 1;
                        if consecutive_capacity_errors < MAX_CONSECUTIVE_CAPACITY_ERRORS {
                            // Below threshold: log and retry the
                            // turn on the same slot. The streak
                            // counter accumulates across
                            // iterations until `advance()` fires.
                            warn!(
                                consecutive = consecutive_capacity_errors,
                                threshold = MAX_CONSECUTIVE_CAPACITY_ERRORS,
                                active = model_runtime.current_model_id(),
                                "capacity error below threshold; retrying on same slot",
                            );
                            continue;
                        }
                        if model_runtime.has_fallback() {
                            let original = model_runtime.current_model_id().to_string();
                            match model_runtime.advance() {
                                crate::model_runtime::AdvanceOutcome::Switched(new_model) => {
                                    warn!(
                                        original,
                                        fallback = new_model,
                                        consecutive = consecutive_capacity_errors,
                                        "advanced to next fallback slot after \
                                         capacity streak",
                                    );
                                    consecutive_capacity_errors = 0;
                                    emit_model_fallback_notice(
                                        &event_tx,
                                        &original,
                                        &new_model,
                                        &self.config.session_id,
                                        crate::model_runtime::ModelFallbackReason::CapacityDegrade {
                                            consecutive_errors: MAX_CONSECUTIVE_CAPACITY_ERRORS,
                                        },
                                    )
                                    .await;
                                    continue;
                                }
                                crate::model_runtime::AdvanceOutcome::Exhausted => {
                                    warn!(
                                        active = original,
                                        "fallback chain exhausted on stream-open error",
                                    );
                                    emit_model_fallback_notice(
                                        &event_tx,
                                        &original,
                                        /*new_model*/ "",
                                        &self.config.session_id,
                                        crate::model_runtime::ModelFallbackReason::ChainExhausted,
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                    return Err(Box::new(coco_error::PlainError::new(
                        format!("LLM stream open failed: {e}"),
                        coco_error::StatusCode::ProviderError,
                    )));
                }
            };

            // Accumulate stream state. `tool_order` preserves the order tool
            // calls first appeared (by `ToolInputStart`) so the downstream
            // exec path keeps the same ordering contract as the blocking path.
            //
            // `response_text` and `reasoning_text` are presentation-only
            // accumulators driven from `StreamEvent::{TextDelta, ReasoningDelta}`:
            //
            // - `response_text` feeds the Stop hook's `last_assistant_message`
            //   input (engine.rs:1691), a log field (`:1739`), and
            //   `QueryResult.response_text` (`:1765`).
            // - `reasoning_text` feeds a log field (`:1347`).
            //
            // The history-bearing assistant content is reconstructed from
            // `event.snapshot` at `StreamEvent::Finish` — this is the path
            // that preserves per-part `provider_metadata` (Gemini
            // `thoughtSignature`, Anthropic `signature`, OpenAI
            // `encrypted_content`). See `docs/coco-rs/streaming-metadata-roundtrip-plan.md`.
            let mut response_text = String::new();
            let mut reasoning_text = String::new();
            let mut tool_order: Vec<String> = Vec::new();
            let mut tool_buffers: std::collections::HashMap<String, StreamingToolCallBuffer> =
                std::collections::HashMap::new();
            let mut stream_usage: Option<TokenUsage> = None;
            // Typed stop reason — set once by the provider-adapter
            // seam (see `coco_inference::StopReason` = extended
            // `UnifiedFinishReason`). The engine matches on the
            // typed enum directly; no wire-string parsing.
            let mut stream_stop_reason: Option<coco_messages::StopReason> = None;
            let mut stream_error: Option<String> = None;
            // Captured at `StreamEvent::Finish`; consumed to rebuild
            // `Vec<AssistantContentPart>` for history. `None` until Finish
            // arrives; cancellation/error paths skip reconstruction
            // entirely so the `None` case is unreachable on the
            // reconstruction path.
            let mut turn_snapshot: Option<std::sync::Arc<coco_inference::AssistantTurnSnapshot>> =
                None;

            loop {
                let event = tokio::select! {
                    _ = self.cancel.cancelled() => {
                        // Cancellation mid-stream: drop the stream
                        // and fall through to the top-of-loop
                        // `is_cancelled()` check which returns a
                        // proper `Ok(QueryResult { cancelled: true })`.
                        // With streaming_tool_execution enabled, the
                        // StreamingHandle's JoinSet aborts any
                        // inflight safe tools when dropped
                        // (transitively via streaming_handle going
                        // out of scope as this function unwinds).
                        drop(rx);
                        break;
                    }
                    ev = rx.recv() => ev,
                };
                let Some(event) = event else {
                    // Channel closed without Finish/Error — treat as a premature
                    // end. Keep whatever content we accumulated; callers fall
                    // through to the empty-tool_calls exit below.
                    break;
                };

                match event {
                    StreamEvent::TextDelta { text } => {
                        response_text.push_str(&text);
                        let _ = emit_stream(
                            &event_tx,
                            crate::AgentStreamEvent::TextDelta {
                                turn_id: turn_id.clone(),
                                delta: text,
                            },
                        )
                        .await;
                    }
                    StreamEvent::ReasoningDelta { text } => {
                        reasoning_text.push_str(&text);
                        let _ = emit_stream(
                            &event_tx,
                            crate::AgentStreamEvent::ThinkingDelta {
                                turn_id: turn_id.clone(),
                                delta: text,
                            },
                        )
                        .await;
                    }
                    StreamEvent::ReasoningEnd { .. } => {
                        // No-op: the snapshot accumulator at the
                        // coco-inference layer captures the signature
                        // per-segment and surfaces it on
                        // `StreamEvent::Finish.snapshot` for full
                        // multi-reasoning fidelity (see plan v6).
                    }
                    StreamEvent::ToolCallStart { id, tool_name } => {
                        if !tool_buffers.contains_key(&id) {
                            tool_order.push(id.clone());
                        }
                        tool_buffers.insert(
                            id.clone(),
                            StreamingToolCallBuffer {
                                tool_name,
                                input_json: String::new(),
                                complete: false,
                            },
                        );
                    }
                    StreamEvent::ToolCallDelta { id, delta } => {
                        if let Some(buf) = tool_buffers.get_mut(&id) {
                            buf.input_json.push_str(&delta);
                        }
                    }
                    StreamEvent::ToolCallEnd { id } => {
                        if let Some(buf) = tool_buffers.get_mut(&id) {
                            buf.complete = true;
                        }
                        // Streaming mode: parse the freshly-completed
                        // input, run full per-tool preparation
                        // (validate → pre-hook → permission →
                        // re-validate), and feed the resulting plan
                        // to the StreamingHandle. Safe tools start
                        // executing immediately via tokio::spawn;
                        // unsafe tools queue for commit_flush.
                        //
                        // ── I1 invariant fix ──
                        // The preparer's early-error paths push
                        // synthetic tool_result rows to history
                        // directly (non-streaming behaviour). In
                        // streaming mode, the assistant message
                        // hasn't been committed yet — it lands at the
                        // `Finish` arm below. A naive inline push
                        // produces history of:
                        //   N:   user/tool_result (synthetic error)
                        //   N+1: assistant/tool_use(s)
                        // ...which violates Anthropic's strict
                        // tool_use/tool_result adjacency.
                        //
                        // Capture the pre-call length, then drain any
                        // pushes after preparation. Successful prep
                        // makes no pushes (the plan is fed to the
                        // handle); failed prep pushes the synthetic
                        // error, which we re-wrap as an
                        // `EarlyOutcome` so `commit_flush` surfaces
                        // it AFTER the assistant message lands.
                        if let (Some(handle), Some(ctx_arc)) =
                            (streaming_handle.as_mut(), streaming_ctx.as_ref())
                            && let Some(buf) = tool_buffers.get(&id)
                            && buf.complete
                        {
                            // Strict parse → repair fallback (trailing
                            // commas, unquoted keys, unclosed strings /
                            // brackets). Models occasionally emit these;
                            // the repair pass turns "drop the call" into
                            // "run with the obvious intent".
                            let input: serde_json::Value = match coco_tool_runtime::parse_tool_input(
                                &buf.input_json,
                            ) {
                                Ok((v, outcome)) => {
                                    if let coco_tool_runtime::ParseOutcome::Repaired {
                                        repaired_with,
                                    } = outcome
                                    {
                                        tracing::info!(
                                            tool_call_id = %id,
                                            tool_name = %buf.tool_name,
                                            repaired_with = ?repaired_with,
                                            "streaming tool input JSON repaired before execution",
                                        );
                                    }
                                    v
                                }
                                Err(e) => {
                                    warn!(
                                        tool_call_id = %id,
                                        tool_name = %buf.tool_name,
                                        error = %e,
                                        "streaming tool input JSON parse failed; dropping call"
                                    );
                                    continue;
                                }
                            };
                            let input =
                                crate::tool_input_normalizer::normalize_observable_tool_input(
                                    &buf.tool_name,
                                    input,
                                    crate::tool_input_normalizer::ToolInputNormalizationContext {
                                        session_id: Some(&self.config.session_id),
                                        plans_dir: plans_dir.as_deref(),
                                        agent_id: ctx_arc
                                            .agent_id
                                            .as_ref()
                                            .map(coco_types::AgentId::as_str),
                                    },
                                );
                            let tcp = ToolCallPart {
                                tool_call_id: id.clone(),
                                tool_name: buf.tool_name.clone(),
                                input,
                                provider_executed: None,
                                provider_metadata: None,
                            };
                            let slice = std::slice::from_ref(&tcp);
                            let mut prep_args = crate::tool_call_preparer::PendingToolPreparation {
                                event_tx: &event_tx,
                                history: &mut *history,
                                ctx: ctx_arc.as_ref(),
                                tool_calls: slice,
                                tools: &self.tools,
                                hooks: self.hooks.as_ref(),
                                orchestration_ctx: self.orchestration_ctx(),
                                hook_tx_opt: hook_tx_opt.as_ref(),
                                permission_denials: &mut permission_denials,
                                state_tracker,
                                permission_bridge: self.permission_bridge.as_ref(),
                                session_id: &self.config.session_id,
                                cancel: &self.cancel,
                                auto_mode_state: self.auto_mode_state.as_ref(),
                                denial_tracker: self.denial_tracker.as_ref(),
                                client: &self.client,
                                auto_mode_rules: &self.auto_mode_rules,
                                completion_event_mode:
                                    crate::helpers::ToolCompletionEventMode::Defer,
                            };
                            let pre_prep_len = prep_args.history.messages.len();
                            let prep_result =
                                crate::tool_call_preparer::prepare_one_pending_tool_call(
                                    &mut prep_args,
                                    &tcp,
                                )
                                .await;
                            // Drain whatever the preparer pushed
                            // synchronously (synthetic error
                            // tool_result rows on the failure paths).
                            // `drain_pushed_since` rebuilds the UUID
                            // index so subsequent lookups stay valid.
                            let captured_errors =
                                history.messages.len().saturating_sub(pre_prep_len);
                            let captured: Vec<coco_messages::Message> = if captured_errors > 0 {
                                history.drain_pushed_since(pre_prep_len)
                            } else {
                                Vec::new()
                            };

                            match prep_result {
                                Some((pending, _ctx)) => {
                                    debug_assert!(
                                        captured.is_empty(),
                                        "preparation succeeded but pushed messages"
                                    );
                                    // Emit ToolUseStarted now that
                                    // the call has passed pre-hook +
                                    // permission and is about to be
                                    // spawned. Non-streaming path
                                    // emits this in
                                    // tool_call_runner.rs:145; we
                                    // mirror that here so SDK
                                    // consumers see the same event
                                    // sequence regardless of path.
                                    let _ = emit_stream(
                                        &event_tx,
                                        crate::AgentStreamEvent::ToolUseStarted {
                                            call_id: pending.tool_use_id.clone(),
                                            name: pending.tool.name().to_string(),
                                            batch_id: None,
                                        },
                                    )
                                    .await;
                                    let model_index = streaming_model_index;
                                    streaming_model_index += 1;
                                    handle.feed_plan(coco_tool_runtime::ToolCallPlan::Runnable(
                                        coco_tool_runtime::PreparedToolCall {
                                            tool_use_id: pending.tool_use_id,
                                            tool_id: pending.tool.id(),
                                            tool: pending.tool,
                                            parsed_input: pending.input,
                                            model_index,
                                        },
                                    ));
                                }
                                None if !captured.is_empty() => {
                                    // Preparation failed and pushed a
                                    // synthetic-error tool_result. If
                                    // cancellation raced the permission
                                    // wait, preserve a valid partial
                                    // assistant/tool_result pair now;
                                    // the normal Finish path will not
                                    // run commit_flush after the cancel
                                    // token is set.
                                    if self.cancel.is_cancelled() {
                                        let mut content_parts = Vec::new();
                                        if !response_text.is_empty() {
                                            content_parts.push(AssistantContentPart::Text(
                                                TextPart {
                                                    text: response_text.clone(),
                                                    provider_metadata: None,
                                                },
                                            ));
                                        }
                                        content_parts
                                            .push(AssistantContentPart::ToolCall(tcp.clone()));
                                        history.push(Message::Assistant(
                                            coco_messages::AssistantMessage {
                                                message: LlmMessage::Assistant {
                                                    content: content_parts
                                                        .into_iter()
                                                        .map(convert_to_assistant_content)
                                                        .collect(),
                                                    provider_options: None,
                                                },
                                                uuid: uuid::Uuid::new_v4(),
                                                model: model_runtime.current_model_id().to_string(),
                                                stop_reason: Some(
                                                    coco_messages::StopReason::ToolUse,
                                                ),
                                                usage: None,
                                                cost_usd: None,
                                                request_id: None,
                                                api_error: None,
                                            },
                                        ));
                                        for msg in captured {
                                            history.push(msg);
                                        }
                                    } else {
                                        // Re-wrap and feed as EarlyOutcome
                                        // so commit_flush surfaces it
                                        // after the assistant message
                                        // commits (I1 ordering fix).
                                        // The streaming preparer runs in
                                        // `ToolCompletionEventMode::Defer`,
                                        // so it did NOT emit
                                        // `ToolUseCompleted` inline —
                                        // commit_flush's `on_outcome`
                                        // callback is the sole emitter
                                        // for this id and must not be
                                        // suppressed by dedup.
                                        let tool_id_for_outcome: coco_types::ToolId =
                                            buf.tool_name.parse().unwrap_or_else(|_| {
                                                coco_types::ToolId::Custom(buf.tool_name.clone())
                                            });
                                        let model_index = streaming_model_index;
                                        streaming_model_index += 1;
                                        let outcome = crate::helpers::build_streaming_early_outcome(
                                            &id,
                                            tool_id_for_outcome,
                                            model_index,
                                            captured,
                                        );
                                        handle.feed_plan(
                                            coco_tool_runtime::ToolCallPlan::EarlyOutcome(outcome),
                                        );
                                    }
                                }
                                None => {
                                    // Rare: prep returned None with
                                    // no captured messages. Drop
                                    // silently — there is no
                                    // model-visible result to pair.
                                }
                            }
                        }
                    }
                    StreamEvent::Finish {
                        usage,
                        stop_reason,
                        raw_stop_reason,
                        snapshot,
                        ..
                    } => {
                        turn_snapshot = Some(snapshot);
                        tracing::debug!(
                            turn,
                            turn_id = %turn_id,
                            stop_reason = %stop_reason,
                            raw_stop_reason = ?raw_stop_reason,
                            tokens_in = usage.input_tokens,
                            tokens_out = usage.output_tokens,
                            cache_read = usage.cache_read_input_tokens(),
                            cache_creation = usage.cache_creation_input_tokens(),
                            text_chars = response_text.len(),
                            reasoning_chars = reasoning_text.len(),
                            tool_call_count = tool_order.len(),
                            "LLM stream finished"
                        );
                        stream_usage = Some(usage);
                        stream_stop_reason = Some(stop_reason);
                        // raw_stop_reason is diagnostic only — already
                        // captured in the debug log above. Drop it.
                        let _ = raw_stop_reason;
                        break;
                    }
                    StreamEvent::Error { message, .. } => {
                        warn!(
                            turn,
                            turn_id = %turn_id,
                            error = %message,
                            text_chars = response_text.len(),
                            tool_call_count = tool_order.len(),
                            "LLM stream errored"
                        );
                        stream_error = Some(message);
                        break;
                    }
                }
            }

            let api_elapsed_ms = api_start.elapsed().as_millis() as i64;
            api_time_ms += api_elapsed_ms;

            // Cancellation mid-stream: skip the rest of turn
            // processing and let the top-of-loop cancel check build
            // the proper `QueryResult { cancelled: true }`. Any
            // streaming handle in-flight is implicitly aborted when
            // this function unwinds (JoinSet drops cancel pending
            // tasks).
            //
            // Before dropping the handle we still drain its
            // `pending_early` queue — cancel races with
            // `prepare_one_pending_tool_call` (e.g. cancel during
            // permission wait) may have parked synthesized error
            // `tool_result` rows there for the I1-ordered commit. We
            // emit a synthetic assistant_msg containing the matching
            // tool_use blocks so the final history honors Anthropic's
            // tool_use ↔ tool_result adjacency. TS parity:
            // `query.ts:1015-1028` (`yieldMissingToolResultBlocks`
            // after abort).
            if self.cancel.is_cancelled() {
                let mut had_tool_use = false;
                if let Some(handle) = streaming_handle.take() {
                    let discarded = handle.discard().await;
                    let early: Vec<_> = discarded
                        .into_iter()
                        .filter(|o| !o.ordered_messages.is_empty())
                        .collect();
                    if !early.is_empty() {
                        had_tool_use = true;
                        let kept_ids: std::collections::HashSet<&String> =
                            early.iter().map(|o| &o.tool_use_id).collect();
                        let synth_parts: Vec<coco_inference::TurnPart> = tool_order
                            .iter()
                            .filter(|id| kept_ids.contains(*id))
                            .filter_map(|id| tool_buffers.get(id).map(|buf| (id, buf)))
                            .map(|(id, buf)| {
                                coco_inference::TurnPart::ToolCall(
                                    coco_inference::ToolCallSegment {
                                        id: id.clone(),
                                        tool_name: buf.tool_name.clone(),
                                        input_json: buf.input_json.clone(),
                                        provider_executed: None,
                                        dynamic: None,
                                        is_input_complete: buf.complete,
                                        is_complete: false,
                                        provider_metadata: None,
                                    },
                                )
                            })
                            .collect();
                        let synth_snapshot =
                            coco_inference::AssistantTurnSnapshot { parts: synth_parts };
                        let (content_parts, _) = assistant_content_from_snapshot(
                            &synth_snapshot,
                            crate::tool_input_normalizer::ToolInputNormalizationContext {
                                session_id: Some(&self.config.session_id),
                                plans_dir: plans_dir.as_deref(),
                                agent_id: self.config.agent_id.as_deref(),
                            },
                        );
                        if !content_parts.is_empty() {
                            let assistant_msg =
                                Message::Assistant(coco_messages::AssistantMessage {
                                    message: LlmMessage::Assistant {
                                        content: content_parts
                                            .into_iter()
                                            .map(convert_to_assistant_content)
                                            .collect(),
                                        provider_options: None,
                                    },
                                    uuid: uuid::Uuid::new_v4(),
                                    model: model_runtime.current_model_id().to_string(),
                                    stop_reason: None,
                                    usage: None,
                                    cost_usd: None,
                                    request_id: None,
                                    api_error: None,
                                });
                            history.push(assistant_msg);
                        }
                        for outcome in early {
                            for msg in outcome.ordered_messages {
                                history.push(msg);
                            }
                        }
                    }
                }
                // TS parity: `query.ts:1046-1049` — append the synthetic
                // `[Request interrupted by user]` user message so the
                // model sees on the next turn that the prior turn was
                // cut short. `for_tool_use = true` when in-flight tool
                // calls were synthesized into history above, matching
                // TS's `toolUse` flag on `createUserInterruptionMessage`.
                history.push(coco_messages::create_user_interruption_message(
                    had_tool_use,
                ));
                continue;
            }

            if let Some(err_msg) = stream_error {
                // Probe failure mid-stream: transparently revert
                // and retry — same rule as stream-open. Probes
                // are optional; their failures must never be
                // user-visible.
                if model_runtime.probe_in_flight() {
                    model_runtime.finalize_probe(
                        crate::model_runtime::ProbeOutcome::Failure,
                        std::time::Instant::now(),
                    );
                    warn!(
                        active = model_runtime.current_model_id(),
                        error = %err_msg,
                        "probe failed mid-stream; reverting to fallback and retrying",
                    );
                    continue;
                }
                if err_msg.contains("prompt_too_long") || err_msg.contains("context_length") {
                    last_continue_reason = Some(
                        self.handle_context_overflow(
                            &mut *history,
                            &event_tx,
                            &mut budget,
                            "mid_stream",
                        )
                        .await,
                    );
                    continue;
                }
                if is_capacity_error_message(&err_msg) {
                    consecutive_capacity_errors += 1;
                    if consecutive_capacity_errors < MAX_CONSECUTIVE_CAPACITY_ERRORS {
                        warn!(
                            consecutive = consecutive_capacity_errors,
                            threshold = MAX_CONSECUTIVE_CAPACITY_ERRORS,
                            active = model_runtime.current_model_id(),
                            "capacity error mid-stream below threshold; retrying on same slot",
                        );
                        continue;
                    }
                    if model_runtime.has_fallback() {
                        let original = model_runtime.current_model_id().to_string();
                        match model_runtime.advance() {
                            crate::model_runtime::AdvanceOutcome::Switched(new_model) => {
                                warn!(
                                    original,
                                    fallback = new_model,
                                    consecutive = consecutive_capacity_errors,
                                    "advanced to next fallback slot after \
                                     capacity streak (mid-stream)",
                                );
                                consecutive_capacity_errors = 0;
                                emit_model_fallback_notice(
                                    &event_tx,
                                    &original,
                                    &new_model,
                                    &self.config.session_id,
                                    crate::model_runtime::ModelFallbackReason::CapacityDegrade {
                                        consecutive_errors: MAX_CONSECUTIVE_CAPACITY_ERRORS,
                                    },
                                )
                                .await;
                                continue;
                            }
                            crate::model_runtime::AdvanceOutcome::Exhausted => {
                                warn!(active = original, "fallback chain exhausted mid-stream",);
                                emit_model_fallback_notice(
                                    &event_tx,
                                    &original,
                                    /*new_model*/ "",
                                    &self.config.session_id,
                                    crate::model_runtime::ModelFallbackReason::ChainExhausted,
                                )
                                .await;
                            }
                        }
                    }
                }
                // Surface streaming-discard outcomes for telemetry
                // before bailing out. The assistant message hasn't
                // committed yet on this path, so committing
                // tool_result rows to history would violate I1;
                // instead we emit `ToolUseCompleted{is_error}` per
                // discarded plan and warn-log a summary, then drop
                // them. Without this drain `JoinSet::drop` aborts
                // inflight safe tools silently — operators lose
                // visibility into how much real work the stream
                // error invalidated.
                if let Some(handle) = streaming_handle.take() {
                    let discarded = handle.discard().await;
                    if !discarded.is_empty() {
                        let count = discarded.len() as i64;
                        for outcome in discarded {
                            let tool_use_id = outcome.tool_use_id.clone();
                            let tool_id = outcome.tool_id.clone();
                            let text = extract_streaming_result_text(&outcome.ordered_messages);
                            let _ = emit_stream(
                                &event_tx,
                                crate::AgentStreamEvent::ToolUseCompleted {
                                    call_id: tool_use_id,
                                    name: tool_id.to_string(),
                                    output: text,
                                    is_error: true,
                                },
                            )
                            .await;
                        }
                        warn!(
                            turn,
                            turn_id = %turn_id,
                            discarded_count = count,
                            error = %err_msg,
                            "discarded streaming tool outcomes after mid-stream error",
                        );
                    }
                }
                return Err(Box::new(coco_error::PlainError::new(
                    format!("LLM stream failed: {err_msg}"),
                    coco_error::StatusCode::ProviderError,
                )));
            }
            // Stream closed without error — reset the capacity streak
            // so an isolated failure followed by a successful turn
            // doesn't carry forward.
            consecutive_capacity_errors = 0;

            let usage = stream_usage.unwrap_or_default();
            total_usage += usage;
            budget.record_usage(&usage);
            // Record usage against the currently-active model id
            // (post-fallback value if a switch has happened).
            let model_id = model_runtime.current_model_id().to_string();
            cost_tracker.record(&model_id, usage, /*cost_usd*/ 0.0, api_elapsed_ms);

            // Reconstruct assistant content from the per-turn snapshot
            // accumulated inside `coco-inference::process_stream_with_config`.
            // Each `TurnPart` carries its own `provider_metadata`, so
            // Gemini `thoughtSignature` / Anthropic `signature` /
            // OpenAI `encrypted_content` survive intact and round-trip
            // back to the model on the next turn.
            //
            // Cancellation / mid-stream error paths skip this block via
            // `continue;` upstream (engine.rs:~1379), so `turn_snapshot`
            // is always `Some` here. Defensive fallback to empty
            // snapshot keeps the unwrap from panicking if that
            // invariant ever weakens.
            let snapshot = turn_snapshot.take().unwrap_or_default();
            let (content_parts, tool_calls) = assistant_content_from_snapshot(
                &snapshot,
                crate::tool_input_normalizer::ToolInputNormalizationContext {
                    session_id: Some(&self.config.session_id),
                    plans_dir: plans_dir.as_deref(),
                    agent_id: self.config.agent_id.as_deref(),
                },
            );

            // Typed StopReason flows straight from the stream — no
            // wire-string parsing. `stream_stop_reason` carries the
            // canonical UnifiedFinishReason set at the
            // vercel-ai-provider seam.
            let parsed_stop_reason = stream_stop_reason;
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
                request_id: None,
                api_error: None,
            });

            // Content-filter / refusal: no recovery — policy decision,
            // retry won't change it. Push the partial real response
            // (may contain a partial refusal explanation from the
            // model), then synthesize an `api_error`-tagged assistant
            // message carrying the user-facing explanation, and fall
            // through to the natural `tool_calls.is_empty()` end-of-turn
            // exit. TS parity: `services/api/claude.ts:2258-2264`
            // (`getErrorMessageIfRefusal`) yields the synthetic message
            // at the stream layer; `query.ts:1262-1265` then short-
            // circuits stop-hooks when the last message is
            // `isApiErrorMessage`. coco-rs synthesizes here at the
            // engine layer because `coco-inference` is provider-agnostic
            // and can't construct `coco_messages::Message`. Multi-LLM:
            // every `vercel-ai-<provider>` adapter maps refusal /
            // safety / recitation / content_filter → typed
            // `StopReason::ContentFilter` at the seam, so this single
            // branch covers Anthropic / OpenAI / Google.
            if tool_calls.is_empty()
                && parsed_stop_reason == Some(coco_messages::StopReason::ContentFilter)
            {
                warn!(
                    turn,
                    turn_id = %turn_id,
                    "content-filter / refusal — emitting api_error message and ending turn"
                );
                history.push(assistant_msg);
                history.push(crate::helpers::build_abnormal_stop_api_error_message(
                    coco_messages::StopReason::ContentFilter,
                    max_tokens_override.or(self.config.max_tokens),
                ));
                // Skip the MaxTokens block below; fall through to the
                // text-only end-of-turn path that emits TurnCompleted.
                // No `continue` — we want the loop to exit naturally.
            }
            // Context-window-exceeded: input + output > model context window.
            // Anthropic-only finish reason (extended-context beta); for other
            // providers the same condition arrives as an HTTP 400 handled at
            // the stream-open / mid-stream sites above. Always route to
            // [`Self::handle_context_overflow`] — escalating
            // `max_output_tokens` cannot help when the *input* already
            // exceeds the window. Push the synthetic api_error first so the
            // transcript records the precipitating event before compaction
            // rewrites history; push the partial `assistant_msg` so the
            // truncated content remains visible to the model post-compact.
            else if tool_calls.is_empty()
                && parsed_stop_reason == Some(coco_messages::StopReason::ContextWindowExceeded)
            {
                let effective_max = max_tokens_override.or(self.config.max_tokens);
                history.push(assistant_msg);
                history.push(crate::helpers::build_abnormal_stop_api_error_message(
                    coco_messages::StopReason::ContextWindowExceeded,
                    effective_max,
                ));
                last_continue_reason = Some(
                    self.handle_context_overflow(
                        &mut *history,
                        &event_tx,
                        &mut budget,
                        "finish_reason",
                    )
                    .await,
                );
                continue;
            }
            // Max-output-tokens recovery: the model hit the output budget
            // with no tool calls. Phase 1: escalate `max_output_tokens` to
            // 64k and retry without persisting the truncated response (TS:
            // query.ts:1199-1221). Phase 2: if already escalated, keep the
            // partial response and inject a "resume" meta user message (TS:
            // query.ts:1223-1249), up to `MAX_OUTPUT_TOKENS_RECOVERY_LIMIT`
            // times.
            //
            // **TS parity for the synthetic api_error message:** TS yields
            // `createAssistantAPIErrorMessage` AT THE STREAM LAYER for every
            // `max_tokens` event (`services/api/claude.ts:2266-2292`), even when
            // the engine will escalate / recover. coco-rs mirrors this — each of
            // the three sub-branches (escalate / recover / fall-through) pushes
            // the synthetic message so transcripts and the UI carry the explicit
            // truncation marker, not just a silently-rewritten "[No message
            // content]" stub.
            else if tool_calls.is_empty()
                && parsed_stop_reason == Some(coco_messages::StopReason::MaxTokens)
            {
                // Escalation only helps when the user's configured limit is
                // BELOW the escalation target. If they're already >= 64k (or
                // we've already escalated this session), skip straight to
                // recovery. TS: `query.ts:1201-1202` guards on env override.
                let user_already_at_escalated = self
                    .config
                    .max_tokens
                    .is_some_and(|v| v >= ESCALATED_MAX_TOKENS);
                let effective_max = max_tokens_override.or(self.config.max_tokens);
                if max_tokens_override.is_none() && !user_already_at_escalated {
                    warn!(
                        escalated_to = ESCALATED_MAX_TOKENS,
                        "max_tokens hit, escalating"
                    );
                    // TS parity: yield the synthetic api_error message even
                    // though we're about to escalate — the user sees the
                    // signal once, before the retry overwrites the
                    // immediate UI state.
                    history.push(crate::helpers::build_abnormal_stop_api_error_message(
                        coco_messages::StopReason::MaxTokens,
                        effective_max,
                    ));
                    max_tokens_override = Some(ESCALATED_MAX_TOKENS);
                    last_continue_reason = Some(ContinueReason::MaxOutputTokensEscalate);
                    continue;
                } else if max_tokens_recovery_count < MAX_OUTPUT_TOKENS_RECOVERY_LIMIT {
                    max_tokens_recovery_count += 1;
                    warn!(
                        attempt = max_tokens_recovery_count,
                        "max_tokens hit after escalation, injecting resume nudge"
                    );
                    history.push(assistant_msg);
                    history.push(crate::helpers::build_abnormal_stop_api_error_message(
                        coco_messages::StopReason::MaxTokens,
                        effective_max,
                    ));
                    history.push(coco_messages::create_meta_message(
                        "Output token limit hit. Resume directly — no apology, no recap of \
                         what you were doing. Pick up mid-thought if that is where the cut \
                         happened. Break remaining work into smaller pieces.",
                    ));
                    // Reset override so next call uses the provider default again;
                    // TS does the same (query.ts:1241 `maxOutputTokensOverride: undefined`).
                    max_tokens_override = None;
                    last_continue_reason = Some(ContinueReason::MaxOutputTokensRecovery {
                        attempt: max_tokens_recovery_count,
                    });
                    continue;
                }
                // Recovery exhausted — push real + synthetic api_error and
                // fall through to terminate the session normally.
                history.push(assistant_msg);
                history.push(crate::helpers::build_abnormal_stop_api_error_message(
                    coco_messages::StopReason::MaxTokens,
                    effective_max,
                ));
            } else {
                history.push(assistant_msg);
            }

            // Backward-compat: the ContentFilter branch above already pushed
            // both `assistant_msg` and the synthetic message; the MaxTokens
            // exhaust branch pushed both too; the MaxTokens escalate/recover
            // branches pushed assistant_msg in some sub-cases. Normal-path
            // (no abnormal stop_reason matched) takes the `else` arm above
            // and pushes here. NB: do **not** push again here.
            //
            // (Earlier the line below was the only `history.push(assistant_msg)`
            // call site; the refactor moved it into the branches above so the
            // synthetic api_error message lands adjacent to its real
            // counterpart.)

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
            let mut streaming_completed_events: Vec<(String, String, String, bool)> = Vec::new();
            if let Some(handle) = streaming_handle.take()
                && streaming_executed
            {
                let history_ref = &mut *history;
                let prevent_slot = &mut streaming_control_prevent;
                let events_ref = &mut streaming_completed_events;
                handle
                    .commit_flush(0, |outcome| {
                        let call_id = outcome.tool_use_id().to_string();
                        let tool_name_str = outcome.tool_id().to_string();
                        let is_error = outcome.error_kind().is_some();
                        let output_text = extract_streaming_result_text(outcome.ordered_messages());
                        events_ref.push((call_id, tool_name_str, output_text, is_error));
                        if let Some(reason) = outcome.prevent_continuation()
                            && prevent_slot.is_none()
                        {
                            *prevent_slot = Some(reason.to_string());
                        }
                        let parts = outcome.into_parts();
                        for msg in parts.ordered_messages {
                            history_ref.push(msg);
                        }
                    })
                    .await;
            }
            for (call_id, tool_name, output, is_error) in streaming_completed_events {
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
                // TS `handleStopHooks` saves cache-safe params and
                // starts promptSuggestion before executing Stop hooks.
                // Keep transcript flush in the same helper so any
                // assistant text is resumable even if a Stop hook blocks
                // and the process exits before the retry turn.
                self.flush_successful_turn_state(&mut *history).await;
                self.maybe_spawn_prompt_suggestion_after_stop(&event_tx)
                    .await;

                // Stop hooks: let external hooks block session completion and
                // inject feedback into the conversation. If any Stop hook
                // blocks, the loop continues with the feedback visible to the
                // model. TS: `query.ts` `handleStopHooks()` around line 1050.
                if let Some(hooks) = &self.hooks {
                    let hook_ctx = self.orchestration_ctx();
                    let last_assistant_message = if response_text.is_empty() {
                        None
                    } else {
                        Some(response_text.as_str())
                    };
                    match orchestration::execute_stop(
                        hooks,
                        &hook_ctx,
                        stop_hook_active,
                        last_assistant_message,
                        hook_tx_opt.as_ref(),
                    )
                    .await
                    {
                        Ok(agg) if agg.prevent_continuation => {
                            info!("Stop hook prevented continuation");
                            self.flush_successful_turn_state(&mut *history).await;
                            self.emit_turn_completed(
                                &event_tx,
                                turn_id,
                                usage,
                                history.messages.len(),
                            )
                            .await;
                            return Ok(make_query_result(
                                response_text,
                                turn,
                                total_usage,
                                cost_tracker,
                                /*cancelled*/ false,
                                /*budget_exhausted*/ false,
                                last_continue_reason,
                                start_time,
                                api_time_ms,
                                Some("stop_hook_prevented".into()),
                                permission_denials,
                                history.messages.clone(),
                            ));
                        }
                        Ok(agg) if agg.is_blocked() => {
                            if let Some(err) = &agg.blocking_error {
                                let feedback = orchestration::format_stop_hook_message(err);
                                warn!(%feedback, "Stop hook blocked session completion");
                                history.push(coco_messages::create_meta_message(&feedback));
                                self.flush_successful_turn_state(&mut *history).await;
                                last_continue_reason = Some(ContinueReason::StopHookBlocking);
                                // Mark the recursion so the next Stop
                                // firing carries `stop_hook_active: true`
                                // (TS parity).
                                stop_hook_active = true;
                                continue;
                            }
                        }
                        Ok(_) => {}
                        Err(e) => warn!(error = %e, "Stop hook execution failed"),
                    }
                }

                if self.config.enable_token_budget_continuation
                    && should_continue_for_budget(&budget)
                {
                    let pct = budget_pct_used(&budget);
                    let nudge = format!(
                        "Token budget continuation: you've used {pct}% of the turn budget. \
                         Keep going — don't summarize or recap, just continue the work."
                    );
                    history.push(coco_messages::create_meta_message(&nudge));
                    budget.record_continuation();
                    last_continue_reason = Some(ContinueReason::TokenBudgetContinuation);
                    info!(turn, pct, "token budget continuation");
                    continue;
                }
                info!(
                    turn,
                    response_chars = response_text.len(),
                    tokens_in = usage.input_tokens,
                    tokens_out = usage.output_tokens,
                    "no tool calls, conversation complete"
                );
                self.emit_turn_completed(&event_tx, turn_id, usage, history.messages.len())
                    .await;
                return Ok(make_query_result(
                    response_text,
                    turn,
                    total_usage,
                    cost_tracker,
                    /*cancelled*/ false,
                    /*budget_exhausted*/ false,
                    last_continue_reason,
                    start_time,
                    api_time_ms,
                    Some("end_turn".into()),
                    permission_denials,
                    history.messages.clone(),
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
                self.finalize_turn_post_tools(&mut *history, &event_tx, turn_id, usage)
                    .await;
                if let Some(stop_reason) = streaming_control_prevent {
                    return Ok(make_query_result(
                        response_text,
                        turn,
                        total_usage,
                        cost_tracker,
                        /*cancelled*/ false,
                        /*budget_exhausted*/ false,
                        last_continue_reason,
                        start_time,
                        api_time_ms,
                        Some(stop_reason),
                        permission_denials,
                        history.messages.clone(),
                    ));
                }
                last_continue_reason = Some(ContinueReason::NextTurn);
                continue;
            }

            // Execute tool calls via StreamingToolExecutor (batch partitioning).
            // User-message id flows through the factory so the file-history
            // snapshot keys on the turn's triggering message, not a later
            // tool result. The factory installs a `QueryHookHandle` into
            // `ToolUseContext` when hooks are configured so tool callbacks
            // that need PreToolUse/PostToolUse use the same pipeline as the
            // runner.
            let ctx_supports_tool_reference = model_runtime
                .current_client()
                .model_info()
                .is_some_and(|info| {
                    info.has_capability(coco_types::Capability::ServerSideToolReference)
                });
            let ctx_supports_client_side_tool_search = model_runtime
                .current_client()
                .model_info()
                .is_some_and(|info| {
                    info.has_capability(coco_types::Capability::ClientSideToolSearch)
                });
            let ctx = self
                .tool_context_factory(hook_tx_opt.as_ref())
                .build(crate::tool_context::ToolContextOverrides {
                    user_message_id: Some(user_msg_uuid.clone()),
                    progress_tx: Some(progress_tx_session.clone()),
                    current_model_id: Some(model_runtime.current_model_id().to_string()),
                    current_model_supports_tool_reference: ctx_supports_tool_reference,
                    current_model_supports_client_side_tool_search:
                        ctx_supports_client_side_tool_search,
                })
                .await;

            let tool_run_outcome = ToolCallRunner {
                event_tx: &event_tx,
                history: &mut *history,
                ctx: &ctx,
                tool_calls: &tool_calls,
                turn,
                tools: &self.tools,
                hooks: self.hooks.as_ref(),
                orchestration_ctx: self.orchestration_ctx(),
                hook_tx_opt: hook_tx_opt.as_ref(),
                permission_denials: &mut permission_denials,
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
            self.finalize_turn_post_tools(&mut *history, &event_tx, turn_id, usage)
                .await;
            if !tool_run_outcome.continue_after_tools {
                return Ok(make_query_result(
                    response_text,
                    turn,
                    total_usage,
                    cost_tracker,
                    /*cancelled*/ false,
                    /*budget_exhausted*/ false,
                    last_continue_reason,
                    start_time,
                    api_time_ms,
                    tool_run_outcome.stop_reason_override,
                    permission_denials,
                    history.messages.clone(),
                ));
            }
            last_continue_reason = Some(ContinueReason::NextTurn);
            let _ = tool_calls; // has_tool_calls retained for future metrics
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
fn assistant_content_from_snapshot(
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
                let input: serde_json::Value =
                    match coco_tool_runtime::parse_tool_input(&tc.input_json) {
                        Ok((v, outcome)) => {
                            if let coco_tool_runtime::ParseOutcome::Repaired { repaired_with } =
                                outcome
                            {
                                tracing::info!(
                                    tool_call_id = %tc.id,
                                    tool_name = %tc.tool_name,
                                    repaired_with = ?repaired_with,
                                    "tool input JSON repaired before execution",
                                );
                            }
                            v
                        }
                        Err(e) => {
                            warn!(
                                tool_call_id = %tc.id,
                                tool_name = %tc.tool_name,
                                error = %e,
                                raw_input = %tc.input_json,
                                "tool input JSON parse failed"
                            );
                            continue;
                        }
                    };
                let input = crate::tool_input_normalizer::normalize_observable_tool_input(
                    &tc.tool_name,
                    input,
                    normalizer_ctx,
                );
                let tcp = ToolCallPart {
                    tool_call_id: tc.id.clone(),
                    tool_name: tc.tool_name.clone(),
                    input,
                    provider_executed: tc.provider_executed,
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

/// Returns true if the message history's tail is already a
/// `[Request interrupted by user]` user message (either variant). Used
/// by the cancel exit to dedupe the marker when the mid-stream branch
/// has already appended one for this turn.
fn last_message_is_interrupt_marker(history: &MessageHistory) -> bool {
    let Some(Message::User(user)) = history.messages.last() else {
        return false;
    };
    let coco_messages::LlmMessage::User { content, .. } = &user.message else {
        return false;
    };
    let [coco_inference::UserContentPart::Text(text_part)] = content.as_slice() else {
        return false;
    };
    text_part.text == coco_messages::INTERRUPT_MESSAGE
        || text_part.text == coco_messages::INTERRUPT_MESSAGE_FOR_TOOL_USE
}

/// Pure constructor for [`QueryResult`], factored out of `run_session_loop`.
/// All inputs flow through parameters — there is no captured state — so the
/// loop's five exit branches build the same shape with one call.
#[allow(clippy::too_many_arguments)]
fn make_query_result(
    response_text: String,
    turns: i32,
    total_usage: TokenUsage,
    cost_tracker: CostTracker,
    cancelled: bool,
    budget_exhausted: bool,
    last_continue_reason: Option<ContinueReason>,
    start_time: std::time::Instant,
    api_time_ms: i64,
    stop_reason: Option<String>,
    permission_denials: Vec<coco_types::PermissionDenialInfo>,
    final_messages: Vec<Message>,
) -> QueryResult {
    QueryResult {
        response_text,
        turns,
        total_usage,
        cost_tracker,
        cancelled,
        budget_exhausted,
        last_continue_reason,
        duration_ms: start_time.elapsed().as_millis() as i64,
        duration_api_ms: api_time_ms,
        stop_reason,
        permission_denials,
        final_messages,
    }
}

#[cfg(test)]
#[path = "engine.test.rs"]
mod tests;

#[cfg(test)]
#[path = "engine_live_rules_scoping.test.rs"]
mod engine_live_rules_scoping_tests;
