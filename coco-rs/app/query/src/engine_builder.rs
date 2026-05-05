//! Builder + accessor impl for [`QueryEngine`].
//!
//! This file owns:
//! - the `new` constructor + every `with_*` builder,
//! - small one-liner accessors (`command_queue`, `inbox`),
//! - the recently-mentioned-paths LRU + post-compact-skill snapshot setters,
//! - the `attachment_emitter` handle and the matching drain helper,
//! - cache-break attribution + collapse-active gating.
//!
//! Extracted from `engine.rs` to keep that file focused on the multi-turn
//! orchestration. Layout follows `coco-rs` conventions: a sibling impl block
//! over the same struct, no extra module type. Field access works because
//! `QueryEngine` fields are `pub(crate)`.

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use coco_context::FileHistoryState;
use coco_hooks::HookRegistry;
use coco_inference::ApiClient;
use coco_messages::MessageHistory;
use coco_tool_runtime::ToolRegistry;
use coco_types::ToolAppState;

use crate::command_queue::CommandQueue;
use crate::command_queue::Inbox;
use crate::config::QueryEngineConfig;
use crate::config::SessionBootstrap;
use crate::engine::QueryEngine;

impl QueryEngine {
    /// Cap on the number of @mentioned paths kept for post-compact priority.
    /// Bounds memory and matches the observation that compact restoration
    /// only ever lists 5 files anyway — older mentions stop mattering.
    pub(crate) const MENTION_PRIORITY_CAPACITY: usize = 32;

    pub fn new(
        config: QueryEngineConfig,
        client: Arc<ApiClient>,
        tools: Arc<ToolRegistry>,
        cancel: CancellationToken,
        hooks: Option<Arc<HookRegistry>>,
    ) -> Self {
        let (attachment_tx, attachment_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            config,
            client,
            fallback_clients: Vec::new(),
            recovery_policy: None,
            tools,
            cancel,
            hooks,
            command_queue: CommandQueue::new(),
            inbox: Inbox::new(),
            file_read_state: None,
            file_history: None,
            config_home: None,
            session_bootstrap: None,
            permission_bridge: None,
            auto_mode_state: None,
            denial_tracker: None,
            auto_mode_rules: coco_permissions::AutoModeRules::default(),
            app_state: None,
            mailbox: None,
            mcp_handle: None,
            agent_handle: None,
            skill_handle: None,
            agent_catalog: None,
            last_cache_safe_params: Arc::new(tokio::sync::RwLock::new(None)),
            fork_dispatcher: None,
            task_handle: None,
            tool_schema_validator: coco_tool_runtime::ToolSchemaValidator::new(),
            tool_result_replacement_state: Arc::new(tokio::sync::RwLock::new(
                coco_tool_runtime::tool_result_storage::ContentReplacementState::new(i64::MAX),
            )),
            task_list: None,
            todo_list: None,
            reminder_sources: coco_system_reminder::ReminderSources::default(),
            attachment_tx,
            attachment_rx: Arc::new(tokio::sync::Mutex::new(attachment_rx)),
            compaction_observers: Arc::new(coco_compact::CompactionObserverRegistry::new()),
            last_assistant_ms: Arc::new(std::sync::atomic::AtomicI64::new(0)),
            last_summarized_message_id: Arc::new(std::sync::Mutex::new(None)),
            session_memory_text: Arc::new(tokio::sync::RwLock::new(String::new())),
            session_memory_service: None,
            memory_runtime: None,
            reactive_state: Arc::new(tokio::sync::Mutex::new(
                coco_compact::ReactiveCompactState::new(),
            )),
            running_tasks: None,
            last_compact_state: Arc::new(std::sync::Mutex::new(None)),
            turn_counter: Arc::new(std::sync::atomic::AtomicI64::new(0)),
            post_compact_skills: Arc::new(std::sync::RwLock::new(Vec::new())),
            staged_ledger: None,
            staged_session_id: uuid::Uuid::new_v4(),
            recently_mentioned_paths: Arc::new(tokio::sync::RwLock::new(
                std::collections::VecDeque::new(),
            )),
            pending_reactive_context_management: Arc::new(tokio::sync::Mutex::new(None)),
            transcript_store: None,
            transcript_session_id: None,
            pending_nested_memory: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            loaded_nested_memory_paths: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashSet::new(),
            )),
        }
    }

    /// Install a transcript store for marble-origami persistence and
    /// future on-disk session writes. `session_id` keys the transcript
    /// path; absent it the engine writes to a fresh in-memory ledger
    /// only (commits are lost on restart).
    pub fn with_transcript_store(
        mut self,
        store: Arc<coco_session::TranscriptStore>,
        session_id: String,
    ) -> Self {
        self.transcript_store = Some(store);
        self.transcript_session_id = Some(session_id);
        self
    }

    /// Install a staged-collapse ledger. Required for the
    /// `experimental.staged_compact` strategy; absent by default. The
    /// ledger is shared so resume / persistence layers can read commits.
    pub fn with_staged_ledger(
        mut self,
        ledger: Arc<tokio::sync::Mutex<coco_compact::StagedCompactLedger>>,
        session_id: uuid::Uuid,
    ) -> Self {
        self.staged_ledger = Some(ledger);
        self.staged_session_id = session_id;
        self
    }

    /// Whether the staged-collapse strategy is currently active. Used
    /// as the mutual-exclusion gate for autocompact (TS
    /// autoCompact.ts:215-223) — when collapse owns the threshold
    /// ladder, proactive autocompact is suppressed.
    ///
    /// Inert by default: `with_staged_ledger` has no production callers
    /// (matches TS-feature-stripped `feature('CONTEXT_COLLAPSE')` state),
    /// so the first AND-clause is always false and this returns `false`.
    /// Wire `with_staged_ledger` and flip
    /// `compact.experimental.staged_compact.enabled` together to opt in.
    pub fn is_collapse_active(&self) -> bool {
        self.staged_ledger.is_some() && self.config.compact.experimental.staged_compact.enabled
    }

    // ── Post-turn cache-safe params (D8) ──

    /// Snapshot the current post-turn cache-safe params. `None` until
    /// the first turn finalises, and after `clear_cache_safe_params`.
    /// Future post-turn fork features (`/btw`, `promptSuggestion`,
    /// `postTurnSummary`) read this to share the parent's prompt
    /// cache. TS parity: `forkedAgent.ts::getLastCacheSafeParams`.
    pub async fn last_cache_safe_params(&self) -> Option<coco_types::CacheSafeParams> {
        self.last_cache_safe_params.read().await.clone()
    }

    /// Clone the `Arc<RwLock<...>>` so observers (TUI, transcript
    /// recorder) can poll the slot without holding a `&QueryEngine`.
    /// Read-only contract: callers MUST NOT replace the inner
    /// `Option` from outside `QueryEngine` — use
    /// [`Self::clear_cache_safe_params`] for the only legitimate
    /// non-engine mutation.
    pub fn cache_safe_params_handle(
        &self,
    ) -> std::sync::Arc<tokio::sync::RwLock<Option<coco_types::CacheSafeParams>>> {
        self.last_cache_safe_params.clone()
    }

    /// Drop the cache-safe params slot. Called from `/clear`-style
    /// regen paths so a fork after `/clear` doesn't accidentally
    /// reuse the pre-clear cache key. TS parity:
    /// `forkedAgent.ts::saveCacheSafeParams(null)`.
    pub async fn clear_cache_safe_params(&self) {
        *self.last_cache_safe_params.write().await = None;
    }

    /// Engine-internal writer for the cache-safe params slot. Called
    /// from `finalize_turn_post_tools` after each successful turn —
    /// **not** part of the public API.
    pub(crate) async fn save_cache_safe_params(&self, params: coco_types::CacheSafeParams) {
        *self.last_cache_safe_params.write().await = Some(params);
    }

    /// Build a [`coco_types::CacheSafeParams`] from the current turn's
    /// post-execution history and save it. Called from every exit
    /// path of `run_session_loop` — both the tool-execution path
    /// (via `finalize_turn_post_tools`) and the text-only end-turn
    /// path that returns early.
    pub(crate) async fn save_post_turn_cache_params(
        &self,
        history: &coco_messages::MessageHistory,
    ) {
        if history.is_empty() {
            return;
        }
        // Serialise the post-turn history so the slot can be
        // observed without holding a parent-history reference. Same
        // shape that `AgentQueryConfig.fork_context_messages`
        // expects, so a future fork caller can thread it directly
        // through the existing fork-context plumbing.
        let fork_messages: Vec<serde_json::Value> = history
            .as_slice()
            .iter()
            .filter_map(|m| serde_json::to_value(m).ok())
            .collect();
        let rendered_system_prompt = self.config.system_prompt.clone().unwrap_or_default();
        self.save_cache_safe_params(coco_types::CacheSafeParams {
            rendered_system_prompt,
            model_id: self.config.model_id.clone(),
            fork_context_messages: fork_messages,
        })
        .await;
    }

    /// Cache-break tracking attribution. Mirrors TS `getTrackingKey`:
    /// subagents land under `agent:custom` (with their `agent_id`),
    /// SDK calls under `sdk`, everything else under `repl_main_thread`.
    pub(crate) fn query_source_label(&self) -> &'static str {
        if self.config.agent_id.is_some() {
            "agent:custom"
        } else if self.config.is_non_interactive {
            "sdk"
        } else {
            "repl_main_thread"
        }
    }

    /// Record paths that were just @mentioned so they get priority during
    /// post-compact file restoration. Newest entries push older ones out
    /// once the LRU exceeds [`Self::MENTION_PRIORITY_CAPACITY`].
    pub async fn note_mentioned_paths<I>(&self, paths: I)
    where
        I: IntoIterator<Item = std::path::PathBuf>,
    {
        let mut g = self.recently_mentioned_paths.write().await;
        for path in paths {
            // Move existing entry to the back instead of duplicating.
            if let Some(idx) = g.iter().position(|p| p == &path) {
                g.remove(idx);
            }
            g.push_back(path);
        }
        while g.len() > Self::MENTION_PRIORITY_CAPACITY {
            g.pop_front();
        }
    }

    /// Snapshot the current set of recently @mentioned paths.
    pub async fn recently_mentioned_paths_snapshot(
        &self,
    ) -> std::collections::HashSet<std::path::PathBuf> {
        let g = self.recently_mentioned_paths.read().await;
        g.iter().cloned().collect()
    }

    /// Replace the post-compact skill snapshot. Callers pre-convert from
    /// their canonical skill state (e.g.
    /// `coco_system_reminder::InvokedSkillEntry`) into
    /// `coco_compact::PostCompactSkill`. Empty vec disables the in-band
    /// path; the next-turn `InvokedSkillsGenerator` still re-injects.
    pub fn set_post_compact_skills(&self, skills: Vec<coco_compact::PostCompactSkill>) {
        if let Ok(mut g) = self.post_compact_skills.write() {
            *g = skills;
        }
    }

    /// Install the session-memory text snapshot. Callers (CLI/TUI/SDK)
    /// load `~/.coco/<session>/session-memory/summary.md` at startup
    /// and refresh it after every extraction. Empty string ≡ no SM
    /// available, the SM-first compact path becomes a pass-through.
    pub fn with_session_memory_text(self, text: String) -> Self {
        if let Ok(mut guard) = self.session_memory_text.try_write() {
            *guard = text;
        }
        self
    }

    /// Async setter used by the extraction pipeline to refresh SM text
    /// after a forked-agent extraction completes.
    pub async fn set_session_memory_text(&self, text: String) {
        let mut guard = self.session_memory_text.write().await;
        *guard = text;
    }

    /// Install the session-memory service so `try_session_memory_compact`
    /// can wait for any in-flight extraction.
    pub fn with_session_memory_service(
        mut self,
        svc: Arc<coco_session_memory::SessionMemoryService>,
    ) -> Self {
        self.session_memory_service = Some(svc);
        self
    }

    /// Install the auto-memory runtime — extraction / dream / 9-section
    /// session memory / recall ranker. Set by the CLI / SDK bootstrap
    /// when `Feature::AutoMemory` is enabled. Without this the
    /// turn-end fan-out stays inert.
    pub fn with_memory_runtime(mut self, runtime: Arc<coco_memory::MemoryRuntime>) -> Self {
        self.memory_runtime = Some(runtime);
        self
    }

    /// Public accessor used by `/dream` and `/summary` slash-command
    /// dispatchers in the SDK / TUI runner to reach the runtime
    /// services. Returns `None` when `Feature::AutoMemory` is off.
    pub fn memory_runtime(&self) -> Option<&Arc<coco_memory::MemoryRuntime>> {
        self.memory_runtime.as_ref()
    }

    /// Install the running-task manager so post-compact attachments can
    /// snapshot active background agents and re-emit them as
    /// `task_status` reminders. TS: `createAsyncAgentAttachmentsIfNeeded`
    /// reads `appState.tasks` directly; coco-rs reads from the
    /// `TaskManager` exposed by `coco-tasks::running`. Optional — when
    /// absent, post-compact emits zero `task_status` attachments
    /// (degrades gracefully to TS feature-stripped behavior).
    pub fn with_running_tasks(mut self, running: Arc<coco_tasks::running::TaskManager>) -> Self {
        self.running_tasks = Some(running);
        self
    }

    /// Snapshot running async-agent tasks for post-compact attachment
    /// emission. Filters per TS `compact.ts:1577-1582`: skip the agent
    /// that owns this engine's `agent_id`, drop pending tasks (not yet
    /// meaningful), drop terminal tasks the model already saw notified.
    pub(crate) async fn snapshot_async_agents_for_post_compact(
        &self,
    ) -> Vec<coco_compact::AsyncAgentSnapshot> {
        let Some(tasks) = &self.running_tasks else {
            return Vec::new();
        };
        let listed = tasks.list().await;
        let self_agent = self.config.agent_id.as_deref();
        listed
            .into_iter()
            .filter(|t| matches!(t.task_type, coco_types::TaskType::LocalAgent))
            .filter(|t| !matches!(t.status, coco_types::TaskStatus::Pending))
            // Skip the engine's own agent — it's already part of the
            // visible conversation, not a peer that the model could
            // duplicate.
            .filter(|t| match self_agent {
                Some(a) => t.id.as_str() != a,
                None => true,
            })
            // TS additionally filters `agent.retrieved == true`. coco-rs
            // task taxonomy doesn't carry an explicit "retrieved" bit;
            // the closest analog is `notified` (the SDK has emitted the
            // completion event). Using `notified` keeps duplicate-spawn
            // protection without re-listing already-acknowledged work.
            .filter(|t| !(t.status.is_terminal() && t.notified))
            .map(|t| coco_compact::AsyncAgentSnapshot {
                task_id: t.id,
                status: task_status_to_ts_string(t.status),
                description: t.description,
                delta_summary: None,
                output_file_path: t.output_file,
            })
            .collect()
    }

    /// Stamp the most recent assistant timestamp (called from the stream
    /// accumulator on every `TurnCompleted`). Drives time-based MC.
    pub fn stamp_assistant_now(&self) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        self.last_assistant_ms
            .store(now_ms, std::sync::atomic::Ordering::Release);
    }

    /// Install the compaction observer registry. Caller builds the
    /// registry, registers per-subsystem observers, then hands an
    /// `Arc` to the engine so notifications fire in `try_full_compact`.
    /// Omitting this leaves an empty registry — equivalent to TS skipping
    /// `runPostCompactCleanup` when the corresponding caches don't exist.
    pub fn with_compaction_observers(
        mut self,
        observers: Arc<coco_compact::CompactionObserverRegistry>,
    ) -> Self {
        self.compaction_observers = observers;
        self
    }

    /// A clone-friendly emitter handle for owning crates (hooks /
    /// permissions / commands / core/tool-runtime / skills) so they can push
    /// `Message::Attachment` entries into this session's history without
    /// direct access to the engine. Drained once per outer-loop turn.
    pub fn attachment_emitter(&self) -> coco_messages::AttachmentEmitter {
        coco_messages::AttachmentEmitter::new(self.attachment_tx.clone())
    }

    /// Drain any silent attachments emitted since the last turn into
    /// `history`. Called at the head of each outer-loop iteration.
    /// Returns the number of drained attachments for telemetry.
    pub(crate) async fn drain_attachment_inbox(&self, history: &mut MessageHistory) -> usize {
        let mut count = 0;
        let mut rx = self.attachment_rx.lock().await;
        while let Ok(att) = rx.try_recv() {
            history
                .messages
                .push(coco_messages::Message::Attachment(att));
            count += 1;
        }
        count
    }

    /// Install the per-subsystem reminder source bundle. Each
    /// `Some(Arc<dyn XxxSource>)` field powers a category of
    /// system-reminders that needs state from an owning crate
    /// (hooks, LSP, tasks, skills, MCP, swarm, bridge, memory).
    /// Omitted sources → corresponding reminders silently skip.
    ///
    /// TS parity: this is the analog of `toolUseContext.options.*`
    /// that TS's `getAttachments` reads from.
    pub fn with_reminder_sources(mut self, sources: coco_system_reminder::ReminderSources) -> Self {
        self.reminder_sources = sources;
        self
    }

    /// Install a mailbox handle for swarm teammate messaging.
    pub fn with_mailbox(mut self, mailbox: coco_tool_runtime::MailboxHandleRef) -> Self {
        self.mailbox = Some(mailbox);
        self
    }

    /// Install the MCP handle so prompt-rendering can read the
    /// connected-server set and pre-filter agents whose
    /// `required_mcp_servers` aren't ready. `None` (the default)
    /// makes the AgentTool prompt skip MCP filtering at the renderer.
    pub fn with_mcp_handle(mut self, handle: coco_tool_runtime::McpHandleRef) -> Self {
        self.mcp_handle = Some(handle);
        self
    }

    /// Install the real [`AgentHandle`](coco_tool_runtime::AgentHandle) so
    /// `AgentTool` invocations route to the swarm / subagent
    /// runtime. Without this the factory defaults to
    /// `NoOpAgentHandle` and every `AgentTool` call returns a clean
    /// "not available in this context" error — fine for tests, but
    /// CLI / SDK / TUI runners should install a real handle at
    /// bootstrap.
    pub fn with_agent_handle(mut self, handle: coco_tool_runtime::AgentHandleRef) -> Self {
        self.agent_handle = Some(handle);
        self
    }

    /// Install a fork dispatcher (D1/D2). Used by post-turn forks
    /// (`/btw`, `promptSuggestion`) to drive a fresh engine without
    /// mutating the parent. CLI / SDK runners install the same
    /// instance — usually backed by `SessionRuntime`.
    pub fn with_fork_dispatcher(
        mut self,
        dispatcher: crate::forked_agent::ForkDispatcherRef,
    ) -> Self {
        self.fork_dispatcher = Some(dispatcher);
        self
    }

    /// Read the engine's installed fork dispatcher. `None` until
    /// installed via [`Self::with_fork_dispatcher`].
    pub fn fork_dispatcher(&self) -> Option<crate::forked_agent::ForkDispatcherRef> {
        self.fork_dispatcher.clone()
    }

    /// Install the background task runtime. CLI bootstrap shares
    /// the same `TaskRuntime` Arc between the engine (read/control
    /// side via this builder) and `SwarmAgentHandle` (registration
    /// side). When absent, the engine threads `None` into
    /// `ToolUseContext.task_handle`, where the task tools surface a
    /// "no task runtime configured" error.
    pub fn with_task_handle(mut self, handle: coco_tool_runtime::TaskHandleRef) -> Self {
        self.task_handle = Some(handle);
        self
    }

    /// Install a single fallback [`ApiClient`]. Convenience wrapper
    /// for the common one-tier case; equivalent to
    /// Replace the engine's primary `ApiClient`. Used by the
    /// subagent factory in [`crate::agent_adapter::QueryEngineAdapter`]
    /// to inject a per-`ModelRole` client when spawning a child:
    /// the factory rebuilds the engine via
    /// `SessionRuntime::build_engine_from_config`, then overrides the
    /// client to the role-resolved one before handing the engine to
    /// the runner. (P1)
    ///
    /// Production callers go through the factory; manual use is
    /// rare. The method preserves all other engine state (config,
    /// hooks, registries, fallback chain).
    #[must_use]
    pub fn with_client(mut self, client: Arc<ApiClient>) -> Self {
        self.client = client;
        self
    }

    /// `.with_fallback_clients(vec![client])`.
    pub fn with_fallback_client(mut self, client: Arc<ApiClient>) -> Self {
        self.fallback_clients = vec![client];
        self
    }

    /// Install an ordered chain of fallback [`ApiClient`]s. The
    /// engine walks slot 0 → slot 1 → … on capacity-error streaks
    /// via [`ModelRuntime::advance`]. Empty input = no fallback.
    pub fn with_fallback_clients(mut self, clients: Vec<Arc<ApiClient>>) -> Self {
        self.fallback_clients = clients;
        self
    }

    /// Install a half-open recovery policy for the session. Enables
    /// periodic probes back to primary after a fallback switch;
    /// see [`coco_config::FallbackRecoveryPolicy`]. Omitting this
    /// call keeps the default sticky-fallback behavior.
    pub fn with_recovery_policy(mut self, policy: coco_config::FallbackRecoveryPolicy) -> Self {
        self.recovery_policy = Some(policy);
        self
    }

    /// Install the real [`SkillHandle`](coco_tool_runtime::SkillHandle) so
    /// `SkillTool` invocations route to the skill runtime (inline
    /// expansion or forked subagent). Without this the factory
    /// defaults to `NoOpSkillHandle` and every skill call returns
    /// `SkillInvocationError::Unavailable` — the runner surfaces
    /// that as a model-visible error.
    pub fn with_skill_handle(mut self, handle: coco_tool_runtime::SkillHandleRef) -> Self {
        self.skill_handle = Some(handle);
        self
    }

    /// Install the agent-definition catalog snapshot (T7). When set,
    /// AgentTool resolves `subagent_type` to a full `AgentDefinition`
    /// at the spawn boundary so the runner reads `definition.model` /
    /// `definition.model_role`. Without this the catalog is
    /// unavailable and AgentTool falls back to subagent_type→role
    /// mapping alone — same behaviour as before T7. Bootstrap should
    /// pass `coco_subagent::AgentDefinitionStore::snapshot()` here.
    pub fn with_agent_catalog(mut self, catalog: Arc<coco_subagent::AgentCatalogSnapshot>) -> Self {
        self.agent_catalog = Some(catalog);
        self
    }

    /// Install the durable task-list store (V2 task tools).
    pub fn with_task_list(mut self, handle: coco_tool_runtime::TaskListHandleRef) -> Self {
        self.task_list = Some(handle);
        self
    }

    /// Install the ephemeral per-agent todo store (V1 TodoWrite).
    pub fn with_todo_list(mut self, handle: coco_tool_runtime::TodoListHandleRef) -> Self {
        self.todo_list = Some(handle);
        self
    }

    /// Attach auto-mode state + rules so `PermissionDecision::Ask` outcomes
    /// are first classified by the 2-stage LLM sidequery before falling back
    /// to interactive approval.
    pub fn with_auto_mode(
        mut self,
        state: Arc<coco_permissions::AutoModeState>,
        denial_tracker: Arc<tokio::sync::Mutex<coco_permissions::DenialTracker>>,
        rules: coco_permissions::AutoModeRules,
    ) -> Self {
        self.auto_mode_state = Some(state);
        self.denial_tracker = Some(denial_tracker);
        self.auto_mode_rules = rules;
        self
    }

    /// Attach session bootstrap data to be emitted as `SessionStarted`
    /// before the first turn. Without this, the engine still runs normally
    /// but does not emit `SessionStarted` (backwards compatible for tests).
    pub fn with_session_bootstrap(mut self, bootstrap: SessionBootstrap) -> Self {
        self.session_bootstrap = Some(bootstrap);
        self
    }

    /// Attach a permission bridge so `PermissionDecision::Ask` outcomes
    /// are forwarded to an external authority (e.g. the SDK client via
    /// `SdkPermissionBridge`) instead of auto-allowing.
    pub fn with_permission_bridge(
        mut self,
        bridge: coco_tool_runtime::ToolPermissionBridgeRef,
    ) -> Self {
        self.permission_bridge = Some(bridge);
        self
    }

    /// Set file read state for @mention dedup and changed-file detection.
    pub fn with_file_read_state(
        mut self,
        file_read_state: Arc<RwLock<coco_context::FileReadState>>,
    ) -> Self {
        self.file_read_state = Some(file_read_state);
        self
    }

    /// Attach a shared `ToolAppState` for cross-component signalling.
    ///
    /// Tools read/write this via `ToolUseContext.app_state` — plan mode's
    /// exit flag, plan-file entry timestamp, and the live permission
    /// mode (`permission_mode`, `pre_plan_mode`, `stripped_dangerous_rules`)
    /// are carried here. Without this the engine runs normally but
    /// the plan-mode-exit reminder never fires and tool mode changes
    /// don't propagate across LLM iterations.
    ///
    /// **Bootstrap**: if `app_state.permission_mode` is `None` (fresh
    /// state), it's seeded from `self.config.permission_mode` so the
    /// first batch's [`ToolContextFactory::build`] sees a concrete mode. If
    /// already `Some(_)` (e.g. session resumed, prior-run state
    /// carried), the existing value is preserved — user + tool
    /// intent trumps config. TS parity: `appState` is
    /// initialized-once at session-create and never re-seeded from
    /// config afterward.
    pub fn with_app_state(mut self, app_state: Arc<RwLock<ToolAppState>>) -> Self {
        // Bootstrap the live mode on first attach. This is a one-shot
        // write — subsequent runs that reuse the same app_state see
        // the preserved value rather than an overwrite.
        if let Ok(mut guard) = app_state.try_write()
            && guard.permission_mode.is_none()
        {
            guard.permission_mode = Some(self.config.permission_mode);
        }
        self.app_state = Some(app_state);
        self
    }

    /// Set file history state for checkpoint/rewind support.
    pub fn with_file_history(
        mut self,
        file_history: Arc<RwLock<FileHistoryState>>,
        config_home: std::path::PathBuf,
    ) -> Self {
        self.file_history = Some(file_history);
        self.config_home = Some(config_home);
        self
    }

    /// Set the config home directory. Used by plan-mode (`plans_dir`
    /// resolution) and surfaced on `ToolUseContext.config_home` so
    /// tools can locate the plan file on disk. `with_file_history`
    /// also sets this as a side-effect; use this builder when you
    /// need config_home without attaching a file-history state (e.g.
    /// integration tests).
    pub fn with_config_home(mut self, config_home: std::path::PathBuf) -> Self {
        self.config_home = Some(config_home);
        self
    }

    /// Access the command queue for mid-turn steering.
    pub fn command_queue(&self) -> &CommandQueue {
        &self.command_queue
    }

    /// Access the inbox for teammate messages.
    pub fn inbox(&self) -> &Inbox {
        &self.inbox
    }
}

/// Render a Rust `TaskStatus` to the TS `LocalAgentTaskState.status` string
/// shape — `'pending' | 'running' | 'completed' | 'failed' | 'killed' | 'cancelled'`.
fn task_status_to_ts_string(status: coco_types::TaskStatus) -> String {
    match status {
        coco_types::TaskStatus::Pending => "pending",
        coco_types::TaskStatus::Running => "running",
        coco_types::TaskStatus::Completed => "completed",
        coco_types::TaskStatus::Failed => "failed",
        coco_types::TaskStatus::Killed => "killed",
        coco_types::TaskStatus::Cancelled => "cancelled",
    }
    .to_string()
}
