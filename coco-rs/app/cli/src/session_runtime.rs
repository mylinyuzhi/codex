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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tracing::info;
use tracing::warn;

use coco_commands::CommandRegistry;
use coco_config::FallbackRecoveryPolicy;
use coco_config::RuntimeConfig;
use coco_context::FileHistorySnapshotSink;
use coco_context::FileHistoryState;
use coco_context::FileReadState;
use coco_hooks::HookRegistry;
use coco_inference::ApiClient;
use coco_memory::SessionMemoryService;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_query::CommandQueue;
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_query::SessionStartHookSideEffectSink;
use coco_query::SessionStartHookSideEffects;
use coco_session::SessionManager;
use coco_session::TranscriptStore;
use coco_tool_runtime::AgentHandleRef;
use coco_tool_runtime::MailboxHandleRef;
use coco_tool_runtime::ToolPermissionBridgeRef;
use coco_tool_runtime::ToolRegistry;
use coco_tui::command::ClearScope;
use coco_types::ModelRole;
use coco_types::ModelSpec;
use coco_types::PermissionMode;
use coco_types::ReasoningEffort;
use coco_types::ThinkingLevel;
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
    fn new(
        project_paths: Arc<coco_paths::ProjectPaths>,
        session_id: Arc<std::sync::RwLock<String>>,
    ) -> Self {
        Self {
            store: TranscriptStore::new(project_paths),
            session_id,
        }
    }
}

#[derive(Clone)]
struct FileWatchRegistrationContext {
    file_changed_watcher: Arc<RwLock<Option<crate::file_changed_watcher::FileChangedHookWatcher>>>,
    hook_registry: Arc<HookRegistry>,
    session_id: Arc<RwLock<String>>,
    engine_config: Arc<RwLock<QueryEngineConfig>>,
    cancel: CancellationToken,
    async_hook_registry: Arc<coco_hooks::async_registry::AsyncHookRegistry>,
    hook_llm_handle: Arc<dyn coco_hooks::HookLlmHandle>,
}

struct QuerySessionStartHookSink {
    file_watch: FileWatchRegistrationContext,
}

#[async_trait::async_trait]
impl SessionStartHookSideEffectSink for QuerySessionStartHookSink {
    async fn handle_session_start_hook_side_effects(&self, effects: SessionStartHookSideEffects) {
        if effects.watch_paths.is_empty() {
            return;
        }
        self.file_watch.add_paths(effects.watch_paths).await;
    }
}

impl FileWatchRegistrationContext {
    async fn add_paths(&self, paths: Vec<String>) {
        let path_bufs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let mut slot = self.file_changed_watcher.write().await;
        if slot.is_none() {
            let registry = self.hook_registry.clone();
            let session_id = self.session_id.read().await.clone();
            let cfg = self.engine_config.read().await.clone();
            let disable_all_hooks = cfg.disable_all_hooks;
            let allow_managed_hooks_only = cfg.allow_managed_hooks_only;
            let project_dir = cfg.project_dir;
            let cwd = std::env::current_dir().unwrap_or_default();
            let cancel = self.cancel.clone();
            let async_registry = self.async_hook_registry.clone();
            let llm_handle = self.hook_llm_handle.clone();
            let factory: Arc<
                dyn Fn() -> coco_hooks::orchestration::OrchestrationContext + Send + Sync,
            > = Arc::new(move || coco_hooks::orchestration::OrchestrationContext {
                session_id: session_id.clone(),
                cwd: cwd.clone(),
                project_dir: project_dir.clone(),
                permission_mode: None,
                transcript_path: None,
                agent_id: None,
                agent_type: None,
                cancel: cancel.clone(),
                disable_all_hooks,
                allow_managed_hooks_only,
                attachment_emitter: coco_messages::AttachmentEmitter::noop(),
                sync_event_sink: None,
                http_url_allowlist: None,
                http_env_var_policy: None,
                async_registry: Some(async_registry.clone()),
                llm_handle: Some(llm_handle.clone()),
                workspace_trust_accepted: None,
            });
            *slot = crate::file_changed_watcher::FileChangedHookWatcher::new(registry, factory);
        }
        if let Some(watcher) = slot.as_ref() {
            watcher.add_paths(path_bufs);
        }
    }
}

fn clone_std_rwlock<T: Clone>(lock: &std::sync::RwLock<T>) -> T {
    match lock.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

fn write_std_rwlock<T>(lock: &std::sync::RwLock<T>, value: T) {
    match lock.write() {
        Ok(mut guard) => *guard = value,
        Err(poisoned) => *poisoned.into_inner() = value,
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

/// Map a coco-config-reload [`TrackedKind`] to the TS-aligned
/// `ConfigChangeSource` wire string consumed by the
/// `ConfigChange` hook (TS `utils/hooks.ts:4194`). Catalog files
/// (`providers.json`, `models.json`) live alongside the user
/// settings in `~/.coco/`, so they share the `user_settings`
/// source. `flag_settings` falls back to `user_settings` since the
/// TS hook source enum doesn't have a flag-settings variant.
fn config_change_source_for_kind(
    kind: coco_config_reload::TrackedKind,
) -> coco_hooks::orchestration::ConfigChangeSource {
    use coco_config::SettingSource;
    use coco_config::WatchedKind;
    use coco_config_reload::TrackedKind;
    use coco_hooks::orchestration::ConfigChangeSource;
    match kind {
        TrackedKind::Settings(WatchedKind::Settings(SettingSource::User)) => {
            ConfigChangeSource::UserSettings
        }
        TrackedKind::Settings(WatchedKind::Settings(SettingSource::Project)) => {
            ConfigChangeSource::ProjectSettings
        }
        TrackedKind::Settings(WatchedKind::Settings(SettingSource::Local)) => {
            ConfigChangeSource::LocalSettings
        }
        TrackedKind::Settings(WatchedKind::Settings(SettingSource::Policy)) => {
            ConfigChangeSource::PolicySettings
        }
        TrackedKind::Settings(WatchedKind::Settings(
            SettingSource::Plugin | SettingSource::Flag,
        ))
        | TrackedKind::Settings(WatchedKind::ProvidersCatalog | WatchedKind::ModelsCatalog)
        | TrackedKind::FlagSettings => ConfigChangeSource::UserSettings,
    }
}

/// Populate a `HookRegistry` from the current `RuntimeConfig` snapshot
/// + the plugin directories rooted at `config_home`/`cwd`.
///
/// Used both at session bootstrap (`SessionRuntime::new`) and at
/// `/hooks reload` time (`SessionRuntime::reload_hooks`). Settings
/// sources are loaded in lowest-precedence-first order so the registry
/// vec mirrors TS settings layering for deterministic iteration. TS
/// keys hook source per entry (`hooksSettings.ts:103-141`); collapsing
/// to a single scope drops scope-precedence sorting in `find_matching`
/// (`hooks/src/lib.rs:296-300`).
fn populate_hook_registry(
    registry: &HookRegistry,
    runtime_config: &coco_config::RuntimeConfig,
    config_home: &std::path::Path,
    cwd: &std::path::Path,
) {
    let policy = coco_hooks::LoaderPolicy {
        disable_all_hooks: runtime_config.settings.merged.disable_all_hooks,
        allow_managed_hooks_only: runtime_config.settings.merged.allow_managed_hooks_only,
    };
    for source in [
        coco_config::SettingSource::User,
        coco_config::SettingSource::Project,
        coco_config::SettingSource::Local,
        coco_config::SettingSource::Flag,
        coco_config::SettingSource::Policy,
    ] {
        let Some(value) = runtime_config.settings.per_source.get(&source) else {
            continue;
        };
        let Some(hooks_value) = value.get("hooks") else {
            continue;
        };
        let scope = match source {
            coco_config::SettingSource::User => coco_types::HookScope::User,
            coco_config::SettingSource::Project => coco_types::HookScope::Project,
            coco_config::SettingSource::Local => coco_types::HookScope::Local,
            // Flag is treated as Local — closest to user's
            // explicit per-invocation override. TS lacks a
            // distinct flag scope; this matches its precedence.
            coco_config::SettingSource::Flag => coco_types::HookScope::Local,
            coco_config::SettingSource::Policy => coco_types::HookScope::Policy,
            coco_config::SettingSource::Plugin => coco_types::HookScope::Plugin,
        };
        match coco_hooks::load_hooks_from_config_with_policy(hooks_value, scope, policy) {
            Ok(definitions) => {
                for def in definitions {
                    registry.register_deduped(def);
                }
            }
            Err(e) => {
                warn!(error = %e, source = %source, "failed to load hooks from settings — source skipped");
            }
        }
    }
    let plugin_dirs = coco_plugins::get_plugin_dirs(config_home, cwd);
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
        coco_plugins::hook_bridge::register_plugin_hooks(registry, &plugin_refs);
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
    /// Slash-command registry — populated once at startup via
    /// `coco_commands::build_command_registry`. Both the typed
    /// `/foo` path (`process_submit_turn`) and the command-palette
    /// path (`UserCommand::ExecuteSkill`) dispatch through this.
    /// Wrapped in `RwLock` so `/reload-plugins` can rebuild and swap
    /// without restarting the session — consumers snapshot the inner
    /// `Arc<CommandRegistry>` once per dispatch via
    /// [`SessionRuntime::current_command_registry`].
    pub command_registry: Arc<RwLock<Arc<CommandRegistry>>>,
    /// Session-scoped `SkillManager` — same Arc that backed
    /// `command_registry`'s skill load, kept alive so the per-turn
    /// reminder pipeline (`SkillsSource`) reads the same catalog.
    pub skill_manager: Arc<coco_skills::SkillManager>,
    /// Where to look for markdown agent definitions. Threaded into the
    /// runtime's [`coco_subagent::AgentDefinitionStore`] so AgentTool's
    /// dynamic prompt (TS `prompt.ts:getPrompt`) sees the same set the
    /// SDK `initialize.agents` listing reports. Empty = no on-disk
    /// agents (built-ins only).
    pub agent_search_paths: coco_subagent::definition_store::AgentSearchPaths,
    /// Built-in catalog toggles. Defaults to [`coco_subagent::BuiltinAgentCatalog::interactive`]
    /// (CLI / TUI sessions); SDK noninteractive callers may pass
    /// [`coco_subagent::BuiltinAgentCatalog::sdk_noninteractive`] to
    /// disable the entire built-in roster.
    pub builtin_agent_catalog: coco_subagent::BuiltinAgentCatalog,
}

/// Construct a [`ThinkingLevel`] for an effort, threading the model's
/// declared `supported_thinking_levels` budget when one is registered.
/// Falls back to a budget-less level (`budget_tokens: None`) so the
/// inference layer's provider-specific conversion picks defaults.
///
/// Lookup is L0-only (`builtin_models_partial`) — same source the TUI
/// picker uses today. Users registering a model in
/// `~/.coco/models.json` without declaring `supported_thinking_levels`
/// get a budget-less wire entry, which is the same behaviour as before
/// this override layer landed.
fn thinking_level_for_effort_from(model_id: &str, effort: ReasoningEffort) -> ThinkingLevel {
    if let Some(level) = coco_config::builtin_models_partial()
        .get(model_id)
        .and_then(|info| info.supported_thinking_levels.as_ref())
        .and_then(|levels| levels.iter().find(|l| l.effort == effort))
    {
        return level.clone();
    }
    ThinkingLevel {
        effort,
        budget_tokens: None,
        options: std::collections::HashMap::new(),
    }
}

/// In-memory binding for a single [`ModelRole`] that overrides the
/// `RuntimeConfig.model_roles` entry for the lifetime of one session.
///
/// Populated by the TUI model picker (`UserCommand::SetModelRole` →
/// [`SessionRuntime::apply_role_override`]) and Ctrl+T thinking cycle
/// (`UserCommand::SetThinkingLevel` →
/// [`SessionRuntime::apply_role_effort`]). The picker carries an
/// explicit `effort`; Ctrl+T preserves the spec and only changes
/// `effort`.
#[derive(Debug, Clone)]
pub struct RoleOverride {
    /// `(provider, model_id, display_name, api)` for the role.
    pub spec: ModelSpec,
    /// User's explicit effort choice. `None` ⇒ engine reaches for the
    /// model's `default_thinking_level` (or provider default if the
    /// model doesn't declare one).
    pub effort: Option<ReasoningEffort>,
}

/// All per-session state shared by both runners. Construction at startup
/// is done once via [`SessionRuntime::build`]; per-turn engines are
/// assembled via [`SessionRuntime::build_engine`].
pub struct SessionRuntime {
    // ── immutable resources (never change after build) ─────────────────
    /// Live Main-role [`ApiClient`]. Wrapped in [`RwLock`] so
    /// [`Self::apply_role_override`] can hot-swap it to a freshly-built
    /// client when the TUI picker selects a new Main model — without
    /// restarting the session. Read via [`Self::main_client`], which
    /// clones the inner `Arc` under a brief read lock.
    ///
    /// In-flight turns are unaffected: each turn's [`QueryEngine`]
    /// captures a clone of the current `Arc<ApiClient>` at build time
    /// (`build_engine` / `build_engine_from_config`), so a mid-turn
    /// swap only takes effect on the **next** turn build.
    client: Arc<RwLock<Arc<ApiClient>>>,
    /// Main-role fallback chain. Read by [`Self::wire_engine`] to install
    /// `with_fallback_clients` on every per-turn engine. **Not** swapped
    /// when the Main client hot-swaps via [`Self::apply_role_override`]:
    /// fallbacks are a recovery mechanism tied to the user's configured
    /// fallback specs in `~/.coco/settings.json`, independent of an
    /// in-session Main picker selection.
    fallback_clients: Vec<Arc<ApiClient>>,
    /// Half-open recovery policy for the Main role. `None` ⇒ sticky
    /// fallback semantics. Read by [`Self::wire_engine`].
    recovery_policy: Option<FallbackRecoveryPolicy>,
    /// Tool registry shared by every engine instance. Read by
    /// [`Self::build_engine`] / [`Self::build_engine_from_config`].
    tools: Arc<ToolRegistry>,
    /// Slash-command registry. Read by
    /// [`crate::tui_runner::dispatch_slash_command`] to resolve every
    /// `/foo` typed by the user or selected from the command palette.
    /// Wrapped in `RwLock` so `/reload-plugins` can rebuild and swap
    /// without restarting the session — consumers snapshot the inner
    /// `Arc<CommandRegistry>` once per dispatch via
    /// [`Self::current_command_registry`] so a concurrent swap can't
    /// invalidate borrows.
    pub command_registry: Arc<RwLock<Arc<CommandRegistry>>>,
    /// Session-scoped skill catalog. Cloned into `ReminderSources`
    /// (`SkillsSource`) on every per-turn engine so the model receives
    /// the `skill_listing` reminder that gates on
    /// `skill_manager.is_empty()`.
    pub(crate) skill_manager: Arc<coco_skills::SkillManager>,
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

    /// Original CWD captured at session start. Frozen for the lifetime
    /// of this [`SessionRuntime`] — never moves even if the user
    /// `cd`'s away inside a Bash command. Used as the anchor for
    /// `reset_cwd_if_outside_project` (when bash drifts out of the
    /// allowed working directory set, we snap it back here) and for
    /// "Shell cwd was reset to …" stderr annotations. TS:
    /// `bootstrap/state.ts::originalCwd`.
    pub original_cwd: PathBuf,

    // ── mutable per-session state (changes on /clear or mid-session) ──
    /// Currently active CWD. Updated **across BashTool calls** so the
    /// model's `cd /tmp` in one turn survives into the next turn.
    /// TS parity: `bootstrap/state.ts::STATE.cwd` updated via
    /// `utils/Shell.ts::setCwd` after every `pwd -P >| <file>` read.
    /// Threaded into every `ToolUseContext` via the engine config so
    /// BashTool can read it as the spawn cwd and write back from
    /// `CommandResult.new_cwd`.
    pub current_cwd: Arc<RwLock<PathBuf>>,
    /// Session id; mutated by [`Self::clear_conversation`] (regen).
    session_id: Arc<RwLock<String>>,
    /// Engine config; mutated by [`Self::clear_conversation`] (session_id)
    /// and [`Self::update_engine_config`]. Read by every per-turn build.
    engine_config: Arc<RwLock<QueryEngineConfig>>,
    /// Synchronous snapshot for detached hook factories. Those
    /// factories run from async tasks but expose a sync `Fn()`, so they
    /// must not call Tokio `blocking_read()` on runtime worker threads.
    orchestration_session_id: Arc<std::sync::RwLock<String>>,
    orchestration_engine_config: Arc<std::sync::RwLock<QueryEngineConfig>>,
    /// Per-session in-memory model-role overrides. Populated by the TUI
    /// model picker (`UserCommand::SetModelRole`) and Ctrl+T thinking
    /// cycle (`UserCommand::SetThinkingLevel`). Layered ABOVE
    /// `runtime_config.model_roles` — [`Self::resolve_role`] checks
    /// overrides first, falls back to the runtime config map second.
    ///
    /// **Not persisted.** Model-role changes via the TUI are session-local;
    /// users who want a binding to survive across sessions edit
    /// `~/.coco.json::model_roles.<role>.primary` themselves.
    ///
    /// Cleared on `Drop` (i.e. session end) via the natural `Arc`
    /// lifecycle. `/clear` keeps overrides — the conversation reset is
    /// orthogonal to model-role bindings.
    role_overrides: Arc<RwLock<HashMap<ModelRole, RoleOverride>>>,
    pub file_read_state: Arc<RwLock<FileReadState>>,
    pub file_history: Option<Arc<RwLock<FileHistoryState>>>,
    pub app_state: Arc<RwLock<ToolAppState>>,
    /// Session-scoped Auto mode classifier state. Installed on every
    /// per-turn engine so `permission_mode = Auto` can auto-approve
    /// safe/read-only tools before falling back to interactive approval.
    auto_mode_state: Arc<coco_permissions::AutoModeState>,
    /// Denial history for Auto mode classifier decisions. Shared across
    /// per-turn engines and cleared when the session changes or compacts.
    denial_tracker: Arc<tokio::sync::Mutex<coco_permissions::DenialTracker>>,
    /// Session-memory extractor + on-disk cache. The same `Arc` as
    /// `memory_runtime.session_memory` when `Feature::AutoMemory` is
    /// on, otherwise `None`. Used by [`Self::wire_engine`] (engine
    /// reads `current_text`) and [`Self::start_new_session`] /
    /// [`Self::clear_conversation`] (session-id retarget + cache wipe).
    session_memory_service: Option<Arc<SessionMemoryService>>,
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
    /// LLM-driven hook handler — implements
    /// [`coco_hooks::HookLlmHandle`] for `Prompt` (full impl) and
    /// `Agent` (stub returning Cancelled — TS-aligned silent fallback)
    /// hook handlers. Threaded into every `OrchestrationContext` so
    /// settings hooks of `type: "prompt"` / `type: "agent"` actually
    /// reach an LLM instead of falling back to passthrough text.
    pub(crate) hook_llm_handle: Arc<dyn coco_hooks::HookLlmHandle>,
    /// Shared sync-hook-event buffer. SessionStart and UserPromptSubmit
    /// orchestration calls push `HookEvent`s here; the
    /// [`coco_hooks::reminder_source::CombinedHookEventsSource`]
    /// installed on every per-turn engine drains them into the
    /// reminder pipeline. Lifetime spans the whole session — same
    /// instance flows through `OrchestrationContext.sync_event_sink`
    /// and `QueryEngine::sync_hook_buffer`.
    pub(crate) sync_hook_buffer: coco_hooks::SyncHookEventBuffer,
    /// Async hook bookkeeping. Currently no production code path
    /// registers async hooks, but the slot is wired into the combined
    /// reminder source so when async hook execution lands it surfaces
    /// `async_hook_response` reminders without further plumbing.
    /// TS parity: `AsyncHookRegistry`.
    pub(crate) async_hook_registry: Arc<coco_hooks::async_registry::AsyncHookRegistry>,
    /// FileChanged hook watcher. Populated when the runtime's hook
    /// registry has any handlers for the `FileChanged` event;
    /// `None` otherwise. TS:
    /// `utils/hooks/fileChangedWatcher.ts`. Paths are registered
    /// lazily from `SessionStart` / `CwdChanged` hook output.
    pub(crate) file_changed_watcher:
        Arc<RwLock<Option<crate::file_changed_watcher::FileChangedHookWatcher>>>,
    /// Multi-turn agent transcript. Each turn snapshots, appends, and
    /// rewrites this on success. Wrapped in `MessageHistory` (the same
    /// type the engine loop uses internally) so TUI-initiated pushes
    /// can call `history_push_and_emit` directly without converting at
    /// the lock boundary.
    pub history: Arc<Mutex<MessageHistory>>,
    /// Shared session id of the `TranscriptFileHistorySink` (when
    /// file_history is enabled). `clear_conversation` writes the
    /// regenerated session id here so the sink targets the new
    /// session's jsonl on the next snapshot. `None` when
    /// file_checkpointing is disabled.
    file_history_sink_session_id: Option<Arc<std::sync::RwLock<String>>>,
    /// Shared per-role `ApiClient` cache. The same `Arc` is handed to
    /// every subsystem that needs role-aware dispatch (hook LLM, fork
    /// dispatcher, side query, …) so a given role resolves to one
    /// `ApiClient` instance with a single `CacheBreakDetector` regardless
    /// of caller. Required so per-role-configured users (e.g.
    /// `models.subagent = openai/gpt-5` while `models.main =
    /// anthropic/...`) actually route subagents through their
    /// configured provider instead of silently reusing Main's client.
    role_client_cache: Arc<coco_inference::RoleClientCache>,
    /// Agent-spawn handle used by `AgentTool` / coordinator-mode
    /// workers. Late-bound after `TaskRuntime` is attached because
    /// `SwarmAgentHandle` requires the canonical TaskManager-backed
    /// registry at construction.
    agent_handle: Arc<RwLock<Option<AgentHandleRef>>>,
    /// Post-turn fork dispatcher (D1/D2). Same late-bind pattern as
    /// `agent_handle`: built after `build()` returns the `Arc<Self>`
    /// (the dispatcher impl captures the runtime), and installed on
    /// every per-turn engine via `wire_engine`. `None` ⇒ post-turn
    /// forks degrade to no-op (`/btw` returns a hint message,
    /// `promptSuggestion` skips). Real impl lives in
    /// `app/cli/src/fork_dispatcher.rs`.
    fork_dispatcher: Arc<RwLock<Option<coco_query::forked_agent::ForkDispatcherRef>>>,
    /// Session-scoped abort token for the in-flight prompt-suggestion
    /// fork. TS parity: `services/PromptSuggestion/promptSuggestion.ts`
    /// module-level `currentAbortController` singleton. When a new
    /// suggestion fork starts, we cancel the previous one so users
    /// rapidly cycling `/clear` don't accumulate fork tasks burning
    /// tokens. `None` ⇒ no fork in flight.
    pub current_suggestion_abort:
        Arc<tokio::sync::Mutex<Option<tokio_util::sync::CancellationToken>>>,
    /// Background task runtime (TaskHandle implementation) — owns
    /// the `TaskManager` + per-task control state. Shared with
    /// `SwarmAgentHandle` so AgentTool's bg path registers spawns
    /// through the same store the engine's `Task*` tools read from.
    /// `None` resolves to `NoOpTaskHandle` semantics (the task tools
    /// surface a clean "no task runtime configured" error).
    task_runtime: Arc<RwLock<Option<Arc<crate::task_runtime::TaskRuntime>>>>,
    /// Durable task-list store shared by the leader, AgentTool children,
    /// and in-process teammates.
    task_list: Arc<RwLock<Option<coco_tool_runtime::TaskListHandleRef>>>,
    team_task_list_router: Arc<RwLock<Option<coco_tool_runtime::TeamTaskListRouterRef>>>,
    /// Per-agent transcript / metadata store for resume support.
    /// Late-bound so CLI bootstrap can construct the impl after
    /// `SessionRuntime::build` returns. `agent_handle_factory`
    /// installs it onto the SwarmAgentHandle when wiring agent-
    /// team support.
    agent_transcript_store: Arc<RwLock<Option<coco_tool_runtime::AgentTranscriptStoreRef>>>,
    /// Main-session transcript store. JSONL writes for the user /
    /// assistant / attachment / tool_result chain land here, keyed
    /// by the live session id (rotates on `/clear`). Cloned into
    /// every per-turn engine via [`Self::wire_engine`]. TS parity:
    /// `Project` from `utils/sessionStorage.ts`.
    transcript_store: Arc<TranscriptStore>,
    /// Cross-engine dedup set of message UUIDs already persisted to
    /// the JSONL transcript. Lives on the runtime (not the engine)
    /// so a fresh per-turn engine doesn't re-write history. Reset to
    /// empty by [`Self::clear_conversation`] when the session id
    /// regenerates.
    transcript_dedup: Arc<tokio::sync::Mutex<std::collections::HashSet<uuid::Uuid>>>,
    /// Cross-engine tool-result replacement state. QueryEngine is
    /// rebuilt per user message, so this runtime-owned state preserves
    /// Level 2 `seen_ids` / replacement strings across turns.
    tool_result_replacement_state:
        coco_tool_runtime::tool_result_storage::ContentReplacementStateRef,
    /// MCP handle installed on every per-turn engine via `wire_engine`.
    /// Late-bound so CLI bootstrap can construct the
    /// `McpManagerAdapter` (or any other McpHandle impl) after
    /// `SessionRuntime::build` returns. Without this the engine's
    /// `mcp_handle` slot stays `None` and AgentTool's prompt-time
    /// MCP filter degrades to fail-closed (hides MCP-required
    /// agents).
    mcp_handle: Arc<RwLock<Option<coco_tool_runtime::McpHandleRef>>>,
    /// Late-bind slot for the LSP handle. CLI / SDK installs a
    /// `LspManagerAdapter` here when `Feature::Lsp` is on and at
    /// least one language server is configured; `wire_engine` reads
    /// the slot at engine-build time and installs it via
    /// `with_lsp_handle`.
    lsp_handle: Arc<RwLock<Option<coco_tool_runtime::LspHandleRef>>>,
    /// Where the agent loader looks for markdown agents. Cached so
    /// `/agents reload` and the file-watcher reload paths can rebuild
    /// the snapshot without re-resolving the paths from scratch.
    agent_search_paths: coco_subagent::definition_store::AgentSearchPaths,
    /// Built-in agent toggles applied to every reload. Set at
    /// `SessionRuntime::build` and treated as immutable thereafter
    /// (toggling the roster mid-session would require a full restart).
    builtin_agent_catalog: coco_subagent::BuiltinAgentCatalog,
    /// Active per-session agent catalog snapshot. Installed on every
    /// per-turn engine via [`Self::wire_engine`] so `AgentTool::prompt`
    /// renders the dynamic agent listing (TS `prompt.ts:getPrompt`).
    /// Wrapped in `RwLock<Arc<...>>` so a future reload (file watcher
    /// or `/agents reload`) can swap the inner `Arc` without
    /// invalidating in-flight per-turn engines (each engine snapshots
    /// the inner Arc at wire time). `Arc<AgentCatalogSnapshot>` is
    /// cheap to clone.
    agent_catalog: Arc<RwLock<Arc<coco_subagent::AgentCatalogSnapshot>>>,
    /// SDK-supplied agent definitions to inject into every fresh
    /// `AgentDefinitionStore` build (initial load + every reload).
    /// Populated by the SDK `initialize` handler via
    /// [`Self::set_sdk_supplied_agents`] when the client pushes an
    /// `initialize.agents` JSON map. Stays alive across `session/start`
    /// → `session/archive` cycles so a single SDK connection's
    /// `initialize` payload survives multiple session boundaries.
    ///
    /// TS parity: `cli/print.ts:4382` calls
    /// `parseAgentsFromJson(_, 'flagSettings')` once and threads the
    /// result into the agent catalog for every subsequent reload.
    /// `loadAgentsDir.ts:296-393 getAgentDefinitionsWithOverrides`
    /// re-applies SDK agents on every reload (they're a regular
    /// `flagSettings` source).
    sdk_supplied_agents: Arc<RwLock<Vec<coco_types::AgentDefinition>>>,
    /// Session-scoped sandbox state. Built once at startup via
    /// [`build_sandbox_state`] and inherited by every per-turn engine
    /// (TUI), every SDK control message handler, and every fork
    /// dispatch — so all paths share the same `Arc<SandboxState>` and
    /// hot-reloads via `update_config` are seen everywhere.
    /// `None` when sandbox is disabled.
    sandbox_state: Option<Arc<coco_sandbox::SandboxState>>,
    /// Session-scoped attachment channel. Producers outside the per-turn
    /// engine (slash commands via the TUI, future swarm / skill / hook
    /// forwarders) emit typed silent `AttachmentMessage`s through
    /// [`Self::attachment_emitter`]; the engine drains the receiver at the
    /// head of every outer-loop turn via
    /// [`coco_query::QueryEngine::drain_attachment_inbox`]. Lives across
    /// engine rebuilds so cross-turn producers see a stable handle.
    session_attachment_tx: tokio::sync::mpsc::UnboundedSender<coco_messages::AttachmentMessage>,
    session_attachment_rx: Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<coco_messages::AttachmentMessage>>,
    >,
    /// Session-scoped mid-turn command queue. The Rust analog of TS
    /// `utils/messageQueueManager.ts` module-level singleton: producers
    /// (the TUI-while-busy bridge in `tui_runner`, future task /
    /// coordinator / hook forwarders) push `QueuedCommand`s here at any
    /// time, and the per-turn `QueryEngine` consumes them via
    /// [`Self::wire_engine`] which calls
    /// [`QueryEngine::with_command_queue`]. Internally `Arc`-backed so
    /// `Clone` is cheap — every engine instance shares the same backing
    /// storage with the runtime and any other holder.
    ///
    /// Teammate messages and task notifications also flow through this
    /// queue (with `QueueOrigin::Coordinator` /
    /// `QueueOrigin::TaskNotification`) — no separate `Inbox` type, TS
    /// parity with `getAgentPendingMessageAttachments` which surfaces
    /// coordinator messages as `queued_command` attachments.
    command_queue: CommandQueue,
    /// Concurrent-sessions PID registry guard. Wraps
    /// `<config_home>/sessions/{pid}.json`; the file is created at
    /// build time and removed when this field is dropped (i.e. when
    /// the last `Arc<SessionRuntime>` reference falls). `None` when
    /// the registration was skipped (subagent context per
    /// `COCO_AGENT_ID`) or the write failed (best-effort — we
    /// `tracing::warn` and proceed without a registry entry rather
    /// than block session startup). TS parity:
    /// `utils/concurrentSessions.ts::registerSession`.
    _pid_registry: Option<coco_session::SessionRegistry>,
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
            command_registry,
            skill_manager,
            agent_search_paths,
            builtin_agent_catalog,
        } = opts;

        let config_home = coco_config::global_config::config_home();
        let session_id = uuid::Uuid::new_v4().to_string();

        // Concurrent-sessions PID registry. Skipped for subagent
        // contexts (TS `getAgentId() != null`), and best-effort: a
        // write failure here is logged and ignored so a constrained
        // FS doesn't block session startup. TS parity:
        // `utils/concurrentSessions.ts::registerSession`.
        let pid_registry = {
            let agent_id_env = coco_config::env::var(coco_config::env::EnvKey::CocoAgentId).ok();
            match coco_session::SessionRegistry::register(
                &config_home,
                &session_id,
                &cwd,
                agent_id_env.as_deref(),
            ) {
                Ok(reg) => reg,
                Err(e) => {
                    warn!("concurrent-sessions register failed (non-fatal): {e}");
                    None
                }
            }
        };

        // FileReadState — @mention dedup + Read tool dedup.
        let file_read_state = Arc::new(RwLock::new(FileReadState::new()));

        // Per-project filesystem layout — one `Arc<ProjectPaths>` shared
        // by the memory runtime, the transcript enumerator, and any
        // future subsystem that needs the same canonical slug. Built
        // once via `crate::paths::project_paths` (canonical-git-root
        // + slug).
        let project_paths = crate::paths::project_paths(&cwd);

        // ── Auto-memory runtime ──
        //
        // Built once per session, gated on `Feature::AutoMemory`. The
        // runtime owns the three services (extract / dream / session
        // memory) plus the recall ranker state. We hand it the
        // resolved `MemoryConfig` (already merged with env overrides),
        // the shared `Arc<ProjectPaths>` (so the SM file lives at
        // `<projectDir>/<sid>/session-memory/summary.md`), and an
        // `AgentHandle` so the forked extraction / dream subagents
        // spawn against the same swarm runtime that user-facing
        // `Agent` tool spawns use.
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
            // Transcript root for dream's grep examples / searching-
            // past-context section. TS parity:
            // `getProjectDir(getOriginalCwd())` lives at
            // `<memory_base>/projects/<slug>/`.
            let transcript_root = project_paths.project_dir();
            // Wire the production tracing-backed telemetry emitter so
            // the ~17 MemoryEvent variants land in the global tracing
            // subscriber (installed by app/cli's tracing_init). Without
            // this every event silently no-ops via NoopEmitter.
            let memory_telemetry: Arc<dyn coco_memory::telemetry::MemoryTelemetryEmitter> =
                Arc::new(coco_memory::telemetry::TracingEmitter::new());
            // Whether auto-compact is active for this session — surfaced
            // by SessionMemoryInit so dashboards correlate SM activity
            // with the compact gate. `is_active()` honors both the user
            // toggle and the kill-switch envs (`COCO_COMPACT_DISABLE`,
            // `COCO_COMPACT_DISABLE_AUTO`), so a session bootstrapped
            // with compact off reports `auto_compact_enabled = false`.
            let auto_compact_enabled = runtime_config.compact.auto.is_active();
            let runtime = coco_memory::runtime::MemoryRuntimeBuilder::new(
                config_home.clone(),
                cwd.clone(),
                session_id.clone(),
                mem_cfg,
                agent,
            )
            .with_project_paths(project_paths.clone())
            .with_transcript_dir(transcript_root)
            .with_telemetry(memory_telemetry)
            .with_auto_compact_enabled(auto_compact_enabled)
            .build();
            info!(
                personal_dir = %runtime.personal_dir().display(),
                "auto-memory runtime initialized"
            );
            let runtime_arc = Arc::new(runtime);
            // Wire the session enumerator backed by `TranscriptStore`
            // so per-turn `tick_dream` can list real prior sessions.
            // TS parity (`autoDream.ts:155-165`):
            // `listSessionsTouchedSince(lastAt)` reads the project's
            // session store, filters by mtime > lastAt, drops the
            // current session. The closure here mirrors that
            // contract; it is invoked **only** after the time + scan
            // throttle gates pass inside `DreamService` so cost is
            // bounded.
            let enumerator_project_paths = project_paths.clone();
            let enumerator_session_id = session_id.clone();
            let enumerator_memory_dir = runtime_arc.personal_dir().to_path_buf();
            let enumerator: coco_memory::SessionEnumerator = Arc::new(move || {
                let store = coco_session::TranscriptStore::new(enumerator_project_paths.clone());
                let last_ms =
                    coco_memory::lock::last_consolidated_at(&enumerator_memory_dir).unwrap_or(0);
                match store.list_main_sessions() {
                    Ok(metas) => metas
                        .into_iter()
                        .filter(|m| m.session_id != enumerator_session_id)
                        .filter(|m| {
                            m.modified_at
                                .parse::<i64>()
                                .map(|t| t > last_ms)
                                .unwrap_or(false)
                        })
                        .map(|m| m.session_id)
                        .collect(),
                    Err(_) => Vec::new(),
                }
            });
            // install_* are one-shot in production (this is the only
            // call site per slot); swallow the duplicate-install Err so
            // a future double-install in tests doesn't blow up startup.
            let _ = runtime_arc.install_session_enumerator(enumerator);
            // NOTE: the tick_dream fire-and-forget is intentionally
            // deferred to AFTER `install_agent` below. Spawning it here
            // (before the real `SwarmAgentHandle` lands in the slot)
            // creates a multi-threaded race where the dream task can
            // read the NoOp handle, fail to spawn the consolidation
            // subagent, and emit a spurious `AutoDreamFailed` event.
            Some(runtime_arc)
        } else {
            // Feature gate off — emit MemdirDisabled so dashboards
            // can split sessions that never bootstrapped memory from
            // those that did. TS parity: `memdir.ts:492-505`'s
            // `tengu_memdir_disabled` fires from the equivalent gate
            // check. We emit directly here instead of through
            // `MemoryRuntime` because no runtime exists at this
            // point.
            tracing::info!(
                target: "coco_memory::telemetry",
                event_type = "tengu_memdir_disabled",
                reason = "feature_gate",
                "auto-memory feature gate off"
            );
            None
        };

        // The production swarm handle is late-bound after TaskRuntime is
        // attached, because LocalAgent task registration is a required
        // constructor dependency. Until then engines carry the explicit
        // no-op handle and `attach_agent_handle` replaces it everywhere.
        let swarm_agent_handle: coco_tool_runtime::AgentHandleRef =
            Arc::new(coco_tool_runtime::NoOpAgentHandle);

        // Now that the real `AgentHandle` exists, install it on the
        // memory runtime so forked extraction / dream agents reach
        // the same swarm runtime instead of the no-op fallback.
        // Install the SideQuery adapter too so the recall ranker
        // dispatches a real `ModelRole::Memory` query instead of
        // falling back to the recency heuristic.
        if let Some(runtime) = &memory_runtime {
            runtime.install_agent(swarm_agent_handle.clone());
            let side_query: coco_tool_runtime::SideQueryHandle =
                Arc::new(crate::side_query_impl::SideQueryAdapter::new(
                    client.clone(),
                    runtime_config.clone(),
                ));
            let _ = runtime.install_side_query(side_query);

            // Fire-and-forget auto-dream gate-check at session start.
            // Deferred here (vs. inside the runtime build above) so
            // the task observes the just-installed real
            // `SwarmAgentHandle` — multi-threaded schedulers were
            // racing the NoOp slot and emitting spurious
            // `AutoDreamFailed` events. TS parity: `initAutoDream`
            // schedules on session start; per-turn ticks via
            // `executeAutoDream` from stop hooks.
            let dream_clone = runtime.clone();
            tokio::spawn(async move {
                let now_ms = coco_memory::service::dream::DreamService::now_ms();
                let outcome = dream_clone.tick_dream(now_ms).await;
                tracing::debug!(?outcome, "auto-dream gate check at session start");
            });
        }

        // Session-memory handle threaded into the engine. Same `Arc`
        // the memory runtime holds when `Feature::AutoMemory` is on;
        // `None` otherwise (engine's SM-first compact path then falls
        // back to LLM summarization). Warm the on-disk cache here so
        // the first compact short-circuit doesn't have to read disk.
        let session_memory_service = memory_runtime.as_ref().map(|r| r.session_memory.clone());
        if let Some(svc) = &session_memory_service {
            svc.load_from_disk().await;
        }

        // Reap abandoned per-session SM dirs (left behind by every
        // prior `/clear`, which regenerates the session id). 30-day
        // retention mirrors the worktree GC cadence; mtime-only, fire-
        // and-forget so a wedged filesystem can't block startup.
        if memory_runtime.is_some() {
            let pdir = project_paths.project_dir();
            let sid = session_id.clone();
            tokio::spawn(async move {
                match coco_memory::service::session::cleanup_stale_session_memories(
                    &pdir,
                    &sid,
                    coco_memory::service::session::DEFAULT_SM_RETENTION,
                )
                .await
                {
                    Ok(n) if n > 0 => {
                        info!(
                            "reaped {n} orphan session-memory dirs under {}",
                            pdir.display()
                        );
                    }
                    Ok(_) => {}
                    Err(e) => warn!("session-memory cleanup failed: {e}"),
                }
            });
        }

        // FileHistoryState — backed by JSONL transcript when enabled.
        // Sink shares the session_id Arc with SessionRuntime so
        // /clear regen propagates immediately (no rebuild required).
        let (file_history, file_history_sink_session_id) =
            if runtime_config.settings.merged.file_checkpointing_enabled {
                let project_paths = crate::paths::project_paths(&cwd);
                let sink_id = Arc::new(std::sync::RwLock::new(session_id.clone()));
                let sink: Arc<dyn FileHistorySnapshotSink> = Arc::new(
                    TranscriptFileHistorySink::new(project_paths, sink_id.clone()),
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
        let auto_mode_state = Arc::new(coco_permissions::AutoModeState::new());
        auto_mode_state.set_active(permission_mode == coco_types::PermissionMode::Auto);
        auto_mode_state.set_cli_flag(permission_mode == coco_types::PermissionMode::Auto);
        let denial_tracker = Arc::new(tokio::sync::Mutex::new(
            coco_permissions::DenialTracker::new(),
        ));

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
            let registry = HookRegistry::new();
            populate_hook_registry(&registry, &runtime_config, &config_home, &cwd);
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

        // Bootstrap the sandbox runtime state from settings + permission
        // rules. The adapter mirrors TS `convertToSandboxRuntimeConfig`;
        // when sandbox isn't enabled or required dependencies are missing
        // the bootstrap returns `None` (degrade to unsandboxed) — unless
        // `sandbox.fail_if_unavailable` is set, in which case it returns
        // an error and we exit before the REPL starts.
        let sandbox_state = build_sandbox_state(&runtime_config, &cwd)?;

        // Session-scoped attachment channel. The engine drains the rx at
        // the head of each turn (drain_attachment_inbox), while producers
        // outside the per-turn engine (TUI slash commands, future swarm /
        // skill forwarders) push via the cloned tx — see
        // `Self::attachment_emitter`. One channel per session, threaded
        // into each per-turn engine via `wire_engine`.
        let (session_attachment_tx, session_attachment_rx) =
            tokio::sync::mpsc::unbounded_channel::<coco_messages::AttachmentMessage>();
        let session_attachment_rx = Arc::new(tokio::sync::Mutex::new(session_attachment_rx));

        // Bootstrap the per-source permission rule maps. Mirrors TS
        // `loadPermissionRules()`: parses every settings.json layer
        // (user/project/local/flag/policy) into typed
        // `PermissionRulesBySource` keyed by `PermissionRuleSource`.
        // Default-empty maps before this wiring meant `permissions.allow`
        // / `deny` / `ask` from settings.json were loaded but never
        // consulted at evaluation time.
        let (allow_rules, deny_rules, ask_rules) =
            crate::permission_rule_loader::typed_permission_rules(&runtime_config.settings);
        let permission_rule_source_roots =
            crate::permission_rule_loader::permission_rule_source_roots(
                &runtime_config.settings,
                &cwd,
            );

        // ── Session-scoped CWD state ──
        //
        // Frozen anchor + live tracker, mirroring TS's
        // `STATE.originalCwd` + `STATE.cwd`. The live tracker is
        // threaded through every `ToolUseContext` so BashTool can
        // read it as the spawn cwd and write back `new_cwd` after
        // each command — `cd /tmp` in turn N survives into turn N+1.
        let session_original_cwd = cwd.clone();
        let session_current_cwd = Arc::new(RwLock::new(cwd.clone()));

        // ── Session-scoped shell provider ──
        //
        // Build once at session start so `BashProvider` keeps the same
        // snapshot watch + session-env reader + `/env` store across all
        // BashTool invocations. TS parity: `bashProvider.ts:58-69` —
        // snapshot promise resolves once for the lifetime of the
        // shell provider singleton.
        let shell_provider: Option<Arc<dyn coco_shell::ShellProvider>> = {
            let mut shell = coco_shell::shell_from_config(&runtime_config.shell);
            let snap_cfg = coco_shell::SnapshotConfig::new(&config_home);
            if !runtime_config.shell.disable_snapshot {
                coco_shell::ShellSnapshot::start_snapshotting(
                    snap_cfg.clone(),
                    &session_id,
                    &mut shell,
                );
                // Sweep prior-run residue in the background — mtime-only,
                // no await needed on the hot path.
                let dir = snap_cfg.snapshot_dir.clone();
                let sid = session_id.clone();
                let retention = snap_cfg.retention;
                tokio::spawn(async move {
                    match coco_shell::cleanup_stale_snapshots(&dir, &sid, retention).await {
                        Ok(n) if n > 0 => {
                            info!("reaped {n} stale shell snapshots from {}", dir.display());
                        }
                        Ok(_) => {}
                        Err(e) => warn!("shell snapshot cleanup failed: {e}"),
                    }
                });
            }
            let session_env_reader = Some(Arc::new(coco_shell::SessionEnvReader::new(
                &config_home,
                &session_id,
            )));
            // `COCO_SHELL_PREFIX` is consumed here (BashProvider wraps the
            // assembled command). The same env var is also consumed by
            // `coco-hooks` for hook-command execution — they share the
            // value but apply it independently.
            let shell_prefix = std::env::var("COCO_SHELL_PREFIX").ok();
            let session_env_vars = coco_shell::SessionEnvVars::new();
            Some(Arc::new(coco_shell::BashProvider::new(
                shell,
                session_env_reader,
                session_env_vars,
                shell_prefix,
            )) as Arc<dyn coco_shell::ShellProvider>)
        };

        // Build the engine config — owns most settings drawn from
        // RuntimeConfig + CLI overrides.
        let engine_config = QueryEngineConfig {
            model_id,
            permission_mode,
            bypass_permissions_available,
            allow_rules,
            deny_rules,
            ask_rules,
            permission_rule_source_roots,
            context_window: 200_000,
            max_output_tokens: 16_384,
            max_turns: runtime_config.loop_config.max_turns.unwrap_or(30),
            max_tokens: cli
                .max_tokens
                .or_else(|| runtime_config.loop_config.max_tokens.map(i64::from)),
            prompt_cache: client
                .supports_prompt_cache()
                .then(|| coco_types::PromptCacheConfig {
                    mode: coco_types::PromptCacheMode::Auto,
                    ttl: coco_types::CacheTtl::OneHour,
                    scope: None,
                    requested_betas: Default::default(),
                    skip_cache_write: false,
                }),
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
            sandbox_state: sandbox_state.clone(),
            memory_config: runtime_config.memory.clone(),
            shell_config: runtime_config.shell.clone(),
            shell_provider,
            original_cwd: Some(session_original_cwd.clone()),
            session_cwd: Some(session_current_cwd.clone()),
            web_fetch_config: runtime_config.web_fetch.clone(),
            web_search_config: runtime_config.web_search.clone(),
            lsp_config: runtime_config.lsp.clone(),
            compact: runtime_config.compact.clone(),
            features: Arc::new(runtime_config.features.clone()),
            tool_overrides: runtime_config.tool_overrides.clone(),
            include_hook_events: cli.include_hook_events,
            ..Default::default()
        };

        let auto_title_enabled = runtime_config.settings.merged.session.auto_title;

        // Shared per-role `ApiClient` cache. Both
        // `SessionRuntime::client_for_role` and `QueryHookLlm` consume
        // this `Arc` — one cache means a given role's
        // `CacheBreakDetector` state stays continuous regardless of
        // which subsystem dispatched the call.
        let role_client_cache = Arc::new(coco_inference::RoleClientCache::new(
            runtime_config.clone(),
            client.clone(),
        ));

        // LLM-driven hook handler. `for_session` pre-resolves
        // `ModelRole::HookAgent` against the shared cache (spec-equality
        // shortcut reuses the Main `Arc` when the role is unconfigured),
        // so users who set `models.hook_agent` in settings.json get that
        // model for hook evaluations. Per-hook `model` overrides parse
        // as `ModelRole` and route through the same cache. TS parity:
        // `execPromptHook` / `execAgentHook` with `hook.model` override.
        let hook_llm_handle: Arc<dyn coco_hooks::HookLlmHandle> = Arc::new(
            coco_query::hook_llm::QueryHookLlm::for_session(role_client_cache.clone()).await,
        );
        // Main-session transcript store. Constructed once so the
        // file-history sink, the per-turn message append in
        // `engine_finalize_turn`, and the agent-transcript persistence
        // path all share the same `TranscriptStore` instance keyed at
        // `<memory_base>/projects/<slug>/` for this cwd.
        let transcript_store = Arc::new(TranscriptStore::new(crate::paths::project_paths(&cwd)));
        let transcript_dedup = Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::<
            uuid::Uuid,
        >::new()));
        let tool_result_replacement_state = Arc::new(tokio::sync::RwLock::new(
            coco_tool_runtime::tool_result_storage::ContentReplacementState::new(i64::MAX),
        ));

        // ── Agent definition catalog ──
        //
        // Build the per-session [`AgentDefinitionStore`] once at startup
        // so AgentTool's dynamic prompt (TS `prompt.ts:getPrompt`) sees
        // the same set the SDK `initialize.agents` listing returns. The
        // snapshot inspector wires `pending_snapshot_update` per
        // definition (TS `loadAgentsDir.ts:262-294`) so `/agents show`
        // can flag drift without each consumer re-running the
        // `check_agent_memory_snapshot` IO.
        //
        // Errors / missing dirs are non-fatal: the store keeps the
        // built-in roster and the per-turn engine reads the resulting
        // (mostly built-in) catalog. Snapshot is reload-able via
        // [`Self::reload_agent_catalog`]; this initial build lives on
        // the blocking pool because the markdown loader is sync IO.
        let auto_memory_enabled = runtime_config
            .features
            .enabled(coco_types::Feature::AutoMemory);
        // Initial agent-catalog load. SDK-supplied agents from
        // `initialize.agents` get injected here on session start —
        // they live on `SessionRuntime.sdk_supplied_agents` until
        // [`Self::set_sdk_supplied_agents`] is called by the SDK
        // `initialize` handler, which fires BEFORE `session/start`.
        // For pure TUI / SDK-less paths the Vec is empty.
        let initial_agent_snapshot = {
            let catalog = builtin_agent_catalog;
            let paths = agent_search_paths.clone();
            let cwd_for_inspector = cwd.clone();
            let home_for_inspector =
                dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
            // SDK-supplied agents are an empty Vec at this point — the
            // SessionRuntime is being constructed for the FIRST time;
            // `set_sdk_supplied_agents` hasn't been called yet. The
            // reload path picks them up once they're stashed.
            tokio::task::spawn_blocking(move || {
                let mut store = coco_subagent::AgentDefinitionStore::new(catalog, paths);
                store.set_snapshot_inspector(Some(
                    coco_memory::agent_memory_snapshot::build_pending_inspector(
                        cwd_for_inspector,
                        home_for_inspector,
                    ),
                ));
                // TS parity: `loadAgentsDir.ts:455-467` auto-adds
                // `Read`/`Edit`/`Write` to non-wildcard agent
                // tool-lists when AutoMemory is on AND the agent
                // declares a `memory` scope. Forward the live
                // feature gate so the catalog the engine sees
                // includes the injected tools.
                store.set_auto_memory_enabled(auto_memory_enabled);
                store.load();
                store.snapshot()
            })
            .await
            .unwrap_or_else(|_| {
                Arc::new(coco_subagent::AgentCatalogSnapshot::new(
                    std::collections::BTreeMap::new(),
                    Vec::new(),
                ))
            })
        };
        let agent_catalog = Arc::new(RwLock::new(initial_agent_snapshot));

        let orchestration_session_id = Arc::new(std::sync::RwLock::new(session_id.clone()));
        let orchestration_engine_config = Arc::new(std::sync::RwLock::new(engine_config.clone()));

        Ok(Arc::new(Self {
            original_cwd: session_original_cwd,
            current_cwd: session_current_cwd,
            client: Arc::new(RwLock::new(client)),
            fallback_clients,
            recovery_policy,
            tools,
            command_registry,
            skill_manager,
            config_home,
            runtime_config,
            session_manager,
            fast_model_spec,
            auto_title_enabled,
            mailbox,
            permission_bridge,
            cancel: CancellationToken::new(),
            session_id: Arc::new(RwLock::new(session_id.clone())),
            engine_config: Arc::new(RwLock::new(engine_config)),
            orchestration_session_id,
            orchestration_engine_config,
            role_overrides: Arc::new(RwLock::new(HashMap::new())),
            sandbox_state,
            file_read_state,
            file_history,
            app_state,
            auto_mode_state,
            denial_tracker,
            session_memory_service,
            memory_runtime,
            swarm_agent_handle,
            hook_registry,
            hook_llm_handle,
            sync_hook_buffer: coco_hooks::SyncHookEventBuffer::new(),
            async_hook_registry: Arc::new(coco_hooks::async_registry::AsyncHookRegistry::new()),
            file_changed_watcher: Arc::new(RwLock::new(None)),
            history: Arc::new(Mutex::new({
                let mut h = MessageHistory::new();
                // Stamp F9 envelope onto history so every history_sync
                // emit carries session_id automatically. agent_id is
                // None for the main session; subagents stamp their own
                // via a separate construction site in `engine_session`.
                h.set_envelope(session_id, None);
                h
            })),
            file_history_sink_session_id,
            role_client_cache,
            // Late-bound — `attach_agent_handle()` installs after the
            // Arc<SessionRuntime> is constructed so the
            // QueryEngineAdapter factory can close over Arc<Self>.
            agent_handle: Arc::new(RwLock::new(None)),
            fork_dispatcher: Arc::new(RwLock::new(None)),
            current_suggestion_abort: Arc::new(tokio::sync::Mutex::new(None)),
            task_runtime: Arc::new(RwLock::new(None)),
            task_list: Arc::new(RwLock::new(None)),
            team_task_list_router: Arc::new(RwLock::new(None)),
            agent_transcript_store: Arc::new(RwLock::new(None)),
            mcp_handle: Arc::new(RwLock::new(None)),
            lsp_handle: Arc::new(RwLock::new(None)),
            agent_search_paths,
            builtin_agent_catalog,
            agent_catalog,
            sdk_supplied_agents: Arc::new(RwLock::new(Vec::new())),
            session_attachment_tx,
            session_attachment_rx,
            transcript_store,
            transcript_dedup,
            tool_result_replacement_state,
            command_queue: CommandQueue::new(),
            _pid_registry: pid_registry,
        }))
    }

    /// Re-scan the configured agent search paths and replace the
    /// in-memory catalog snapshot. Subsequent per-turn engines built
    /// via [`Self::wire_engine`] pick up the new snapshot; engines
    /// already in flight keep the snapshot they captured at wire time.
    ///
    /// Triggered by `/agents reload`, `/reload-plugins`, and the
    /// future agent-dir file watcher. TS parity:
    /// `loadAgentsDir.ts::reloadAgents`.
    pub async fn reload_agent_catalog(&self) {
        let catalog = self.builtin_agent_catalog;
        let paths = self.agent_search_paths.clone();
        let cwd = std::env::current_dir().unwrap_or_default();
        let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
        let auto_memory_enabled = self
            .runtime_config
            .features
            .enabled(coco_types::Feature::AutoMemory);
        // Clone the SDK-supplied agents Vec into the worker. After
        // `set_sdk_supplied_agents` populates the slot, every reload
        // picks up the same set as additional FlagSettings entries.
        // The Vec lives across `session/start` → `session/archive`
        // cycles so a single SDK connection's `initialize` payload
        // survives the whole connection lifetime.
        let sdk_agents = self.sdk_supplied_agents.read().await.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            let mut store = coco_subagent::AgentDefinitionStore::new(catalog, paths);
            store.set_snapshot_inspector(Some(
                coco_memory::agent_memory_snapshot::build_pending_inspector(cwd, home),
            ));
            store.set_auto_memory_enabled(auto_memory_enabled);
            store.load();
            // Inject SDK-pushed agents AFTER on-disk load so they
            // participate in source-precedence resolution (FlagSettings
            // > ProjectSettings > UserSettings > Plugin > BuiltIn).
            // The store re-applies precedence on each `insert_definition`,
            // so an SDK agent with the same `agent_type` as a built-in
            // overrides the built-in — same as TS.
            for def in sdk_agents {
                store.insert_definition(def);
            }
            store.snapshot()
        })
        .await
        .ok();
        if let Some(snapshot) = snapshot {
            *self.agent_catalog.write().await = snapshot;
        }
    }

    /// Replace the set of SDK-supplied agent definitions used by every
    /// future catalog (re)load. Called by the SDK `initialize` handler
    /// when the client pushes `initialize.agents`.
    ///
    /// Triggers an immediate `reload_agent_catalog()` so the new agents
    /// land in the active snapshot before the next `turn/start` (the
    /// engine snapshots the catalog when wiring per-turn).
    ///
    /// TS parity: `cli/print.ts:4382` parses + injects, then the
    /// reload pipeline picks them up — coco-rs combines those into
    /// one call.
    pub async fn set_sdk_supplied_agents(&self, agents: Vec<coco_types::AgentDefinition>) {
        let count = agents.len();
        {
            let mut slot = self.sdk_supplied_agents.write().await;
            *slot = agents;
        }
        self.reload_agent_catalog().await;
        tracing::info!(
            target: "coco::session_runtime",
            count,
            "SDK-supplied agents applied; agent catalog reloaded"
        );
    }

    /// Cheap pointer-clone of the active catalog snapshot. The returned
    /// `Arc` is stable for the lifetime of the caller — a concurrent
    /// reload swaps the inner `Arc` but doesn't invalidate handles
    /// previously taken.
    pub async fn current_agent_catalog(&self) -> Arc<coco_subagent::AgentCatalogSnapshot> {
        self.agent_catalog.read().await.clone()
    }

    /// Session-scoped attachment emitter for producers outside the
    /// per-turn engine (TUI slash commands, swarm forwarders, …).
    ///
    /// Each `emit()` enqueues a typed `AttachmentMessage` (typically
    /// silent-* variants) onto the session channel. The engine drains
    /// at the head of each outer-loop turn via
    /// [`coco_query::QueryEngine::drain_attachment_inbox`] so producers
    /// don't need access to `MessageHistory`.
    pub fn attachment_emitter(&self) -> coco_messages::AttachmentEmitter {
        coco_messages::AttachmentEmitter::new(self.session_attachment_tx.clone())
    }

    /// The tool registry shared by every engine instance.
    ///
    /// Callers that need to register or deregister tools at runtime (e.g.
    /// the SDK MCP lifecycle handlers) use this to mutate the registry
    /// via its interior-mutability API.
    pub fn tools(&self) -> &Arc<ToolRegistry> {
        &self.tools
    }

    /// Session-scoped sandbox state. Cheap-clone via `Arc`; consumers
    /// (fork dispatch, SDK handler) inherit the same instance so
    /// `SandboxState::update_config` hot-reloads propagate everywhere.
    pub fn sandbox_state(&self) -> Option<Arc<coco_sandbox::SandboxState>> {
        self.sandbox_state.clone()
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

    /// Install or replace the late-bound LSP handle. Same semantics as
    /// [`Self::attach_mcp_handle`] — slot is read at every
    /// `wire_engine` call so per-turn engines pick up swaps.
    pub async fn attach_lsp_handle(&self, handle: coco_tool_runtime::LspHandleRef) {
        let mut slot = self.lsp_handle.write().await;
        *slot = Some(handle);
    }

    /// Snapshot the installed LSP handle. `None` ⇒ no handle wired —
    /// `wire_engine` falls back to `NoOpLspHandle` and `LspTool` hides
    /// from the model.
    pub async fn current_lsp_handle(&self) -> Option<coco_tool_runtime::LspHandleRef> {
        self.lsp_handle.read().await.clone()
    }

    /// Snapshot the current session id (cheap clone of the inner String).
    pub async fn current_session_id(&self) -> String {
        self.session_id.read().await.clone()
    }

    /// Seed the transcript dedup set with uuids that are already
    /// persisted on disk. Called on resume / fork so the first
    /// post-load turn doesn't re-write the loaded messages.
    pub async fn seed_transcript_dedup<I>(&self, uuids: I)
    where
        I: IntoIterator<Item = uuid::Uuid>,
    {
        let mut g = self.transcript_dedup.lock().await;
        g.extend(uuids);
    }

    /// Reconstruct Level 2 tool-result replacement state from the
    /// restored messages plus transcript content-replacement records.
    /// Called on resume/fork before the first resumed turn.
    pub async fn seed_tool_result_replacement_state(&self, messages: &[Message], session_id: &str) {
        let records = self
            .transcript_store
            .load_content_replacements(session_id)
            .unwrap_or_default();
        let mut next =
            coco_tool_runtime::tool_result_storage::ContentReplacementState::new(i64::MAX);
        for msg in messages {
            if let Message::ToolResult(tr) = msg {
                next.seen_ids.insert(tr.tool_use_id.clone());
            }
        }
        for record in records {
            next.seen_ids.insert(record.tool_use_id().to_string());
            next.replacements.insert(
                record.tool_use_id().to_string(),
                record.replacement().to_string(),
            );
        }
        *self.tool_result_replacement_state.write().await = next;
    }

    /// Borrow the optional `MemoryRuntime`. `None` when
    /// `Feature::AutoMemory` is off. Callers (e.g. the slash dispatcher's
    /// `/dream` and `/summary` triggers) clone the inner `Arc`.
    pub fn memory_runtime(&self) -> Option<&Arc<coco_memory::MemoryRuntime>> {
        self.memory_runtime.as_ref()
    }

    /// Snapshot the current Main-role [`ApiClient`]. Cloning is cheap
    /// (single `Arc` increment under a brief read lock). Callers
    /// retain the snapshot for the duration of one operation — a
    /// concurrent [`Self::apply_role_override`] for Main may swap the
    /// inner `Arc`, but a held snapshot keeps the old client alive
    /// until its last reference drops, so in-flight turns finish
    /// against their captured client without interruption.
    pub async fn main_client(&self) -> Arc<ApiClient> {
        self.client.read().await.clone()
    }

    /// Resolve an `ApiClient` for the given `ModelRole`. Consults the
    /// in-memory [`Self::role_overrides`] first; falls back to the
    /// shared [`coco_inference::RoleClientCache`] (which reads
    /// `runtime_config.model_roles`) on miss.
    ///
    /// Why this exists: the previous design assumed every model call
    /// went through `runtime.client` (= Main). Multi-provider configs
    /// like `models.subagent = openai/gpt-5` would silently reuse
    /// Main's client, defeating the user's per-role routing. Layered
    /// overrides extend that path with session-local TUI picker
    /// selections without touching settings.json.
    ///
    /// ## Main role
    ///
    /// For `role == Main` this always returns the live main client
    /// (see [`Self::main_client`]). When an override is installed via
    /// [`Self::apply_role_override`], the swap happens inside that
    /// method so the next call here sees the new client — there's
    /// no second build-and-swap on the read path.
    ///
    /// ## Known gap: unconfigured non-Main role fallback
    ///
    /// When a non-Main role like Plan has no `model_roles.<role>`
    /// entry, `resolve_model_roles` plants Main's spec as its
    /// fallback. The shared [`coco_inference::RoleClientCache`]
    /// captures the Main client at session bootstrap and continues to
    /// return that snapshot for the spec-equality fallback path —
    /// even after a Main hot-swap. Most users configure each role
    /// explicitly, so this affects only the unconfigured-fallback
    /// case. Tracked as a follow-up to plumb the live main handle
    /// into `RoleClientCache`.
    pub async fn client_for_role(&self, role: ModelRole) -> anyhow::Result<Arc<ApiClient>> {
        if role == ModelRole::Main {
            return Ok(self.main_client().await);
        }
        let override_spec = {
            let overrides = self.role_overrides.read().await;
            overrides.get(&role).map(|ov| ov.spec.clone())
        };
        if let Some(spec) = override_spec {
            let retry: coco_inference::RetryConfig = self.runtime_config.api.retry.clone().into();
            return coco_inference::model_factory::build_api_client(
                &self.runtime_config,
                &spec,
                retry,
            )
            .map_err(anyhow::Error::from);
        }
        self.role_client_cache
            .resolve(role)
            .await
            .map_err(anyhow::Error::from)
    }

    /// Resolve a role to `(spec, effort)`, layering overrides above
    /// `runtime_config.model_roles`. Returns `None` only when the role
    /// is not configured anywhere (model picker / engine consumers
    /// already guard on this via the Main fallback chain).
    ///
    /// Used by [`Self::current_engine_config`] to project the active
    /// Main effort onto `QueryEngineConfig.thinking_level`.
    pub async fn resolve_role(&self, role: ModelRole) -> Option<RoleOverride> {
        {
            let overrides = self.role_overrides.read().await;
            if let Some(ov) = overrides.get(&role) {
                return Some(ov.clone());
            }
        }
        self.runtime_config
            .model_roles
            .get(role)
            .map(|spec| RoleOverride {
                spec: spec.clone(),
                effort: None,
            })
    }

    /// Install (or replace) an in-memory override for `role`. The
    /// override layers above `runtime_config.model_roles` and is NOT
    /// persisted to `~/.coco.json` — re-bind on every session via the
    /// picker, or edit settings to make the change durable.
    ///
    /// For `role == Main` this also rewrites
    /// `engine_config.{model_id, thinking_level}` AND hot-swaps the
    /// runtime's live Main [`ApiClient`] (see [`Self::main_client`])
    /// so the next turn's API calls hit the picked provider/model,
    /// not the bootstrap-resolved one. In-flight turns are unaffected
    /// — they captured the old `Arc<ApiClient>` at engine-build time.
    ///
    /// Returns `Err` when the new Main spec can't be built into an
    /// `ApiClient` (e.g. provider not registered, model factory
    /// error). In that case the override IS NOT stored — the picker's
    /// optimistic mirror should revert and a toast should surface the
    /// failure. Non-Main role builds happen lazily in
    /// [`Self::client_for_role`], so non-Main overrides always
    /// succeed at install time.
    pub async fn apply_role_override(
        &self,
        role: ModelRole,
        ov: RoleOverride,
    ) -> anyhow::Result<()> {
        let effort = ov.effort;
        let model_id = ov.spec.model_id.clone();

        if role == ModelRole::Main {
            // Build the replacement client BEFORE mutating any state.
            // Fail-fast: if the spec is invalid the override never
            // lands, so the picker / status bar stay coherent with
            // the still-bootstrap-resolved Main client.
            let retry: coco_inference::RetryConfig = self.runtime_config.api.retry.clone().into();
            let new_client = coco_inference::model_factory::build_api_client(
                &self.runtime_config,
                &ov.spec,
                retry,
            )
            .map_err(anyhow::Error::from)?;
            // Store the override first so concurrent readers (e.g. a
            // turn finishing at the same instant) see the new spec
            // when they consult `role_overrides`.
            {
                let mut overrides = self.role_overrides.write().await;
                overrides.insert(role, ov);
            }
            // Engine config: project new model_id + effort budget.
            // The Arc<ApiClient> swap below makes the actual wire
            // call route through the new model; this projection makes
            // every config consumer (`tool_context`, finalize hooks,
            // SDK params) see the new identity too.
            self.update_engine_config(move |cfg| {
                cfg.model_id = model_id;
                cfg.thinking_level =
                    effort.map(|e| thinking_level_for_effort_from(&cfg.model_id, e));
            })
            .await;
            // Atomic-ish swap of the live Main client. Concurrent
            // turn-build readers either see the old (snapshot via
            // `main_client()` before the lock acquires) or the new
            // — never a torn state.
            {
                let mut g = self.client.write().await;
                *g = new_client;
            }
            return Ok(());
        }

        // Non-Main: storage-only. `client_for_role` builds the
        // override's client lazily on demand.
        let mut overrides = self.role_overrides.write().await;
        overrides.insert(role, ov);
        Ok(())
    }

    /// Update only the `effort` on an existing role override, preserving
    /// the spec. The Main role's `engine_config.thinking_level` is
    /// rewritten so the next turn picks up the change. **No client
    /// rebuild** — effort lives at the call-options layer, so the
    /// current Main `Arc<ApiClient>` keeps applying.
    ///
    /// When the role has no prior override, the current
    /// `runtime_config.model_roles` spec is captured and stored
    /// alongside the new effort so subsequent reads see a consistent
    /// `RoleOverride`.
    pub async fn apply_role_effort(&self, role: ModelRole, effort: Option<ReasoningEffort>) {
        let spec_for_seed = self.runtime_config.model_roles.get(role).cloned();
        let mut overrides = self.role_overrides.write().await;
        match overrides.get_mut(&role) {
            Some(existing) => existing.effort = effort,
            None => {
                if let Some(spec) = spec_for_seed {
                    overrides.insert(role, RoleOverride { spec, effort });
                }
            }
        }
        drop(overrides);
        if role == ModelRole::Main {
            self.update_engine_config(|cfg| {
                cfg.thinking_level =
                    effort.map(|e| thinking_level_for_effort_from(&cfg.model_id, e));
            })
            .await;
        }
    }

    /// Select the `ApiClient` to use for the *current turn* when
    /// `permission_mode == Plan`. Returns `None` when no swap is needed
    /// — callers stay on the engine's default Main client.
    ///
    /// TS parity behaviour: `getRuntimeMainLoopModel`
    /// (utils/model/model.ts:145-167). TS encodes the plan-mode swap
    /// via the `opusplan`/`haiku` aliases on the user's main model;
    /// coco-rs is multi-LLM and instead extends the `ModelRole::Plan`
    /// slot (previously subagent-only) to also drive the main-session
    /// model choice when in plan mode. Users opt in by setting
    /// `models.plan = <provider>/<model_id>` in their settings.json;
    /// without a Plan slot, the fallback chain (`runtime.rs:507`)
    /// returns the Main client and this returns `Some(main)` — same
    /// observable behaviour as the pre-change path.
    ///
    /// `exceeds_threshold` is the live "most recent assistant message
    /// context > N" flag computed at the engine turn entry. TS bypasses
    /// the swap when this is true to avoid truncation; the Rust
    /// counterpart honours the same intent — returning `None` here is
    /// equivalent to TS returning `mainLoopModel` unchanged.
    pub async fn resolve_plan_mode_client(
        &self,
        permission_mode: PermissionMode,
        exceeds_threshold: bool,
    ) -> Option<Arc<ApiClient>> {
        if permission_mode != PermissionMode::Plan {
            return None;
        }
        if exceeds_threshold {
            return None;
        }
        self.client_for_role(ModelRole::Plan).await.ok()
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
    ///
    /// The Main client is snapshotted via [`Self::main_client`] so a
    /// hot-swap that landed between turns is picked up here — each
    /// per-turn engine captures the current `Arc<ApiClient>` and
    /// keeps it for the duration of the turn.
    pub async fn build_engine(&self, cancel: CancellationToken) -> QueryEngine {
        let engine_config = self.current_engine_config().await;
        let engine = QueryEngine::new(
            engine_config,
            self.main_client().await,
            self.tools.clone(),
            cancel,
            Some(self.hook_registry.clone()),
        );
        self.wire_engine(engine, None).await
    }

    /// Public accessor for the hook registry. Same `Arc` as the one
    /// installed on every per-turn engine; safe to clone.
    pub fn hook_registry(&self) -> Arc<HookRegistry> {
        self.hook_registry.clone()
    }

    /// Public accessor for the session-scoped [`coco_skills::SkillManager`].
    /// Same `Arc` that backed the command-registry build and the
    /// reminder pipeline — safe to clone (cheap ref-count bump).
    /// Used by binary-entry wiring (e.g. `mcp_handle_adapter`) that
    /// sits outside the crate's `pub(crate)` field-access scope.
    pub fn skill_manager(&self) -> Arc<coco_skills::SkillManager> {
        self.skill_manager.clone()
    }

    /// Session-scoped command queue handle. Producers outside the
    /// per-turn engine — the TUI bridge in `tui_runner` (user typing
    /// while busy), future task-completion / coordinator / hook
    /// forwarders — call `enqueue` on this handle to inject mid-turn
    /// steering messages. Returned by reference; callers `.clone()` if
    /// they need an owned `Arc`-backed handle.
    ///
    /// Teammate messages and task notifications use the same queue
    /// with `QueueOrigin::Coordinator` / `QueueOrigin::TaskNotification`
    /// — TS parity with `getAgentPendingMessageAttachments`.
    ///
    /// TS parity: `utils/messageQueueManager.ts::enqueue` (exported as a
    /// free function reading the module-level singleton).
    pub fn command_queue(&self) -> &CommandQueue {
        &self.command_queue
    }

    /// Build a closure that materialises an
    /// [`coco_hooks::orchestration::OrchestrationContext`] tied to the
    /// current session's identity / cwd / disable flags.
    ///
    /// Used by detached hook firings (e.g. the `Elicitation` /
    /// `ElicitationResult` wrapper around `SendElicitation`, the
    /// FileChanged file watcher) that need a context built from inside
    /// a sync closure. Each call reads the synchronous snapshot mirrors
    /// kept up to date by session/config mutations, avoiding Tokio
    /// `blocking_read()` on runtime worker threads.
    pub fn orchestration_ctx_factory(
        self: &Arc<Self>,
    ) -> Arc<dyn Fn() -> coco_hooks::orchestration::OrchestrationContext + Send + Sync> {
        let runtime = self.clone();
        Arc::new(move || {
            let cfg = clone_std_rwlock(&runtime.orchestration_engine_config);
            let session_id = clone_std_rwlock(&runtime.orchestration_session_id);
            coco_hooks::orchestration::OrchestrationContext {
                session_id,
                cwd: std::env::current_dir().unwrap_or_default(),
                project_dir: cfg.project_dir.clone(),
                permission_mode: None,
                transcript_path: None,
                agent_id: None,
                agent_type: None,
                cancel: runtime.cancel.clone(),
                disable_all_hooks: cfg.disable_all_hooks,
                allow_managed_hooks_only: cfg.allow_managed_hooks_only,
                attachment_emitter: coco_messages::AttachmentEmitter::noop(),
                sync_event_sink: None,
                http_url_allowlist: None,
                http_env_var_policy: None,
                async_registry: Some(runtime.async_hook_registry.clone()),
                llm_handle: Some(runtime.hook_llm_handle.clone()),
                workspace_trust_accepted: None,
            }
        })
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
            self.main_client().await,
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
        // Pre-resolve the `ModelRole::Plan` client so the engine can
        // swap it in for `permission_mode == Plan` turns. We use the
        // same `client_for_role` cache as subagent routing — when
        // `models.plan` is unconfigured the fallback chain at
        // `runtime.rs:507` returns the Main spec, so the call returns
        // an `ApiClient` equivalent to Main and the swap becomes a
        // no-op (same observable behaviour as not installing this).
        //
        // Build failures are non-fatal — log and skip; the session
        // still works, plan mode just doesn't get a swap.
        let plan_client = match self.client_for_role(ModelRole::Plan).await {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::debug!(
                    error = %e,
                    "client_for_role(Plan) failed during wire_engine; plan-mode swap disabled this session"
                );
                None
            }
        };
        engine = engine.with_plan_role_client(plan_client);
        engine = engine.with_file_read_state(self.file_read_state.clone());
        engine = engine.with_app_state(app_state.clone());
        let auto_active = app_state
            .read()
            .await
            .permission_mode
            .is_some_and(|mode| mode == coco_types::PermissionMode::Auto);
        self.auto_mode_state.set_active(auto_active);
        engine = engine.with_auto_mode(
            self.auto_mode_state.clone(),
            self.denial_tracker.clone(),
            coco_permissions::AutoModeRules::default(),
        );
        // Skill-emitted `permission_updates` now flow through the
        // engine's own per-engine `EngineLiveRulesHandle`
        // (auto-installed by `QueryEngine::new`) which writes into
        // `QueryEngine.live_command_rules` — a fresh Arc per engine
        // = per user message. No session-level handle install: that
        // would leak rules across user messages. TS parity: `query()`
        // closure-captures `appState.alwaysAllowRules.command` and
        // drops it on return. See `engine_live_rules` for the
        // lifecycle invariant.
        // Session-scoped steering primitive. Without this, a fresh
        // `CommandQueue::new()` is constructed in `QueryEngine::new` and
        // dies with the per-turn engine, so any producer (TUI bridge,
        // future task / coordinator forwarders) enqueueing on
        // `runtime.command_queue()` would land on an instance the
        // running engine cannot see.
        engine = engine.with_command_queue(self.command_queue.clone());
        // Same lifetime argument as `with_command_queue`: the attachment
        // channel must live across engine rebuilds so cross-turn
        // producers (TUI slash commands, future swarm forwarders) see a
        // stable handle. The engine's own per-instance attachment
        // channel is replaced by the session-scoped one.
        engine = engine.with_attachment_channel(
            self.session_attachment_tx.clone(),
            self.session_attachment_rx.clone(),
        );
        if let Some(svc) = &self.session_memory_service {
            let sm_text_now = svc.current_text().await;
            engine = engine.with_session_memory_text(sm_text_now);
            engine = engine.with_session_memory_service(svc.clone());
        }
        // Install the real swarm-backed AgentHandle so AgentTool /
        // SendMessageTool / TeamCreateTool reach the swarm runtime
        // on every engine instance.
        engine = engine.with_agent_handle(self.swarm_agent_handle.clone());
        // Install the per-engine sync-hook-event buffer so the
        // `OrchestrationContext.sync_event_sink` constructed from this
        // engine's `orchestration_ctx()` writes into the same buffer
        // that the reminder source below drains.
        engine = engine.with_sync_hook_buffer(self.sync_hook_buffer.clone());
        // Same wiring for async hooks: the engine's `orchestration_ctx`
        // populates `async_registry` so engine-fired async hooks
        // (PreToolUse / PostToolUse / Stop / SubagentStop with
        // `is_async: true`) deliver via `CombinedHookEventsSource`.
        engine = engine.with_async_hook_registry(self.async_hook_registry.clone());
        // Same wiring for the LLM-driven hook handler so the engine's
        // `orchestration_ctx` carries it on every fired event — Prompt
        // / Agent settings hooks reach the LLM via `QueryHookLlm`.
        engine = engine.with_hook_llm_handle(self.hook_llm_handle.clone());
        // Wire the shared `RoleClientCache` so the engine's
        // `finalize_turn_post_tools` can resolve `ModelRole::Fast` for
        // the post-tool-batch summary fork (TS `generateToolUseSummary`).
        // Same cache instance the hook_llm handle uses — keeps
        // `CacheBreakDetector` state continuous across Fast-role calls
        // regardless of caller.
        engine = engine.with_role_client_cache(self.role_client_cache.clone());
        engine =
            engine.with_session_start_hook_side_effect_sink(Arc::new(QuerySessionStartHookSink {
                file_watch: self.file_watch_registration_context(),
            }));
        if let Some(runtime) = &self.memory_runtime {
            engine = engine.with_memory_runtime(runtime.clone());
        }
        // Reminder sources — populated unconditionally so non-memory
        // sessions still get hook + skill reminders. Each slot is
        // optional and silently skips if its data is empty (TS parity:
        // `getAttachments` returns `[]` when the underlying source
        // has nothing to surface).
        let sources = coco_system_reminder::ReminderSources {
            // Combined hook source: async-hook registry drains first,
            // then the sync-hook buffer that orchestration just wrote.
            // TS parity: `getAsyncHookResponseAttachments` +
            // sync-hook attachments produced inline by
            // `processSessionStartHooks` / `executeUserPromptSubmitHooks`.
            hook_events: Some(Arc::new(
                coco_hooks::reminder_source::CombinedHookEventsSource::new(
                    self.async_hook_registry.clone(),
                    self.sync_hook_buffer.clone(),
                ),
            )),
            // Memory source: only when the runtime is built (gated on
            // `Feature::AutoMemory` upstream).
            memory: self.memory_runtime.as_ref().map(|runtime| {
                Arc::new(coco_query::reminder_adapters::MemoryAdapter::new(
                    runtime.clone(),
                )) as Arc<dyn coco_system_reminder::MemorySource>
            }),
            // Skills source: in-process `SkillManager` Arc kept alive
            // for the session. Empty manager ⇒ generator short-circuits.
            skills: Some(self.skill_manager.clone() as Arc<dyn coco_system_reminder::SkillsSource>),
            ..Default::default()
        };
        engine = engine.with_reminder_sources(sources);
        // Build observers fresh per call so the FileReadState and
        // AppState observers reference the engine's actual handles.
        // Cheap — the registry is just a Vec of Arc<dyn Observer>.
        let observers = coco_query::observers::build_default_registry(
            Some(self.file_read_state.clone()),
            Some(self.denial_tracker.clone()),
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
        // Same snapshot pattern as MCP — every per-turn engine reads
        // the late-bound LSP slot once at wire time. Hot-reloads of
        // the LSP config land on the next engine build.
        if let Some(lsp) = self.lsp_handle.read().await.clone() {
            engine = engine.with_lsp_handle(lsp);
        }
        // Install the agent catalog snapshot so `AgentTool::prompt`
        // renders the dynamic per-turn agent listing (TS parity:
        // `tools/AgentTool/prompt.ts::getPrompt`). Without this the
        // engine falls back to `AgentTool`'s static description and
        // the model never sees the agents it can actually spawn.
        // Each engine instance captures the inner `Arc<...>` once at
        // wire time; concurrent `/agents reload` swaps land on the
        // next per-turn engine, not the in-flight one.
        engine = engine.with_agent_catalog(self.agent_catalog.read().await.clone());
        if let Some(fh) = &self.file_history {
            engine = engine.with_file_history(fh.clone(), self.config_home.clone());
        }
        if let Some(bridge) = &self.permission_bridge {
            engine = engine.with_permission_bridge(bridge.clone());
        }
        // Main-session transcript persistence. Same `TranscriptStore`
        // instance feeds both the per-turn user / assistant JSONL
        // append in `engine_finalize_turn::record_transcript_tail`
        // and the marble-origami metadata writes already wired
        // there. The dedup set lives on `SessionRuntime` so a fresh
        // per-turn engine doesn't re-write history each time.
        // TS parity: `Project.recordTranscript` keys writes by
        // session id and skips already-persisted uuids.
        let live_session_id = self.session_id.read().await.clone();
        engine = engine.with_transcript_store(self.transcript_store.clone(), live_session_id);
        engine = engine.with_transcript_dedup(self.transcript_dedup.clone());
        engine =
            engine.with_tool_result_replacement_state(self.tool_result_replacement_state.clone());
        // Agent handle: installed by bootstrap after TaskRuntime exists.
        // Until then the engine carries the explicit no-op handle from
        // `swarm_agent_handle`.
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
        // Session-scoped prompt-suggestion abort slot (TS module-level
        // `currentAbortController`). Sharing the same `Arc` across
        // every per-turn engine lets a new spawn cancel the in-flight
        // previous one.
        engine = engine.with_current_suggestion_abort(self.current_suggestion_abort.clone());
        // Production task runtime — same `Arc` is shared with
        // `SwarmAgentHandle` so AgentTool background spawns and the
        // engine's `Task*` tools see one source of truth.
        if let Some(rt) = self.task_runtime.read().await.clone() {
            engine = engine.with_task_handle(rt as coco_tool_runtime::BackgroundTaskHandleRef);
        }
        if let Some(task_list) = self.task_list.read().await.clone() {
            engine = engine.with_task_list(task_list);
        }
        if let Some(router) = self.team_task_list_router.read().await.clone() {
            engine = engine.with_team_task_list_router(router);
        }
        engine
    }

    /// Install the agent-spawn handle on this runtime. Called once
    /// after `build()` returns the `Arc<Self>`. The handle is
    /// late-bound because the adapter inside it needs to capture
    /// `Arc<Self>` to drive per-spawn engine builds — calling this
    /// from inside `build()` would create a cycle.
    pub async fn attach_agent_handle(&self, handle: AgentHandleRef) {
        *self.agent_handle.write().await = Some(handle.clone());
        if let Some(runtime) = &self.memory_runtime {
            runtime.install_agent(handle);
        }
    }

    /// Interrupt an in-process teammate's current turn without
    /// cancelling the teammate lifecycle.
    pub async fn interrupt_agent_current_work(&self, agent_id: &str) -> Result<bool, String> {
        let handle = self
            .agent_handle
            .read()
            .await
            .clone()
            .unwrap_or_else(|| self.swarm_agent_handle.clone());
        handle.interrupt_agent_current_work(agent_id).await
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

    pub async fn attach_task_list(&self, handle: coco_tool_runtime::TaskListHandleRef) {
        *self.task_list.write().await = Some(handle);
    }

    pub async fn attach_team_task_list_router(
        &self,
        router: coco_tool_runtime::TeamTaskListRouterRef,
    ) {
        *self.team_task_list_router.write().await = Some(router);
    }

    pub async fn current_task_list(&self) -> Option<coco_tool_runtime::TaskListHandleRef> {
        self.task_list.read().await.clone()
    }

    pub async fn current_team_task_list_router(
        &self,
    ) -> Option<coco_tool_runtime::TeamTaskListRouterRef> {
        self.team_task_list_router.read().await.clone()
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
    /// Fire SessionStart hooks with the given `source` string ("startup",
    /// "resume", "compact", "clear"). Output flows into the shared
    /// `sync_hook_buffer` so it surfaces as `hook_*` reminders on the
    /// next turn — TS parity with `processSessionStartHooks(source)`.
    ///
    /// Runners call this once at session bootstrap (TUI / SDK) so the
    /// first turn's reminder pass picks up the events. Failure is
    /// logged + tolerated; no panic on hook misconfig.
    pub async fn fire_session_start_hooks(&self, source: &str) {
        // TS `SessionStartHookInputSchema.source` is the closed enum
        // `('startup' | 'resume' | 'clear' | 'compact')`. Parse here so
        // callers using bare strings still work, but log + skip if the
        // string is unrecognised to avoid wiring an out-of-spec value.
        let parsed_source = match source {
            "startup" => coco_hooks::orchestration::SessionStartSource::Startup,
            "resume" => coco_hooks::orchestration::SessionStartSource::Resume,
            "clear" => coco_hooks::orchestration::SessionStartSource::Clear,
            "compact" => coco_hooks::orchestration::SessionStartSource::Compact,
            other => {
                warn!(
                    source = other,
                    "SessionStart hook fired with unrecognised source; skipping"
                );
                return;
            }
        };
        let cfg = self.current_engine_config().await;
        let session_id = self.current_session_id().await;
        let ctx = coco_hooks::orchestration::OrchestrationContext {
            session_id,
            cwd: std::env::current_dir().unwrap_or_default(),
            project_dir: cfg.project_dir.clone(),
            permission_mode: None,
            transcript_path: None,
            agent_id: None,
            agent_type: None,
            cancel: self.cancel.clone(),
            disable_all_hooks: cfg.disable_all_hooks,
            allow_managed_hooks_only: cfg.allow_managed_hooks_only,
            attachment_emitter: coco_messages::AttachmentEmitter::noop(),
            sync_event_sink: Some(self.sync_hook_buffer.clone()),
            http_url_allowlist: None,
            http_env_var_policy: None,
            async_registry: Some(self.async_hook_registry.clone()),
            llm_handle: Some(self.hook_llm_handle.clone()),
            workspace_trust_accepted: None,
        };
        let model_arg = if cfg.model_id.is_empty() {
            None
        } else {
            Some(cfg.model_id.as_str())
        };
        match coco_hooks::orchestration::execute_session_start(
            &self.hook_registry,
            &ctx,
            parsed_source,
            /*agent_type*/ None,
            model_arg,
        )
        .await
        {
            Ok(agg) => {
                // TS `SessionStartHookSpecificOutputSchema.watchPaths` —
                // hook output may register paths the FileChanged watcher
                // should monitor. We hand them off to the runtime's
                // shared watcher so subsequent file events fire
                // FileChanged hooks. Empty vec is a no-op.
                if !agg.watch_paths.is_empty() {
                    self.add_file_watch_paths(agg.watch_paths.clone()).await;
                }
            }
            Err(e) => {
                warn!(error = %e, source, "SessionStart hook execution failed at startup");
            }
        }
    }

    /// Fire Setup hooks (TS `executeSetupHooks(trigger)`).
    ///
    /// Called at session bootstrap with `Maintenance`, and at explicit
    /// `coco init` entry with `Init`. Output is fire-and-forget — TS
    /// treats Setup as observability-only (no blocking, no continuation
    /// signals). Failure is logged.
    pub async fn fire_setup_hooks(&self, trigger: coco_hooks::orchestration::SetupTrigger) {
        let cfg = self.current_engine_config().await;
        let session_id = self.current_session_id().await;
        let ctx = coco_hooks::orchestration::OrchestrationContext {
            session_id,
            cwd: std::env::current_dir().unwrap_or_default(),
            project_dir: cfg.project_dir.clone(),
            permission_mode: None,
            transcript_path: None,
            agent_id: None,
            agent_type: None,
            cancel: self.cancel.clone(),
            disable_all_hooks: cfg.disable_all_hooks,
            allow_managed_hooks_only: cfg.allow_managed_hooks_only,
            attachment_emitter: coco_messages::AttachmentEmitter::noop(),
            sync_event_sink: Some(self.sync_hook_buffer.clone()),
            http_url_allowlist: None,
            http_env_var_policy: None,
            async_registry: Some(self.async_hook_registry.clone()),
            llm_handle: Some(self.hook_llm_handle.clone()),
            workspace_trust_accepted: None,
        };
        if let Err(e) =
            coco_hooks::orchestration::execute_setup(&self.hook_registry, &ctx, trigger).await
        {
            warn!(error = %e, ?trigger, "Setup hook execution failed");
        }
    }

    /// Fire UserPromptSubmit hooks for the given prompt text. Output
    /// flows into the shared `sync_hook_buffer`. Returns the aggregated
    /// result so the caller can honour `blocking_error` (suppress the
    /// turn) and `prevent_continuation` (skip the turn but keep the
    /// prompt) — TS parity with
    /// `executeUserPromptSubmitHooks` consumed by
    /// `processUserInput.ts:182-263`.
    pub async fn fire_user_prompt_submit_hooks(
        &self,
        prompt: &str,
    ) -> coco_hooks::orchestration::AggregatedHookResult {
        let cfg = self.current_engine_config().await;
        let session_id = self.current_session_id().await;
        let ctx = coco_hooks::orchestration::OrchestrationContext {
            session_id,
            cwd: std::env::current_dir().unwrap_or_default(),
            project_dir: cfg.project_dir.clone(),
            permission_mode: Some(format!("{:?}", cfg.permission_mode)),
            transcript_path: None,
            agent_id: None,
            agent_type: None,
            cancel: self.cancel.clone(),
            disable_all_hooks: cfg.disable_all_hooks,
            allow_managed_hooks_only: cfg.allow_managed_hooks_only,
            attachment_emitter: coco_messages::AttachmentEmitter::noop(),
            sync_event_sink: Some(self.sync_hook_buffer.clone()),
            http_url_allowlist: None,
            http_env_var_policy: None,
            async_registry: Some(self.async_hook_registry.clone()),
            llm_handle: Some(self.hook_llm_handle.clone()),
            workspace_trust_accepted: None,
        };
        match coco_hooks::orchestration::execute_user_prompt_submit(
            &self.hook_registry,
            &ctx,
            prompt,
        )
        .await
        {
            Ok(agg) => agg,
            Err(e) => {
                warn!(error = %e, "UserPromptSubmit hook execution failed");
                coco_hooks::orchestration::AggregatedHookResult::default()
            }
        }
    }

    /// Fire Notification hooks (TS `executeNotificationHooks(notif)`).
    ///
    /// Called from `TuiPermissionBridge` / `SdkPermissionBridge` when
    /// the user is about to be asked for input (`permission_prompt`),
    /// and from any future idle / elicitation prompts. Output is
    /// fire-and-forget — TS `notifier.ts:25` awaits the hook only to
    /// preserve ordering before the actual UI notification, never to
    /// block the prompt itself.
    pub async fn fire_notification_hooks(
        &self,
        notification_type: &str,
        message: &str,
        title: Option<&str>,
    ) {
        let cfg = self.current_engine_config().await;
        let session_id = self.current_session_id().await;
        let ctx = coco_hooks::orchestration::OrchestrationContext {
            session_id,
            cwd: std::env::current_dir().unwrap_or_default(),
            project_dir: cfg.project_dir.clone(),
            permission_mode: Some(format!("{:?}", cfg.permission_mode)),
            transcript_path: None,
            agent_id: None,
            agent_type: None,
            cancel: self.cancel.clone(),
            disable_all_hooks: cfg.disable_all_hooks,
            allow_managed_hooks_only: cfg.allow_managed_hooks_only,
            attachment_emitter: coco_messages::AttachmentEmitter::noop(),
            sync_event_sink: Some(self.sync_hook_buffer.clone()),
            http_url_allowlist: None,
            http_env_var_policy: None,
            async_registry: Some(self.async_hook_registry.clone()),
            llm_handle: Some(self.hook_llm_handle.clone()),
            workspace_trust_accepted: None,
        };
        if let Err(e) = coco_hooks::orchestration::execute_notification(
            &self.hook_registry,
            &ctx,
            notification_type,
            message,
            title,
        )
        .await
        {
            warn!(
                error = %e,
                notification_type,
                "Notification hook execution failed"
            );
        }
    }

    fn file_watch_registration_context(&self) -> FileWatchRegistrationContext {
        FileWatchRegistrationContext {
            file_changed_watcher: self.file_changed_watcher.clone(),
            hook_registry: self.hook_registry.clone(),
            session_id: self.session_id.clone(),
            engine_config: self.engine_config.clone(),
            cancel: self.cancel.clone(),
            async_hook_registry: self.async_hook_registry.clone(),
            hook_llm_handle: self.hook_llm_handle.clone(),
        }
    }

    /// Append paths to the `FileChanged` watcher, lazily constructing
    /// it on first call. TS parity: `fileChangedWatcher.ts:add` lazily
    /// boots the chokidar instance the first time a hook returns a
    /// `watchPaths` array. Empty input is a no-op.
    pub async fn add_file_watch_paths(&self, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        self.file_watch_registration_context()
            .add_paths(paths)
            .await;
    }

    /// Fire CwdChanged hooks (TS `executeCwdChangedHooks(oldCwd, newCwd)`).
    ///
    /// Callers must capture the old cwd before mutating
    /// `std::env::current_dir`. TS only fires this from
    /// `fileChangedWatcher.ts:148` (chokidar-equivalent watcher); coco-rs
    /// will gain that watcher as part of P4. In the meantime, surfacing
    /// the helper lets ad-hoc cwd-mutating code paths (worktree exit,
    /// SDK setCwd control) wire the hook without re-implementing the
    /// orchestration context build.
    pub async fn fire_cwd_changed_hooks(&self, old_cwd: &str, new_cwd: &str) {
        let cfg = self.current_engine_config().await;
        let session_id = self.current_session_id().await;
        let ctx = coco_hooks::orchestration::OrchestrationContext {
            session_id,
            cwd: std::path::PathBuf::from(new_cwd),
            project_dir: cfg.project_dir.clone(),
            permission_mode: Some(format!("{:?}", cfg.permission_mode)),
            transcript_path: None,
            agent_id: None,
            agent_type: None,
            cancel: self.cancel.clone(),
            disable_all_hooks: cfg.disable_all_hooks,
            allow_managed_hooks_only: cfg.allow_managed_hooks_only,
            attachment_emitter: coco_messages::AttachmentEmitter::noop(),
            sync_event_sink: Some(self.sync_hook_buffer.clone()),
            http_url_allowlist: None,
            http_env_var_policy: None,
            async_registry: Some(self.async_hook_registry.clone()),
            llm_handle: Some(self.hook_llm_handle.clone()),
            workspace_trust_accepted: None,
        };
        match coco_hooks::orchestration::execute_cwd_changed(
            &self.hook_registry,
            &ctx,
            old_cwd,
            new_cwd,
        )
        .await
        {
            Ok(agg) => {
                // TS `CwdChangedHookSpecificOutputSchema.watchPaths` —
                // the cwd swap is a natural moment for hooks to update
                // the FileChanged watch list (e.g. add the new project's
                // `.envrc`).
                if !agg.watch_paths.is_empty() {
                    self.add_file_watch_paths(agg.watch_paths.clone()).await;
                }
            }
            Err(e) => {
                warn!(error = %e, old_cwd, new_cwd, "CwdChanged hook execution failed");
            }
        }
    }

    /// Fire ConfigChange hooks (TS `executeConfigChangeHooks`).
    ///
    /// Called from the per-session config-change watcher task spawned
    /// by [`Self::spawn_config_change_watcher`]. Output is fire-and-forget;
    /// TS uses the result for `hasBlockingResult` checks but coco-rs's
    /// reload pipeline already publishes the new `RuntimeConfig` before
    /// hooks fire, so the hook is observe-only here.
    pub async fn fire_config_change_hooks(
        &self,
        source: coco_hooks::orchestration::ConfigChangeSource,
        file_path: Option<&str>,
    ) {
        let cfg = self.current_engine_config().await;
        let session_id = self.current_session_id().await;
        let ctx = coco_hooks::orchestration::OrchestrationContext {
            session_id,
            cwd: std::env::current_dir().unwrap_or_default(),
            project_dir: cfg.project_dir.clone(),
            permission_mode: Some(format!("{:?}", cfg.permission_mode)),
            transcript_path: None,
            agent_id: None,
            agent_type: None,
            cancel: self.cancel.clone(),
            disable_all_hooks: cfg.disable_all_hooks,
            allow_managed_hooks_only: cfg.allow_managed_hooks_only,
            attachment_emitter: coco_messages::AttachmentEmitter::noop(),
            sync_event_sink: Some(self.sync_hook_buffer.clone()),
            http_url_allowlist: None,
            http_env_var_policy: None,
            async_registry: Some(self.async_hook_registry.clone()),
            llm_handle: Some(self.hook_llm_handle.clone()),
            workspace_trust_accepted: None,
        };
        if let Err(e) = coco_hooks::orchestration::execute_config_change(
            &self.hook_registry,
            &ctx,
            source,
            file_path,
        )
        .await
        {
            warn!(error = %e, source = ?source, "ConfigChange hook execution failed");
        }
    }

    /// Spawn a tokio task that subscribes to a [`coco_config_reload::ConfigChange`]
    /// stream and fires the corresponding TS-aligned `ConfigChange` hook
    /// for each event. Returns the [`tokio::task::JoinHandle`] so the
    /// caller can hold it for the session lifetime; dropping it aborts
    /// the watcher.
    ///
    /// `cancel` lets callers terminate the watcher proactively
    /// (typically the session-level [`Self::cancel`] token); when the
    /// broadcast channel closes (reloader dropped), the loop exits on
    /// its own.
    pub fn spawn_config_change_watcher(
        self: &Arc<Self>,
        mut rx: tokio::sync::broadcast::Receiver<coco_config_reload::ConfigChange>,
    ) -> tokio::task::JoinHandle<()> {
        let runtime = Arc::clone(self);
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    recv = rx.recv() => match recv {
                        Ok(change) => {
                            let source = config_change_source_for_kind(change.kind);
                            let path = change.path.to_string_lossy().into_owned();
                            runtime
                                .fire_config_change_hooks(source, Some(&path))
                                .await;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!(skipped, "ConfigChange watcher lagged; events dropped");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        })
    }

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
        self.denial_tracker.lock().await.clear();
        *self.tool_result_replacement_state.write().await =
            coco_tool_runtime::tool_result_storage::ContentReplacementState::new(i64::MAX);
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
        write_std_rwlock(&self.orchestration_session_id, new_session_id.to_string());
        let new_id_for_cfg = new_session_id.to_string();
        self.update_engine_config(|cfg| cfg.session_id = new_id_for_cfg)
            .await;
        if let Some(svc) = &self.session_memory_service {
            svc.set_session_id(new_session_id.to_string()).await;
        }
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
        self.main_client().await.cache_break_reset().await;
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
        let snapshot = {
            let mut g = self.engine_config.write().await;
            f(&mut g);
            g.clone()
        };
        write_std_rwlock(&self.orchestration_engine_config, snapshot);
    }

    /// Snapshot the current command registry. Cheap (single Arc clone).
    /// Callers should hold the snapshot for the duration of one
    /// dispatch — a concurrent `/reload-plugins` may swap the inner
    /// Arc, but existing snapshots stay live until dropped.
    pub async fn current_command_registry(&self) -> Arc<CommandRegistry> {
        self.command_registry.read().await.clone()
    }

    /// Rebuild the slash-command registry from disk and atomically
    /// swap it in. Triggered by `/reload-plugins` so the user can pick
    /// up plugin / skill / command edits without restarting the
    /// session. New `SkillManager` + `PluginManager` are constructed
    /// fresh each call; resolution order matches the original
    /// bootstrap (`commands::build_command_registry`).
    ///
    /// Returns the count of registered commands in the new registry
    /// so the caller can show the user a confirmation.
    pub async fn reload_plugins(&self, cwd: &std::path::Path) -> usize {
        let skill_manager = coco_skills::SkillManager::new();
        skill_manager.load_from_dirs(&[
            self.config_home.join("skills"),
            cwd.join(".coco").join("skills"),
        ]);
        let mut plugin_manager = coco_plugins::PluginManager::new();
        plugin_manager.load_from_dirs(&coco_plugins::get_plugin_dirs(&self.config_home, cwd));
        let registry = coco_commands::build_command_registry(
            &skill_manager,
            &plugin_manager,
            coco_types::UserType::from_env(),
            self.runtime_config.features.clone(),
            cwd.to_path_buf(),
            dirs::home_dir().unwrap_or_else(|| cwd.to_path_buf()),
            None,
        );
        let count = registry.len();
        let new_registry = Arc::new(registry);
        let mut slot = self.command_registry.write().await;
        *slot = new_registry;
        count
    }

    /// Reload the live `HookRegistry` from the latest `RuntimeConfig`
    /// snapshot (settings + plugin hooks). Triggered by `/hooks reload`.
    ///
    /// TS parity: `updateHooksConfigSnapshot()`
    /// (`utils/hooks/hooksConfigSnapshot.ts`).
    ///
    /// Atomic semantics:
    /// - Settings hooks (User/Project/Local/Flag/Policy scopes) and
    ///   plugin hooks are both rebuilt.
    /// - `fired_once` set is **preserved** so a `once` hook that
    ///   already fired this session doesn't re-fire after reload.
    /// - Per-agent `agent_scoped` hook layer is **preserved** — those are
    ///   owned by the coordinator's spawn lifecycle, not by settings.
    /// - Slash commands run only at turn boundaries (the dispatch loop
    ///   in `tui_runner` `drain_active_turn`s before invoking them),
    ///   so PreToolUse/PostToolUse for an in-flight call cannot see
    ///   different hook sets.
    ///
    /// Returns the count of hooks now registered.
    pub async fn reload_hooks(&self) -> Result<usize> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| self.config_home.clone());
        let policy = coco_hooks::LoaderPolicy {
            disable_all_hooks: self.runtime_config.settings.merged.disable_all_hooks,
            allow_managed_hooks_only: self.runtime_config.settings.merged.allow_managed_hooks_only,
        };

        // Build (scope, value) pairs for every active settings source.
        // Plugin hooks are layered separately because they live on
        // disk inside plugin directories, not in settings.json.
        let mut sources: Vec<(coco_types::HookScope, serde_json::Value)> = Vec::new();
        for source in [
            coco_config::SettingSource::User,
            coco_config::SettingSource::Project,
            coco_config::SettingSource::Local,
            coco_config::SettingSource::Flag,
            coco_config::SettingSource::Policy,
        ] {
            let Some(value) = self.runtime_config.settings.per_source.get(&source) else {
                continue;
            };
            let Some(hooks_value) = value.get("hooks") else {
                continue;
            };
            let scope = match source {
                coco_config::SettingSource::User => coco_types::HookScope::User,
                coco_config::SettingSource::Project => coco_types::HookScope::Project,
                coco_config::SettingSource::Local => coco_types::HookScope::Local,
                coco_config::SettingSource::Flag => coco_types::HookScope::Local,
                coco_config::SettingSource::Policy => coco_types::HookScope::Policy,
                coco_config::SettingSource::Plugin => coco_types::HookScope::Plugin,
            };
            sources.push((scope, hooks_value.clone()));
        }

        // Atomic settings-hook swap.
        let settings_count = self
            .hook_registry
            .reload_from_runtime(&sources, policy)
            .map_err(|e| anyhow::anyhow!("hook reload failed: {e}"))?;

        // Re-layer plugin hooks on top — they aren't in settings.json
        // so `reload_from_runtime` doesn't see them.
        let mut plugin_manager = coco_plugins::PluginManager::new();
        plugin_manager.load_from_dirs(&coco_plugins::get_plugin_dirs(&self.config_home, &cwd));
        let plugin_refs: Vec<&coco_plugins::LoadedPlugin> = plugin_manager.enabled();
        if !plugin_refs.is_empty() {
            coco_plugins::hook_bridge::register_plugin_hooks(&self.hook_registry, &plugin_refs);
        }

        Ok(self.hook_registry.len().max(settings_count))
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
                transcript_path: None,
                agent_id: None,
                agent_type: None,
                cancel: self.cancel.clone(),
                disable_all_hooks: cfg.disable_all_hooks,
                allow_managed_hooks_only: cfg.allow_managed_hooks_only,
                attachment_emitter: coco_messages::AttachmentEmitter::noop(),
                // SessionEnd doesn't surface as a reminder in TS, so
                // no sink needed here.
                sync_event_sink: None,
                http_url_allowlist: None,
                http_env_var_policy: None,
                async_registry: Some(self.async_hook_registry.clone()),
                llm_handle: Some(self.hook_llm_handle.clone()),
                workspace_trust_accepted: None,
            };
            if let Err(e) = coco_hooks::orchestration::execute_session_end(
                &self.hook_registry,
                &pre_ctx,
                coco_hooks::orchestration::ExitReason::Clear,
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

        // Drop any queued steering messages — `/clear` (and the lighter
        // `/clear history`) semantically says "fresh start", and a
        // queued prompt from the pre-clear session would otherwise
        // surface as the first user input in the post-clear transcript.
        // Runs before the `is_history_only` early return so both scopes
        // wipe the queue. TS parity: `clearCommandQueue()` from
        // `utils/messageQueueManager.ts` is invoked alongside the
        // history reset in REPL.tsx's clear path.
        self.command_queue.clear().await;

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
        if let Some(svc) = &self.session_memory_service {
            svc.set_last_summarized_message_id(None).await;
        }

        // Reset the auto-memory runtime's per-conversation state — recall
        // PrefetchState, extraction cursor + throttle counter, and the
        // session-memory init flag. The on-disk MEMORY.md and topic
        // files are deliberately left alone; they're cross-conversation
        // memory that a /clear is not meant to wipe. TS parity:
        // `MemoryRuntime::reset` mirrors the equivalent reset path in
        // `clearConversation()` for forked-extraction state.
        if let Some(runtime) = &self.memory_runtime {
            runtime.reset().await;
        }

        // Step 4 (TS conversation.ts:203): regenerate the session id and
        // propagate it to every id-keyed subsystem. Without this, post-
        // clear writes would land in the OLD session's directory and
        // surface as "extra memory" / "phantom file-history snapshots"
        // on the next `--resume` of the pre-clear session.
        let new_session_id = uuid::Uuid::new_v4().to_string();
        self.adopt_session_id(&new_session_id).await;
        // Reset the transcript dedup set so the new session writes a
        // fresh JSONL from message #1 — without this, the post-clear
        // turn would skip persisting any UUID that happened to match
        // a pre-clear message (impossible in practice, but the empty
        // set is the correct invariant per TS `clearSessionCaches`).
        self.transcript_dedup.lock().await.clear();
        *self.tool_result_replacement_state.write().await =
            coco_tool_runtime::tool_result_storage::ContentReplacementState::new(i64::MAX);

        // Step 5 (TS conversation.ts:245): SessionStart hooks. Result
        // messages seed the post-clear transcript.
        let cfg = self.current_engine_config().await;
        let post_ctx = coco_hooks::orchestration::OrchestrationContext {
            session_id: new_session_id,
            cwd: std::env::current_dir().unwrap_or_default(),
            project_dir: cfg.project_dir.clone(),
            permission_mode: None,
            transcript_path: None,
            agent_id: None,
            agent_type: None,
            cancel: self.cancel.clone(),
            disable_all_hooks: cfg.disable_all_hooks,
            allow_managed_hooks_only: cfg.allow_managed_hooks_only,
            attachment_emitter: coco_messages::AttachmentEmitter::noop(),
            // Surface SessionStart hook output as `hook_*` reminders on
            // the next turn — TS parity with `processSessionStartHooks`
            // emitting `createAttachmentMessage({ hookEvent:'SessionStart', ... })`.
            sync_event_sink: Some(self.sync_hook_buffer.clone()),
            http_url_allowlist: None,
            http_env_var_policy: None,
            async_registry: Some(self.async_hook_registry.clone()),
            llm_handle: Some(self.hook_llm_handle.clone()),
            workspace_trust_accepted: None,
        };
        let model_arg = if cfg.model_id.is_empty() {
            None
        } else {
            Some(cfg.model_id.as_str())
        };
        // Clear the in-memory transcript before invoking SessionStart
        // hooks. The hook output flows into the sync hook buffer
        // (`post_ctx.sync_event_sink`) and surfaces as `hook_*`
        // reminders on the next turn — TS parity with
        // `processSessionStartHooks('clear')` returning attachment
        // messages that the conversation engine appends post-clear.
        {
            let mut h = self.history.lock().await;
            h.clear();
        }
        if let Err(e) = coco_hooks::orchestration::execute_session_start(
            &self.hook_registry,
            &post_ctx,
            coco_hooks::orchestration::SessionStartSource::Clear,
            /*agent_type*/ None,
            model_arg,
        )
        .await
        {
            warn!(error = %e, "SessionStart hook execution failed during /clear");
        }

        Ok(())
    }
}

/// Construct an `Arc<SandboxState>` for the active session, or return `None`
/// when sandbox is disabled / unavailable. Drives TS-equivalent bootstrap
/// (`SandboxManager.initialize`) without re-implementing the npm runtime —
/// the heavy lifting lives in the `coco_sandbox` crate.
///
/// Returns `Ok(None)` when:
/// - `Feature::Sandbox` is off, or
/// - mode is `FullAccess`, or
/// - bootstrap gates fail AND `sandbox.fail_if_unavailable` is `false`
///   (commands will run unsandboxed; user gets a startup banner).
///
/// Returns `Err` when bootstrap gates fail AND
/// `sandbox.fail_if_unavailable` is `true` — the caller propagates this so
/// coco exits before the REPL starts, matching TS
/// `sandbox.failIfUnavailable` (`entrypoints/sandboxTypes.ts:95`).
pub(crate) fn build_sandbox_state(
    runtime_config: &RuntimeConfig,
    cwd: &std::path::Path,
) -> anyhow::Result<Option<Arc<coco_sandbox::SandboxState>>> {
    use coco_sandbox::adapter::AdapterInputs;

    if !runtime_config
        .features
        .enabled(coco_types::Feature::Sandbox)
    {
        return Ok(None);
    }

    let mode = runtime_config.sandbox.mode;
    if matches!(mode, coco_types::SandboxMode::FullAccess) {
        return Ok(None);
    }

    // `runtime_config.sandbox` is now the rich, TS-parity `SandboxSettings`
    // type owned by coco-config — no manual bridging needed. We mark it
    // `enabled = true` because reaching this point already implies the
    // feature gate passed and the user requested an enforcing mode.
    let mut sandbox_settings = runtime_config.sandbox.clone();
    sandbox_settings.enabled = true;

    let gate = coco_sandbox::check_enable_gates(&sandbox_settings);
    if !matches!(gate, coco_sandbox::EnableCheckResult::Enabled) {
        // Surface a TS-parity startup banner via `sandbox_unavailable_reason`
        // so the user understands *why* sandboxing is degraded. When
        // `fail_if_unavailable` is set, this is a hard error.
        let missing_deps: Vec<String> = match &gate {
            coco_sandbox::EnableCheckResult::DisabledByMissingDeps { missing } => missing.clone(),
            _ => Vec::new(),
        };
        let reason = coco_sandbox::sandbox_unavailable_reason(
            &sandbox_settings,
            coco_sandbox::current_platform_supported(),
            sandbox_settings.is_platform_enabled(),
            &missing_deps,
        );

        if sandbox_settings.fail_if_unavailable {
            let detail = reason.unwrap_or_else(|| format!("sandbox bootstrap failed: {gate:?}"));
            return Err(anyhow::anyhow!(
                "sandbox.fail_if_unavailable is set but sandbox cannot start: {detail}"
            ));
        }

        if let Some(banner) = reason {
            // stderr so the message survives any TUI redirection.
            eprintln!("[coco] sandbox unavailable: {banner}");
            warn!(?gate, banner, "sandbox enabled but runtime cannot start");
        } else {
            warn!(?gate, "sandbox enabled but runtime cannot start");
        }
        return Ok(None);
    }

    let settings_root = runtime_config
        .paths
        .project_dir
        .clone()
        .unwrap_or_else(|| cwd.to_path_buf());

    let permission_allow_rules: Vec<String> =
        runtime_config.settings.merged.permissions.allow.clone();
    let permission_deny_rules: Vec<String> =
        runtime_config.settings.merged.permissions.deny.clone();
    let additional_directories: Vec<PathBuf> = runtime_config
        .settings
        .merged
        .permissions
        .additional_directories
        .iter()
        .map(PathBuf::from)
        .collect();

    let coco_temp_dir = std::env::temp_dir().join("coco");
    let worktree = coco_sandbox::detect_worktree_main_repo(cwd);

    // Per-source rule plumbing — drives the `allow_managed_*_only`
    // gates. The adapter needs source provenance because the merged
    // `SandboxSettings` collapses every layer; only allow rules need
    // sourcing (TS always honors all-source denies).
    // The sandbox adapter only consumes allow-source provenance today
    // (deny rules apply uniformly regardless of source). `_ask` is
    // ignored here; the ask map is consumed at the engine config layer
    // via `permission_rule_loader::typed_permission_rules`.
    let (sourced_allow_rules, _sourced_deny_rules, _sourced_ask_rules) =
        runtime_config.settings.sourced_permission_rules();
    let sourced_fs_allow_read = runtime_config.settings.sourced_filesystem_allow_read();

    let inputs = AdapterInputs {
        settings: &sandbox_settings,
        mode,
        settings_root: &settings_root,
        original_cwd: cwd,
        current_cwd: cwd,
        permission_allow_rules: &permission_allow_rules,
        permission_deny_rules: &permission_deny_rules,
        additional_directories: &additional_directories,
        coco_temp_dir: &coco_temp_dir,
        settings_files: &[],
        worktree_main_repo: worktree.as_deref(),
        sourced_permission_allow_rules: Some(&sourced_allow_rules),
        sourced_filesystem_allow_read: Some(&sourced_fs_allow_read),
    };
    let out = coco_sandbox::build_runtime_config(inputs);

    let platform = coco_sandbox::platform::create_platform();
    let state = match mode {
        coco_types::SandboxMode::ExternalSandbox => {
            coco_sandbox::SandboxState::external(out.enforcement, out.settings, out.config)
        }
        _ => coco_sandbox::SandboxState::new(out.enforcement, out.settings, out.config, platform),
    };
    Ok(Some(Arc::new(state)))
}

#[cfg(test)]
#[path = "session_runtime.test.rs"]
mod tests;
