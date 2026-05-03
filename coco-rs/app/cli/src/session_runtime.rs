//! Per-session runtime container shared by both TUI and SDK runners.
//!
//! The TUI runner (`tui_runner::run_tui` / `run_agent_driver`) and the SDK
//! runner (`sdk_server::sdk_runner::QueryEngineRunner`) both need to:
//!
//! 1. Construct ~12 per-session subsystem state objects at startup
//!    (`FileReadState`, `SessionMemoryService`, `HookRegistry`,
//!    `CompactionObserverRegistry`, `FileHistoryState`, `ToolAppState`,
//!    history Mutex, …).
//! 2. Per-turn, build a `QueryEngine` by chaining ~11 `.with_*` calls
//!    that install those subsystems on the engine.
//! 3. On `/clear`, perform a TS-aligned reset (SessionEnd hooks → drop
//!    caches → regen session id → SessionStart hooks).
//!
//! Before this module existed, both runners had their own copies of
//! steps 1+2+3 — the SDK copy had drifted to ~30% completeness and 7
//! distinct bugs that all had the same shape ("TUI installed X, SDK
//! forgot to install X"). [`SessionRuntime`] is the single owner of
//! that state; both runners construct one at startup, then call
//! [`SessionRuntime::build_engine`] per turn and
//! [`SessionRuntime::clear_conversation`] on `/clear`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tracing::info;
use tracing::warn;

use coco_config::FallbackRecoveryPolicy;
use coco_config::RuntimeConfig;
use coco_context::FileHistorySnapshotSink;
use coco_context::FileHistoryState;
use coco_context::FileReadState;
use coco_hooks::HookRegistry;
use coco_inference::ApiClient;
use coco_messages::Message;
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_session::SessionManager;
use coco_session::TranscriptStore;
use coco_session_memory::SessionMemoryService;
use coco_tool_runtime::AgentHandleRef;
use coco_tool_runtime::MailboxHandleRef;
use coco_tool_runtime::ToolPermissionBridgeRef;
use coco_tool_runtime::ToolRegistry;
use coco_tui::command::ClearScope;
use coco_types::ModelRole;
use coco_types::ModelSpec;
use coco_types::PermissionMode;
use coco_types::ToolAppState;
use tokio_util::sync::CancellationToken;

use crate::Cli;

/// `FileHistorySnapshotSink` that writes via [`TranscriptStore`]. Lives
/// here because both runners need to install it on `FileHistoryState`.
///
/// `session_id` is shared via `Arc<std::sync::RwLock<String>>` so
/// `SessionRuntime::clear_conversation` can swap it in place without
/// rebuilding the sink. Without this, a `/clear` regen would leave the
/// sink writing to the OLD session's transcript jsonl forever — phantom
/// snapshots from the new session leaking into the resumed old one.
struct TranscriptFileHistorySink {
    store: TranscriptStore,
    session_id: Arc<std::sync::RwLock<String>>,
}

impl TranscriptFileHistorySink {
    fn new(sessions_dir: PathBuf, session_id: Arc<std::sync::RwLock<String>>) -> Self {
        Self {
            store: TranscriptStore::new(sessions_dir),
            session_id,
        }
    }
}

#[async_trait::async_trait]
impl FileHistorySnapshotSink for TranscriptFileHistorySink {
    async fn record(
        &self,
        message_id: &str,
        snapshot_json: serde_json::Value,
        is_snapshot_update: bool,
    ) {
        let id = self
            .session_id
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        if let Err(e) = self.store.insert_file_history_snapshot(
            &id,
            message_id,
            snapshot_json,
            is_snapshot_update,
        ) {
            warn!(error = %e, message_id, "failed to persist file-history snapshot");
        }
    }
}

/// Options for building a [`SessionRuntime`].
pub struct SessionRuntimeBuildOpts<'a> {
    pub cli: &'a Cli,
    pub runtime_config: Arc<RuntimeConfig>,
    pub cwd: PathBuf,
    pub model_id: String,
    pub system_prompt: String,
    pub bypass_permissions_available: bool,
    pub permission_mode: PermissionMode,
    pub client: Arc<ApiClient>,
    pub fallback_clients: Vec<Arc<ApiClient>>,
    pub recovery_policy: Option<FallbackRecoveryPolicy>,
    pub tools: Arc<ToolRegistry>,
    pub session_manager: Arc<SessionManager>,
    pub fast_model_spec: Option<ModelSpec>,
    /// SDK runner installs an `SdkPermissionBridge`; TUI passes `None`
    /// and uses interactive approval prompts instead.
    pub permission_bridge: Option<ToolPermissionBridgeRef>,
}

/// All per-session state shared by both runners. Construction at startup
/// is done once via [`SessionRuntime::build`]; per-turn engines are
/// assembled via [`SessionRuntime::build_engine`].
pub struct SessionRuntime {
    // ── immutable resources (never change after build) ─────────────────
    pub client: Arc<ApiClient>,
    /// Main-role fallback chain. Read by [`Self::wire_engine`] to install
    /// `with_fallback_clients` on every per-turn engine.
    fallback_clients: Vec<Arc<ApiClient>>,
    /// Half-open recovery policy for the Main role. `None` ⇒ sticky
    /// fallback semantics. Read by [`Self::wire_engine`].
    recovery_policy: Option<FallbackRecoveryPolicy>,
    /// Tool registry shared by every engine instance. Read by
    /// [`Self::build_engine`] / [`Self::build_engine_from_config`].
    tools: Arc<ToolRegistry>,
    pub config_home: PathBuf,
    pub runtime_config: Arc<RuntimeConfig>,
    pub session_manager: Arc<SessionManager>,
    pub fast_model_spec: Option<ModelSpec>,
    pub auto_title_enabled: bool,
    /// SwarmMailbox handle installed on every engine via `with_mailbox`.
    mailbox: MailboxHandleRef,
    /// Optional SDK permission bridge (None for TUI). Installed via
    /// `with_permission_bridge` when present.
    permission_bridge: Option<ToolPermissionBridgeRef>,
    /// Long-lived parent token for runtime-level lifecycle (hook
    /// orchestration shutdown). Per-turn engine cancels are
    /// independent — see TUI `run_agent_driver` for per-iteration
    /// `CancellationToken::new()`.
    cancel: CancellationToken,

    // ── mutable per-session state (changes on /clear or mid-session) ──
    /// Session id; mutated by [`Self::clear_conversation`] (regen).
    session_id: Arc<RwLock<String>>,
    /// Engine config; mutated by [`Self::clear_conversation`] (session_id)
    /// and [`Self::update_engine_config`]. Read by every per-turn build.
    engine_config: Arc<RwLock<QueryEngineConfig>>,
    pub file_read_state: Arc<RwLock<FileReadState>>,
    pub file_history: Option<Arc<RwLock<FileHistoryState>>>,
    pub app_state: Arc<RwLock<ToolAppState>>,
    /// Session-memory extractor + on-disk cache. Used by
    /// [`Self::wire_engine`] (engine reads `current_text`) and
    /// [`Self::start_new_session`] / [`Self::clear_conversation`]
    /// (session-id retarget + cache wipe).
    session_memory_service: Arc<SessionMemoryService>,
    /// Auto-memory runtime — extraction / dream / 9-section session
    /// memory / recall ranker. `None` when `Feature::AutoMemory` is
    /// off; otherwise threaded into every engine via
    /// [`coco_query::QueryEngine::with_memory_runtime`].
    memory_runtime: Option<Arc<coco_memory::MemoryRuntime>>,
    /// Real `AgentHandle` for `AgentTool` calls and forked subagents.
    /// Constructed once at session start, installed on every engine
    /// via `wire_engine`. `send_message`, team mgmt, async-launched
    /// agent ops work; sync subagent spawns work once the engine
    /// factory is wired (separately).
    swarm_agent_handle: coco_tool_runtime::AgentHandleRef,
    /// Hook registry merged from settings + plugin manifests. Installed
    /// on every engine + driven by SessionStart / SessionEnd in
    /// [`Self::clear_conversation`].
    pub(crate) hook_registry: Arc<HookRegistry>,
    /// Multi-turn agent transcript. Each turn snapshots, appends, and
    /// rewrites this on success.
    pub history: Arc<Mutex<Vec<Message>>>,
    /// Shared session id of the `TranscriptFileHistorySink` (when
    /// file_history is enabled). `clear_conversation` writes the
    /// regenerated session id here so the sink targets the new
    /// session's jsonl on the next snapshot. `None` when
    /// file_checkpointing is disabled.
    file_history_sink_session_id: Option<Arc<std::sync::RwLock<String>>>,
    /// Lazy cache of `ApiClient` per `ModelRole`. `Main` is always
    /// pre-populated (== `self.client`). Other roles are built on
    /// first request via `client_for_role`. Required so
    /// per-role-configured users (e.g. `models.subagent =
    /// openai/gpt-5` while `models.main = anthropic/...`) actually
    /// route subagents through their configured provider instead of
    /// silently reusing Main's client.
    role_clients: tokio::sync::RwLock<std::collections::HashMap<ModelRole, Arc<ApiClient>>>,
    /// Agent-spawn handle used by `AgentTool` / coordinator-mode
    /// workers. `Some(SwarmAgentHandle)` when `Feature::AgentTeams`
    /// is enabled at session bootstrap; `None` falls back to
    /// `NoOpAgentHandle` on every per-turn engine. The handle owns
    /// the in-process runner + team manager + worktree manager;
    /// it's late-bound (set after `build()` returns the `Arc`)
    /// because the underlying [`coco_query::QueryEngineAdapter`]
    /// factory captures `Arc<Self>` and would otherwise create a
    /// chicken-and-egg cycle. P1 (production wiring of subagent
    /// execution).
    agent_handle: Arc<RwLock<Option<AgentHandleRef>>>,
    /// Post-turn fork dispatcher (D1/D2). Same late-bind pattern as
    /// `agent_handle`: built after `build()` returns the `Arc<Self>`
    /// (the dispatcher impl captures the runtime), and installed on
    /// every per-turn engine via `wire_engine`. `None` ⇒ post-turn
    /// forks degrade to no-op (`/btw` returns a hint message,
    /// `promptSuggestion` skips). Real impl lives in
    /// `app/cli/src/fork_dispatcher.rs`.
    fork_dispatcher: Arc<RwLock<Option<coco_query::forked_agent::ForkDispatcherRef>>>,
    /// Background task runtime (TaskHandle implementation) — owns
    /// the `TaskManager` + per-task control state. Shared with
    /// `SwarmAgentHandle` so AgentTool's bg path registers spawns
    /// through the same store the engine's `Task*` tools read from.
    /// `None` resolves to `NoOpTaskHandle` semantics (the task tools
    /// surface a clean "no task runtime configured" error).
    task_runtime: Arc<RwLock<Option<Arc<crate::task_runtime::TaskRuntime>>>>,
    /// Per-agent transcript / metadata store for resume support.
    /// Late-bound so CLI bootstrap can construct the impl after
    /// `SessionRuntime::build` returns. `agent_handle_factory`
    /// installs it onto the SwarmAgentHandle when wiring agent-
    /// team support.
    agent_transcript_store: Arc<RwLock<Option<coco_tool_runtime::AgentTranscriptStoreRef>>>,
    /// MCP handle installed on every per-turn engine via `wire_engine`.
    /// Late-bound so CLI bootstrap can construct the
    /// `McpManagerAdapter` (or any other McpHandle impl) after
    /// `SessionRuntime::build` returns. Without this the engine's
    /// `mcp_handle` slot stays `None` and AgentTool's prompt-time
    /// MCP filter degrades to fail-closed (hides MCP-required
    /// agents).
    mcp_handle: Arc<RwLock<Option<coco_tool_runtime::McpHandleRef>>>,
}

impl SessionRuntime {
    /// Build the full session runtime. Constructs every subsystem TS
    /// `clearConversation` and the per-turn engine assembly need.
    pub async fn build(opts: SessionRuntimeBuildOpts<'_>) -> Result<Arc<Self>> {
        let SessionRuntimeBuildOpts {
            cli,
            runtime_config,
            cwd,
            model_id,
            system_prompt,
            bypass_permissions_available,
            permission_mode,
            client,
            fallback_clients,
            recovery_policy,
            tools,
            session_manager,
            fast_model_spec,
            permission_bridge,
        } = opts;

        let config_home = coco_config::global_config::config_home();
        let session_id = uuid::Uuid::new_v4().to_string();

        // FileReadState — @mention dedup + Read tool dedup.
        let file_read_state = Arc::new(RwLock::new(FileReadState::new()));

        // SessionMemoryService — caller-driven extraction pipeline.
        let session_memory_service = Arc::new(SessionMemoryService::new(
            config_home.clone(),
            session_id.clone(),
        ));
        if let Err(e) = session_memory_service.load_from_disk().await {
            warn!("session-memory load failed (non-fatal): {e}");
        }

        // Install the forked-agent summarizer using the dedicated `Memory`
        // role from settings.json (`models.memory`).
        if let Some(memory_spec) = runtime_config
            .model_roles
            .get(coco_types::ModelRole::Memory)
            .cloned()
        {
            match coco_inference::model_factory::build_api_client(
                &runtime_config,
                &memory_spec,
                runtime_config.api.retry.clone().into(),
            ) {
                Ok(memory_client) => {
                    let summarizer: coco_session_memory::SummarizerFn =
                        Arc::new(move |prompt: String| {
                            let client = memory_client.clone();
                            Box::pin(async move {
                                let params = coco_inference::QueryParams {
                                    prompt: vec![coco_messages::LlmMessage::user_text(&prompt)],
                                    max_tokens: Some(2_000),
                                    thinking_level: None,
                                    fast_mode: false,
                                    tools: None,
                                    context_management: None,
                                    query_source: None,
                                    agent_id: None,
                                    time_since_last_assistant_ms: None,
                                };
                                let result = client.query(&params).await?;
                                let text = result
                                    .content
                                    .iter()
                                    .filter_map(|c| match c {
                                        coco_messages::AssistantContent::Text(t) => {
                                            Some(t.text.as_str())
                                        }
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>()
                                    .join("");
                                Ok(text)
                            })
                        });
                    session_memory_service.set_summarizer(summarizer).await;
                    info!(
                        role = ?coco_types::ModelRole::Memory,
                        model_id = %memory_spec.model_id,
                        "session-memory extractor installed"
                    );
                }
                Err(e) => warn!("session-memory client build failed (non-fatal): {e}"),
            }
        }

        // ── Auto-memory runtime ──
        //
        // Built once per session, gated on `Feature::AutoMemory`. The
        // runtime owns the three services (extract / dream / session
        // memory) plus the recall ranker state. We hand it the
        // resolved `MemoryConfig` (already merged with env overrides)
        // and an `AgentHandle` so the forked extraction / dream
        // subagents can spawn against the same swarm runtime that
        // user-facing `Agent` tool spawns use.
        //
        // The handle starts as `NoOpAgentHandle`; the SDK / TUI
        // runner calls `MemoryRuntime::install_agent` once the real
        // `SwarmAgentHandle` is built. Recall + system-prompt
        // rendering work without an agent handle.
        let memory_runtime = if runtime_config
            .features
            .enabled(coco_types::Feature::AutoMemory)
        {
            let agent: coco_tool_runtime::AgentHandleRef =
                Arc::new(coco_tool_runtime::NoOpAgentHandle);
            let mem_cfg = coco_memory::MemoryConfig::from(runtime_config.memory.clone());
            let runtime = coco_memory::runtime::MemoryRuntimeBuilder::new(
                config_home.clone(),
                cwd.clone(),
                session_id.clone(),
                mem_cfg,
                agent,
            )
            .build();
            info!(
                personal_dir = %runtime.personal_dir().display(),
                "auto-memory runtime initialized"
            );
            // Fire-and-forget auto-dream gate-check at session start.
            // Internal three-gate scheduler short-circuits when time
            // / sessions / lock conditions aren't met, so this is
            // cheap when nothing is due. A full consolidation only
            // runs when `dream_min_hours` (default 24h) have elapsed
            // AND `dream_min_sessions` (default 5) have produced
            // transcripts since the last consolidation. TS parity:
            // `initAutoDream` schedules on session start.
            let runtime_arc = Arc::new(runtime);
            let dream_clone = runtime_arc.clone();
            let transcript_dir = config_home.join("sessions");
            tokio::spawn(async move {
                let now_ms = coco_memory::service::dream::DreamService::now_ms();
                // No session enumeration yet — pass empty slice so
                // the session gate stays the limiting factor until a
                // real session-store iterator is plumbed in.
                let outcome = dream_clone
                    .dream
                    .maybe_consolidate(&transcript_dir, &[], now_ms)
                    .await;
                tracing::debug!(?outcome, "auto-dream gate check at session start");
            });
            Some(runtime_arc)
        } else {
            None
        };

        // ── Swarm agent handle ──
        //
        // Production sessions used to silently fall back to
        // `NoOpAgentHandle` for *every* `AgentTool` / forked
        // subagent call. We now construct a real `SwarmAgentHandle`
        // wired with an `InProcessAgentRunner` so:
        //   - `send_message` reaches teammates,
        //   - team create/delete operations work,
        //   - the memory crate's forked extraction / dream agents
        //     spawn against the same runtime that user-facing
        //     `Agent` tool spawns use.
        //
        // The execution engine (`AgentQueryEngine`) is installed
        // separately by the engine-factory wiring — until that lands
        // sync subagent spawns return a clean "no execution engine"
        // error rather than the stub "agent spawning not available"
        // we used to emit.
        let swarm_agent_handle = {
            // In-process subagents inherit the leader's
            // `ToolPermissionBridge` (installed on `SessionRuntime` and
            // propagated by `wire_engine`); no extra channel needed
            // here. The runner only needs cwd + max-agents.
            let runner = Arc::new(coco_coordinator::runner::InProcessAgentRunner::new(
                cwd.display().to_string(),
                /*max_agents*/ 8,
            ));
            let team_manager = Arc::new(RwLock::new(None));
            let mut handle = coco_coordinator::agent_handle::SwarmAgentHandle::new(
                runner,
                team_manager,
                cwd.display().to_string(),
                runtime_config.clone(),
            );
            // Worktree manager — required for `isolation: "worktree"`
            // subagents. Resolved against the canonical git root so
            // worktrees of one repo share one worktree pool.
            if let Some(repo_root) = coco_git::find_canonical_git_root(&cwd) {
                let manager = Arc::new(coco_coordinator::worktree::AgentWorktreeManager::new(
                    repo_root,
                ));
                handle.set_worktree_manager(manager);
            }
            let arc_handle: coco_tool_runtime::AgentHandleRef = Arc::new(handle);
            arc_handle
        };

        // Now that the real `AgentHandle` exists, install it on the
        // memory runtime so forked extraction / dream agents reach
        // the same swarm runtime instead of the no-op fallback.
        // Install the SideQuery adapter too so the recall ranker
        // dispatches a real `ModelRole::Memory` query instead of
        // falling back to the recency heuristic.
        if let Some(runtime) = &memory_runtime {
            runtime.install_agent(swarm_agent_handle.clone()).await;
            let side_query: coco_tool_runtime::SideQueryHandle =
                Arc::new(crate::side_query_impl::SideQueryAdapter::new(
                    client.clone(),
                    runtime_config.clone(),
                ));
            runtime.install_side_query(side_query).await;
        }

        // FileHistoryState — backed by JSONL transcript when enabled.
        // Sink shares the session_id Arc with SessionRuntime so
        // /clear regen propagates immediately (no rebuild required).
        let (file_history, file_history_sink_session_id) =
            if runtime_config.settings.merged.file_checkpointing_enabled {
                let transcript_dir = config_home.join("sessions");
                let sink_id = Arc::new(std::sync::RwLock::new(session_id.clone()));
                let sink: Arc<dyn FileHistorySnapshotSink> = Arc::new(
                    TranscriptFileHistorySink::new(transcript_dir, sink_id.clone()),
                );
                let mut state = FileHistoryState::new();
                state.set_sink(sink);
                (Some(Arc::new(RwLock::new(state))), Some(sink_id))
            } else {
                (None, None)
            };

        // Shared per-session ToolAppState (plan-mode reminder cadence,
        // exited_plan_mode flag, last_emitted_date latch, etc.).
        let app_state: Arc<RwLock<ToolAppState>> = Arc::new(RwLock::new(ToolAppState::default()));

        // Hook registry — settings hooks first, then plugin hooks
        // layered on top via the bridge so plugin manifests can
        // declare their own SessionStart / PreToolUse / PostCompact /
        // etc. hooks. Same single-scope setup TS uses (see
        // `plugins/loadPlugins`). The PluginManager itself is only
        // needed for the duration of registration — `register_plugin_hooks`
        // copies hook definitions into the registry, so dropping the
        // manager afterward is safe. If a future SDK `plugin/reload`
        // path needs the live manager it can be reintroduced as a
        // proper `Arc<PluginManager>` field; until then we don't pay
        // for the storage.
        let hook_registry = {
            let mut registry = HookRegistry::new();
            if let Some(hooks_value) = &runtime_config.settings.merged.hooks {
                match coco_hooks::load_hooks_from_config(
                    hooks_value,
                    coco_types::HookScope::Project,
                ) {
                    Ok(definitions) => {
                        for def in definitions {
                            registry.register_deduped(def);
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to load hooks from settings — hooks disabled this session");
                    }
                }
            }
            let plugin_dirs = coco_plugins::get_plugin_dirs(&config_home, &cwd);
            let mut plugin_manager = coco_plugins::PluginManager::new();
            plugin_manager.load_from_dirs(&plugin_dirs);
            let plugin_count = plugin_manager.len();
            if plugin_count > 0 {
                info!(
                    plugins = plugin_count,
                    "loaded {plugin_count} plugin(s) from {} dir(s)",
                    plugin_dirs.len()
                );
            }
            // `register_plugin_hooks` uses `register_deduped` internally
            // so a plugin re-declaring a settings hook stays single-fire.
            let plugin_refs: Vec<&coco_plugins::LoadedPlugin> = plugin_manager.enabled();
            if !plugin_refs.is_empty() {
                coco_plugins::hook_bridge::register_plugin_hooks(&mut registry, &plugin_refs);
            }
            Arc::new(registry)
        };

        let mailbox: MailboxHandleRef = Arc::new(coco_coordinator::mailbox::SwarmMailboxHandle);

        // Augment the caller-provided system prompt with the
        // auto-memory section (type taxonomy, how-to-save, MEMORY.md
        // body). The memory crate hands us a pre-rendered block so
        // this crate stays free of memory-prompt assembly logic.
        // Cache-broken upstream by `coco_context::build_system_prompt`
        // when the section is non-empty; we splice the same string in
        // here so the engine's prompt cache prefix sees it.
        let system_prompt_with_memory = if let Some(runtime) = &memory_runtime
            && let Some(section) = runtime.render_system_prompt_section().await
            && !section.is_empty()
        {
            format!("{system_prompt}\n\n{section}")
        } else {
            system_prompt
        };

        // Build the engine config — owns most settings drawn from
        // RuntimeConfig + CLI overrides.
        let engine_config = QueryEngineConfig {
            model_id,
            permission_mode,
            bypass_permissions_available,
            context_window: 200_000,
            max_output_tokens: 16_384,
            max_turns: runtime_config.loop_config.max_turns.unwrap_or(30),
            max_tokens: cli
                .max_tokens
                .or_else(|| runtime_config.loop_config.max_tokens.map(i64::from)),
            system_prompt: Some(system_prompt_with_memory),
            streaming_tool_execution: runtime_config.loop_config.enable_streaming_tools,
            session_id: session_id.clone(),
            project_dir: runtime_config
                .paths
                .project_dir
                .clone()
                .or_else(|| Some(cwd.clone())),
            plan_mode_settings: runtime_config.settings.merged.plan_mode.clone(),
            system_reminder: runtime_config.settings.merged.system_reminder.clone(),
            tool_config: runtime_config.tool.clone(),
            sandbox_config: runtime_config.sandbox.clone(),
            memory_config: runtime_config.memory.clone(),
            shell_config: runtime_config.shell.clone(),
            web_fetch_config: runtime_config.web_fetch.clone(),
            web_search_config: runtime_config.web_search.clone(),
            compact: runtime_config.compact.clone(),
            features: Arc::new(runtime_config.features.clone()),
            tool_overrides: runtime_config.tool_overrides.clone(),
            ..Default::default()
        };

        let auto_title_enabled = runtime_config.settings.merged.session.auto_title;

        let client_for_main_init = client.clone();
        Ok(Arc::new(Self {
            client,
            fallback_clients,
            recovery_policy,
            tools,
            config_home,
            runtime_config,
            session_manager,
            fast_model_spec,
            auto_title_enabled,
            mailbox,
            permission_bridge,
            cancel: CancellationToken::new(),
            session_id: Arc::new(RwLock::new(session_id)),
            engine_config: Arc::new(RwLock::new(engine_config)),
            file_read_state,
            file_history,
            app_state,
            session_memory_service,
            memory_runtime,
            swarm_agent_handle,
            hook_registry,
            history: Arc::new(Mutex::new(Vec::new())),
            file_history_sink_session_id,
            role_clients: tokio::sync::RwLock::new({
                // Pre-populate Main so callers that go through
                // `client_for_role(ModelRole::Main)` still get the
                // canonical client without rebuilding.
                let mut m = std::collections::HashMap::new();
                m.insert(ModelRole::Main, client_for_main_init);
                m
            }),
            // Late-bound — `attach_agent_handle()` installs after the
            // Arc<SessionRuntime> is constructed so the
            // QueryEngineAdapter factory can close over Arc<Self>.
            agent_handle: Arc::new(RwLock::new(None)),
            fork_dispatcher: Arc::new(RwLock::new(None)),
            task_runtime: Arc::new(RwLock::new(None)),
            agent_transcript_store: Arc::new(RwLock::new(None)),
            mcp_handle: Arc::new(RwLock::new(None)),
        }))
    }

    /// Install the MCP handle that every per-turn engine receives via
    /// `wire_engine`. Call this after `SessionRuntime::build` returns
    /// so the bootstrap can wrap a real `McpConnectionManager`.
    pub async fn attach_mcp_handle(&self, handle: coco_tool_runtime::McpHandleRef) {
        let mut slot = self.mcp_handle.write().await;
        *slot = Some(handle);
    }

    /// Snapshot the installed MCP handle. `None` ⇒ no handle wired.
    pub async fn current_mcp_handle(&self) -> Option<coco_tool_runtime::McpHandleRef> {
        self.mcp_handle.read().await.clone()
    }

    /// Snapshot the current session id (cheap clone of the inner String).
    pub async fn current_session_id(&self) -> String {
        self.session_id.read().await.clone()
    }

    /// Resolve an `ApiClient` for the given `ModelRole`. Lazily builds
    /// and caches via [`coco_inference::model_factory::build_api_client`] on
    /// first access; subsequent calls return the cached `Arc`.
    /// Returns `Err` when the runtime config has no spec for `role`
    /// AND `role != Main` — the runtime always owns a Main client.
    ///
    /// Why this exists: the previous design assumed every model call
    /// went through `runtime.client` (= Main). Multi-provider configs
    /// like `models.subagent = openai/gpt-5` would silently reuse
    /// Main's client, defeating the user's per-role routing.
    pub async fn client_for_role(&self, role: ModelRole) -> anyhow::Result<Arc<ApiClient>> {
        // Fast path: cached.
        {
            let g = self.role_clients.read().await;
            if let Some(c) = g.get(&role) {
                return Ok(c.clone());
            }
        }
        // Main always pre-populated; reaching here for Main means the
        // cache map was tampered with — refresh.
        if role == ModelRole::Main {
            let mut g = self.role_clients.write().await;
            g.insert(ModelRole::Main, self.client.clone());
            return Ok(self.client.clone());
        }
        // Resolve role spec from runtime config; fall back to Main
        // when unconfigured (matches `RuntimeConfig::resolve_model_roles`
        // semantics — unconfigured roles inherit Main's spec).
        let spec = self
            .runtime_config
            .model_roles
            .get(role)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("model role {role:?} unresolved (no Main fallback either)")
            })?;
        let retry: coco_inference::RetryConfig = self.runtime_config.api.retry.clone().into();
        let built =
            coco_inference::model_factory::build_api_client(&self.runtime_config, &spec, retry)?;
        let mut g = self.role_clients.write().await;
        // Lost-update protection: another waiter may have built first.
        if let Some(existing) = g.get(&role) {
            return Ok(existing.clone());
        }
        g.insert(role, built.clone());
        Ok(built)
    }

    /// Snapshot the current `QueryEngineConfig` (clones the inner struct).
    /// Per-turn engine builds use this so mid-session mutations
    /// (`set_permission_mode`, `/clear` regen) propagate immediately.
    pub async fn current_engine_config(&self) -> QueryEngineConfig {
        self.engine_config.read().await.clone()
    }

    /// Build a fresh `QueryEngine` for one turn using the runtime's
    /// stored `engine_config`. Both runners share this so the wiring
    /// can never drift. The session-memory text is refreshed from disk
    /// before each build so a fresh extraction shows up on the next turn.
    pub async fn build_engine(&self, cancel: CancellationToken) -> QueryEngine {
        let engine_config = self.current_engine_config().await;
        let engine = QueryEngine::new(
            engine_config,
            self.client.clone(),
            self.tools.clone(),
            cancel,
            Some(self.hook_registry.clone()),
        );
        self.wire_engine(engine, None).await
    }

    /// Build a fresh `QueryEngine` from a caller-provided
    /// `QueryEngineConfig`. Used by SDK paths whose per-turn config
    /// fields (model, session_id, max_*) come from the
    /// `turn/start` request and override the runtime defaults.
    ///
    /// `app_state_override` lets the caller pin a specific
    /// `ToolAppState` Arc — SDK passes `Some(handoff.app_state)` so
    /// per-session app state and the compaction observers built from
    /// it stay coherent. TUI passes `None` and inherits
    /// `runtime.app_state`.
    pub async fn build_engine_from_config(
        &self,
        config: QueryEngineConfig,
        cancel: CancellationToken,
        app_state_override: Option<Arc<RwLock<ToolAppState>>>,
    ) -> QueryEngine {
        let engine = QueryEngine::new(
            config,
            self.client.clone(),
            self.tools.clone(),
            cancel,
            Some(self.hook_registry.clone()),
        );
        self.wire_engine(engine, app_state_override).await
    }

    /// Install every per-session subsystem on a pre-built engine. The
    /// single source of truth for "what subsystems an engine needs" —
    /// both runners route through this so a new subsystem only needs
    /// adding here, not in two transport-specific spots.
    ///
    /// `app_state_override`: when `Some`, this Arc is what the engine
    /// gets via `with_app_state`, AND it's what the compaction
    /// observers reset on `notify_all`. When `None`, falls back to the
    /// runtime's own `app_state`. Without this override, SDK's
    /// `handoff.app_state` would be installed on the engine but
    /// `runtime.app_state` would be reset by observers — the two would
    /// drift after every compaction.
    pub async fn wire_engine(
        &self,
        mut engine: QueryEngine,
        app_state_override: Option<Arc<RwLock<ToolAppState>>>,
    ) -> QueryEngine {
        let app_state = app_state_override.unwrap_or_else(|| self.app_state.clone());
        if !self.fallback_clients.is_empty() {
            engine = engine.with_fallback_clients(self.fallback_clients.clone());
        }
        if let Some(policy) = self.recovery_policy {
            engine = engine.with_recovery_policy(policy);
        }
        engine = engine.with_file_read_state(self.file_read_state.clone());
        engine = engine.with_app_state(app_state.clone());
        let sm_text_now = self.session_memory_service.current_text().await;
        engine = engine.with_session_memory_text(sm_text_now);
        engine = engine.with_session_memory_service(self.session_memory_service.clone());
        // Install the real swarm-backed AgentHandle so AgentTool /
        // SendMessageTool / TeamCreateTool reach the swarm runtime
        // on every engine instance.
        engine = engine.with_agent_handle(self.swarm_agent_handle.clone());
        if let Some(runtime) = &self.memory_runtime {
            engine = engine.with_memory_runtime(runtime.clone());
            // Install the MemoryAdapter as the system-reminder
            // pipeline's MemorySource — `RelevantMemoryGenerator`
            // calls into it each turn to pick up to 5 ranked memory
            // files. Without this wiring the runtime ranks memories
            // that the reminder layer never sees.
            let sources = coco_system_reminder::ReminderSources {
                memory: Some(Arc::new(coco_query::reminder_adapters::MemoryAdapter::new(
                    runtime.clone(),
                ))),
                ..Default::default()
            };
            engine = engine.with_reminder_sources(sources);
        }
        // Build observers fresh per call so the FileReadState and
        // AppState observers reference the engine's actual handles.
        // Cheap — the registry is just a Vec of Arc<dyn Observer>.
        let observers = coco_query::observers::build_default_registry(
            Some(self.file_read_state.clone()),
            /*denial_tracker*/ None,
            Some(app_state),
        );
        engine = engine.with_compaction_observers(observers);
        engine = engine.with_mailbox(self.mailbox.clone());
        // Install the MCP handle so AgentTool::prompt's per-turn
        // dynamic listing can pre-filter agents whose
        // `required_mcp_servers` aren't connected. Snapshot semantics:
        // each engine instance reads the handle slot at wire time;
        // hot-reloads land on the next engine.
        if let Some(mcp) = self.mcp_handle.read().await.clone() {
            engine = engine.with_mcp_handle(mcp);
        }
        if let Some(fh) = &self.file_history {
            engine = engine.with_file_history(fh.clone(), self.config_home.clone());
        }
        if let Some(bridge) = &self.permission_bridge {
            engine = engine.with_permission_bridge(bridge.clone());
        }
        // Agent handle (P1): only installed when `attach_agent_handle`
        // ran at bootstrap (i.e. `Feature::AgentTeams` is on). Without
        // it, the engine factory's default `NoOpAgentHandle` answers
        // `AgentTool` / `SendMessage` / `TeamCreate` calls with a
        // model-visible "not available in this context" error.
        if let Some(handle) = self.agent_handle.read().await.clone() {
            engine = engine.with_agent_handle(handle);
        }
        // Fork dispatcher (D1/D2). Same late-bind contract as
        // `agent_handle` — installed only when `attach_fork_dispatcher`
        // ran at bootstrap. Without it, post-turn forks fall back to
        // their no-op paths (placeholder text / silent skip).
        if let Some(dispatcher) = self.fork_dispatcher.read().await.clone() {
            engine = engine.with_fork_dispatcher(dispatcher);
        }
        // Production task runtime — same `Arc` is shared with
        // `SwarmAgentHandle` so AgentTool background spawns and the
        // engine's `Task*` tools see one source of truth.
        if let Some(rt) = self.task_runtime.read().await.clone() {
            engine = engine.with_task_handle(rt as coco_tool_runtime::TaskHandleRef);
        }
        engine
    }

    /// Install the agent-spawn handle on this runtime. Called once
    /// after `build()` returns the `Arc<Self>`, by the bootstrap path
    /// in `app/cli/main.rs` when `Feature::AgentTeams` is enabled. The
    /// handle is late-bound because the adapter inside it needs to
    /// capture `Arc<Self>` to drive per-spawn engine builds — calling
    /// this from inside `build()` would create a cycle.
    pub async fn attach_agent_handle(&self, handle: AgentHandleRef) {
        *self.agent_handle.write().await = Some(handle);
    }

    /// Install the post-turn fork dispatcher (D1/D2). Late-bound for
    /// the same Arc-cycle reason as `attach_agent_handle`: the
    /// dispatcher impl captures `Arc<Self>` to build per-fork engines.
    pub async fn attach_fork_dispatcher(
        &self,
        dispatcher: coco_query::forked_agent::ForkDispatcherRef,
    ) {
        *self.fork_dispatcher.write().await = Some(dispatcher);
    }

    /// Read the currently installed fork dispatcher. Returns `None`
    /// before bootstrap installs one (or in unit tests). Used by SDK
    /// runners that want to dispatch a fork outside of the engine's
    /// post-turn hook (`/btw` over the SDK protocol).
    pub async fn current_fork_dispatcher(
        &self,
    ) -> Option<coco_query::forked_agent::ForkDispatcherRef> {
        self.fork_dispatcher.read().await.clone()
    }

    /// Install the background task runtime. Called once during CLI
    /// bootstrap; the same `Arc` flows into `SwarmAgentHandle` for
    /// the registration side. Idempotent — re-attaching replaces.
    pub async fn attach_task_runtime(&self, rt: Arc<crate::task_runtime::TaskRuntime>) {
        *self.task_runtime.write().await = Some(rt);
    }

    /// Read the installed task runtime. `None` when no production
    /// runtime is wired (tests, headless paths that don't use bg
    /// AgentTool). Used by `agent_handle_factory` to share the same
    /// instance with `SwarmAgentHandle`.
    pub async fn current_task_runtime(&self) -> Option<Arc<crate::task_runtime::TaskRuntime>> {
        self.task_runtime.read().await.clone()
    }

    /// Install the per-agent transcript / metadata store used for
    /// background AgentTool resume. Late-bind: same lifecycle as
    /// `attach_task_runtime`. `agent_handle_factory` reads this and
    /// forwards onto `SwarmAgentHandle::set_transcript_store`.
    pub async fn attach_agent_transcript_store(
        &self,
        store: coco_tool_runtime::AgentTranscriptStoreRef,
    ) {
        *self.agent_transcript_store.write().await = Some(store);
    }

    /// Read the installed agent-transcript store.
    pub async fn current_agent_transcript_store(
        &self,
    ) -> Option<coco_tool_runtime::AgentTranscriptStoreRef> {
        self.agent_transcript_store.read().await.clone()
    }

    /// Reset all per-session subsystems and adopt a new session id.
    ///
    /// Used by SDK `session/start` to flip from an archived session to
    /// a fresh one without rebuilding the entire `SessionRuntime`.
    /// Caller-owned state (`SessionHandle.history`,
    /// `SessionHandle.app_state` per the SDK protocol) is created fresh
    /// by the caller; this method only refreshes runtime-owned state
    /// keyed on session_id.
    ///
    /// What gets reset:
    /// - `runtime.session_id` → `new_session_id`
    /// - `runtime.engine_config.session_id` (next per-turn engine sees it)
    /// - `runtime.session_memory_service` (`set_session_id` + cache wipe)
    /// - `runtime.file_read_state` (LRU cleared so prior session's
    ///   @mention dedup doesn't leak)
    /// - `runtime.file_history_sink_session_id` Arc (next snapshot
    ///   targets new session's transcript jsonl)
    /// - cache-break detector on `client` + each `fallback_clients`
    ///   (baseline drop on first new-session call won't false-positive)
    ///
    /// What stays:
    /// - `hook_registry`, `tools`, `client` (and Arc identity), other
    ///   process-level resources — these are correctly cross-session.
    ///
    /// Distinct from `clear_conversation`: that fires SessionEnd /
    /// SessionStart hooks and runs through the TS-aligned `/clear` flow.
    /// This method skips both — SDK `session/archive` is the hook
    /// boundary on its own, not the new session's start.
    pub async fn start_new_session(&self, new_session_id: String) {
        self.adopt_session_id(&new_session_id).await;
        {
            let mut frs = self.file_read_state.write().await;
            frs.clear();
        }
        self.reset_cache_break_detectors().await;
    }

    /// Repoint every session-id-keyed subsystem at `new_session_id`.
    ///
    /// Both `start_new_session` (SDK `session/start`) and the full
    /// `/clear` path call this so the swap stays in lockstep:
    /// `runtime.session_id`, `engine_config.session_id`,
    /// `SessionMemoryService` (which also clears its caches),
    /// and the `TranscriptFileHistorySink`'s shared id Arc.
    async fn adopt_session_id(&self, new_session_id: &str) {
        {
            let mut s = self.session_id.write().await;
            *s = new_session_id.to_string();
        }
        let new_id_for_cfg = new_session_id.to_string();
        self.update_engine_config(|cfg| cfg.session_id = new_id_for_cfg)
            .await;
        self.session_memory_service
            .set_session_id(new_session_id.to_string())
            .await;
        if let Some(sink_id) = &self.file_history_sink_session_id
            && let Ok(mut g) = sink_id.write()
        {
            *g = new_session_id.to_string();
        }
    }

    /// Clear cache-break tracking on Main + every Main-fallback client.
    /// Called whenever the agent transcript is being reset (new SDK
    /// session, full `/clear`, history-only `/clear`) so the next
    /// outbound prompt establishes a fresh baseline rather than
    /// false-positive-firing against the prior session's snapshot.
    async fn reset_cache_break_detectors(&self) {
        self.client.cache_break_reset().await;
        for fb in &self.fallback_clients {
            fb.cache_break_reset().await;
        }
    }

    /// Mutate `engine_config` under lock. Use for mid-session updates
    /// like `SetPermissionMode`.
    pub async fn update_engine_config<F>(&self, f: F)
    where
        F: FnOnce(&mut QueryEngineConfig),
    {
        let mut g = self.engine_config.write().await;
        f(&mut g);
    }

    /// TS `clearConversation` (commands/clear/conversation.ts):
    /// SessionEnd hooks → drop subsystem caches → regen session id →
    /// SessionStart hooks (whose result messages seed the new transcript).
    ///
    /// `History` scope is a Rust-only "transcript declutter" shortcut:
    /// resets ToolAppState + cache-break detector only, skips hooks /
    /// caches / session-id regen.
    pub async fn clear_conversation(&self, scope: ClearScope) -> Result<()> {
        let is_history_only = matches!(scope, ClearScope::History);

        // Step 1 (TS conversation.ts:69): SessionEnd hooks fire BEFORE
        // the reset, with the bounded SESSION_END timeout (1.5s default;
        // `COCO_SESSIONEND_HOOKS_TIMEOUT_MS` overrides). History scope
        // skips this — the contract says SessionEnd fires only on actual
        // session boundary.
        if !is_history_only {
            let cur_session_id = self.current_session_id().await;
            let cfg = self.current_engine_config().await;
            let pre_ctx = coco_hooks::orchestration::OrchestrationContext {
                session_id: cur_session_id,
                cwd: std::env::current_dir().unwrap_or_default(),
                project_dir: cfg.project_dir.clone(),
                permission_mode: None,
                cancel: self.cancel.clone(),
                disable_all_hooks: cfg.disable_all_hooks,
                allow_managed_hooks_only: cfg.allow_managed_hooks_only,
                attachment_emitter: coco_messages::AttachmentEmitter::noop(),
            };
            if let Err(e) = coco_hooks::orchestration::execute_session_end(
                &self.hook_registry,
                &pre_ctx,
                "clear",
            )
            .await
            {
                warn!(error = %e, "SessionEnd hook execution failed during /clear");
            }
        }

        // Step 2: always-reset state. ToolAppState + cache-break
        // detector are the common prefix of TS `clearSessionCaches`.
        *self.app_state.write().await = ToolAppState::default();
        self.reset_cache_break_detectors().await;

        if is_history_only {
            return Ok(());
        }

        // Step 3: TS-aligned full reset.
        let cur_session_id = self.current_session_id().await;
        coco_context::clear_plan_slug(&cur_session_id);
        {
            let mut frs = self.file_read_state.write().await;
            frs.clear();
        }
        if let Some(fh) = &self.file_history {
            let mut fh = fh.write().await;
            *fh = FileHistoryState::default();
        }
        self.session_memory_service
            .set_last_summarized_message_id(None)
            .await;

        // Step 4 (TS conversation.ts:203): regenerate the session id and
        // propagate it to every id-keyed subsystem. Without this, post-
        // clear writes would land in the OLD session's directory and
        // surface as "extra memory" / "phantom file-history snapshots"
        // on the next `--resume` of the pre-clear session.
        let new_session_id = uuid::Uuid::new_v4().to_string();
        self.adopt_session_id(&new_session_id).await;

        // Step 5 (TS conversation.ts:245): SessionStart hooks. Result
        // messages seed the post-clear transcript.
        let cfg = self.current_engine_config().await;
        let post_ctx = coco_hooks::orchestration::OrchestrationContext {
            session_id: new_session_id,
            cwd: std::env::current_dir().unwrap_or_default(),
            project_dir: cfg.project_dir.clone(),
            permission_mode: None,
            cancel: self.cancel.clone(),
            disable_all_hooks: cfg.disable_all_hooks,
            allow_managed_hooks_only: cfg.allow_managed_hooks_only,
            attachment_emitter: coco_messages::AttachmentEmitter::noop(),
        };
        let model_arg = if cfg.model_id.is_empty() {
            None
        } else {
            Some(cfg.model_id.as_str())
        };
        match coco_hooks::orchestration::execute_session_start(
            &self.hook_registry,
            &post_ctx,
            "clear",
            /*agent_type*/ None,
            model_arg,
        )
        .await
        {
            Ok(result) => {
                let mut h = self.history.lock().await;
                h.clear();
                for ctx_text in result.additional_contexts {
                    if !ctx_text.trim().is_empty() {
                        h.push(coco_messages::create_user_message(&ctx_text));
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "SessionStart hook execution failed during /clear");
            }
        }

        Ok(())
    }
}
