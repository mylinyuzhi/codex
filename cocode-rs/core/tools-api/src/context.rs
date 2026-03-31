//! Tool execution context.
//!
//! Provides [`ToolContext`] and [`ToolContextBuilder`] — the context
//! passed to every tool during execution.

// Re-export extracted types so `crate::context::*` paths continue to work.
pub use crate::file_tracker::FileReadState;
pub use crate::file_tracker::FileTracker;
pub use crate::permission::ApprovalStore;
pub use crate::permission::InvokedSkill;
pub use crate::permission::PermissionRequester;
pub use crate::question::QuestionResponder;
pub use crate::spawn_agent::AgentCancelTokens;
pub use crate::spawn_agent::KilledAgents;
pub use crate::spawn_agent::ModelCallFn;
pub use crate::spawn_agent::ModelCallInput;
pub use crate::spawn_agent::ModelCallResult;
pub use crate::spawn_agent::SpawnAgentFn;
pub use crate::spawn_agent::SpawnAgentInput;
pub use crate::spawn_agent::SpawnAgentResult;
use cocode_hooks::HookRegistry;
use cocode_lsp::LspServerManager;
use cocode_policy::PermissionRuleEvaluator;
use cocode_protocol::CoreEvent;
use cocode_protocol::Features;
use cocode_protocol::PermissionMode;
use cocode_protocol::RoleSelections;
use cocode_protocol::TuiEvent;
use cocode_protocol::WebFetchConfig;
use cocode_protocol::WebSearchConfig;
use cocode_shell::ShellExecutor;
use cocode_skill::SkillManager;
use cocode_skill::SkillUsageTracker;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;

/// Per-call identification.
#[derive(Debug, Clone)]
pub struct ToolCallIdentity {
    /// Unique call ID for this execution.
    pub call_id: String,
    /// Session ID.
    pub session_id: String,
    /// Turn ID for the current conversation turn.
    pub turn_id: String,
    /// Turn number for the current conversation turn (1-indexed).
    pub turn_number: i32,
    /// Agent ID (set when running inside a sub-agent).
    pub agent_id: Option<String>,
}

/// Environment and configuration for tool execution.
#[derive(Debug, Clone)]
pub struct ToolEnvironment {
    /// Current working directory.
    pub cwd: PathBuf,
    /// Additional working directories (e.g., for multi-root workspaces).
    pub additional_working_directories: Vec<PathBuf>,
    /// Permission mode for this execution.
    pub permission_mode: PermissionMode,
    /// Feature flags for tool enablement checks.
    pub features: Features,
    /// Web search configuration.
    pub web_search_config: WebSearchConfig,
    /// Web fetch configuration.
    pub web_fetch_config: WebFetchConfig,
    /// Allowed subagent types for the Task tool.
    ///
    /// When set (from `Task(type1, type2)` syntax in the agent's tools list),
    /// only the specified subagent types can be spawned. `None` means no
    /// restriction -- all agent types are available.
    pub task_type_restrictions: Option<Vec<String>>,
    /// Whether plan mode is currently active.
    pub is_plan_mode: bool,
    /// Whether this is an ultraplan session (plan pre-written by a remote session).
    pub is_ultraplan: bool,
}

/// Event emission and cancellation channels.
#[derive(Debug, Clone)]
pub struct ToolChannels {
    /// Channel for emitting core events.
    pub event_tx: Option<mpsc::Sender<CoreEvent>>,
    /// Cancellation token for aborting execution.
    pub cancel_token: CancellationToken,
}

/// Shared mutable state across tool executions (all Arc-wrapped).
#[derive(Clone)]
pub struct ToolSharedState {
    /// Stored approvals.
    pub approval_store: Arc<Mutex<ApprovalStore>>,
    /// File tracker.
    pub file_tracker: Arc<Mutex<FileTracker>>,
    /// Skills that have been invoked (for hook cleanup).
    pub invoked_skills: Arc<Mutex<Vec<InvokedSkill>>>,
    /// Per-task byte offsets for incremental (delta) output reading.
    ///
    /// TaskOutput stores the last-read byte offset per task_id so subsequent
    /// reads only return new entries, matching CC's `readOutputFileDelta`.
    pub output_offsets: Arc<tokio::sync::Mutex<HashMap<String, u64>>>,
}

/// Service handles for tool execution.
#[derive(Clone)]
pub struct ToolServices {
    /// Shell executor for command execution and background task management.
    pub shell_executor: ShellExecutor,
    /// Sandbox state for platform-level command isolation.
    pub sandbox_state: Option<Arc<cocode_sandbox::SandboxState>>,
    /// Optional LSP server manager for language intelligence tools.
    pub lsp_manager: Option<Arc<LspServerManager>>,
    /// Optional skill manager for executing named skills.
    pub skill_manager: Option<Arc<SkillManager>>,
    /// Optional skill usage tracker for recording invocations.
    pub skill_usage_tracker: Option<Arc<SkillUsageTracker>>,
    /// Optional hook registry for skill hook integration.
    pub hook_registry: Option<Arc<HookRegistry>>,
    /// Optional permission requester for interactive approval flow.
    ///
    /// When set, the executor can route `NeedsApproval` results to the
    /// UI/TUI for user confirmation instead of denying immediately.
    pub permission_requester: Option<Arc<dyn PermissionRequester>>,
    /// Optional permission rule evaluator for pre-configured rules.
    ///
    /// When set, rules are evaluated before the tool's own `check_permission()`
    /// to allow, deny, or delegate based on project/user/policy configuration.
    pub permission_evaluator: Option<PermissionRuleEvaluator>,
    /// Optional file backup store for pre-modify snapshots (Tier 1 rewind).
    pub file_backup_store: Option<Arc<cocode_file_backup::FileBackupStore>>,
    /// Optional question responder for AskUserQuestion tool.
    ///
    /// When set, the AskUserQuestion tool can emit a `QuestionAsked` event
    /// and wait for the user's structured response via a oneshot channel.
    pub question_responder: Option<Arc<QuestionResponder>>,
}

/// Subagent-related context.
#[derive(Clone)]
pub struct AgentContext {
    /// Optional callback for spawning subagents.
    pub spawn_agent_fn: Option<SpawnAgentFn>,
    /// Shared registry of cancellation tokens for background agents.
    ///
    /// TaskStop uses this to cancel agents by ID. Tokens are registered
    /// by the executor when spawning subagents.
    pub agent_cancel_tokens: AgentCancelTokens,
    /// Shared set of agent IDs killed via TaskStop.
    ///
    /// Populated by `kill_shell.rs` after cancelling an agent so the session
    /// layer can report the agent's status as `Killed` instead of `Failed`.
    pub killed_agents: KilledAgents,
    /// Base directory for background agent output files.
    ///
    /// Used by TaskOutput to find agent output JSONL files. When set, this
    /// takes precedence over the fallback session_dir and temp_dir checks.
    pub agent_output_dir: Option<PathBuf>,
    /// Optional lightweight model call function (for SmartEdit correction).
    pub model_call_fn: Option<ModelCallFn>,
    /// Parent's role selections (snapshot for subagent isolation).
    ///
    /// When set, spawned subagents will inherit these selections,
    /// ensuring they're unaffected by subsequent changes to the parent's settings.
    pub parent_selections: Option<RoleSelections>,
    /// Team name when running as a teammate.
    ///
    /// When set, tools like ExitPlanMode route approval through the team
    /// mailbox instead of showing a user-facing dialog.
    pub team_name: Option<String>,
}

impl std::fmt::Debug for ToolSharedState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolSharedState").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ToolServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolServices")
            .field("lsp_manager", &self.lsp_manager.is_some())
            .field("skill_manager", &self.skill_manager.is_some())
            .field("hook_registry", &self.hook_registry.is_some())
            .field("permission_requester", &self.permission_requester.is_some())
            .field("permission_evaluator", &self.permission_evaluator.is_some())
            .field("file_backup_store", &self.file_backup_store.is_some())
            .field("question_responder", &self.question_responder.is_some())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for AgentContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentContext")
            .field("spawn_agent_fn", &self.spawn_agent_fn.is_some())
            .field("agent_output_dir", &self.agent_output_dir)
            .field("model_call_fn", &self.model_call_fn.is_some())
            .field("parent_selections", &self.parent_selections.is_some())
            .finish_non_exhaustive()
    }
}

/// Session-scoped paths.
#[derive(Debug, Clone, Default)]
pub struct SessionPaths {
    /// Session directory for storing tool results.
    ///
    /// Large tool results (>400K chars by default) are persisted here with only
    /// a preview kept in context. Typical path: `~/.cocode/sessions/{session_id}/`
    pub session_dir: Option<PathBuf>,
    /// Path to the cocode home directory (e.g. `~/.cocode`).
    ///
    /// Used for durable cron persistence and other session-scoped file operations.
    pub cocode_home: Option<PathBuf>,
    /// Auto memory directory path (for write permission bypass).
    pub auto_memory_dir: Option<PathBuf>,
    /// Path to the current plan file (if in plan mode).
    pub plan_file_path: Option<PathBuf>,
    /// Whether cowork mode is active (disables memory write bypass).
    pub is_cowork_mode: bool,
}

/// Context for tool execution.
///
/// This provides everything a tool needs during execution:
/// - Call identification (call_id, turn_id, session_id, agent_id)
/// - Working directory and additional directories
/// - Permission mode and approvals
/// - Event channel for progress updates
/// - Cancellation support
/// - File tracking with content/timestamp validation
/// - Subagent spawning capability
/// - Plan mode state for Write/Edit permission checks
/// - Background task registry for Bash background execution
/// - LSP server manager for language intelligence
/// - Session directory for persisting large tool results
#[derive(Clone)]
pub struct ToolContext {
    /// Per-call identification.
    pub identity: ToolCallIdentity,
    /// Environment and configuration.
    pub env: ToolEnvironment,
    /// Event emission and cancellation channels.
    pub channels: ToolChannels,
    /// Shared mutable state across tool executions.
    pub state: ToolSharedState,
    /// Service handles for tool execution.
    pub services: ToolServices,
    /// Subagent-related context.
    pub agent: AgentContext,
    /// Session-scoped paths.
    pub paths: SessionPaths,
}

impl ToolContext {
    /// Create a new tool context.
    pub fn new(call_id: impl Into<String>, session_id: impl Into<String>, cwd: PathBuf) -> Self {
        let shell_executor = ShellExecutor::new(cwd.clone());
        Self {
            identity: ToolCallIdentity {
                call_id: call_id.into(),
                session_id: session_id.into(),
                turn_id: String::new(),
                turn_number: 0,
                agent_id: None,
            },
            env: ToolEnvironment {
                cwd,
                additional_working_directories: Vec::new(),
                permission_mode: PermissionMode::Default,
                features: Features::with_defaults(),
                web_search_config: WebSearchConfig::default(),
                web_fetch_config: WebFetchConfig::default(),
                task_type_restrictions: None,
                is_plan_mode: false,
                is_ultraplan: false,
            },
            channels: ToolChannels {
                event_tx: None,
                cancel_token: CancellationToken::new(),
            },
            state: ToolSharedState {
                approval_store: Arc::new(Mutex::new(ApprovalStore::new())),
                file_tracker: Arc::new(Mutex::new(FileTracker::new())),
                invoked_skills: Arc::new(Mutex::new(Vec::new())),
                output_offsets: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            },
            services: ToolServices {
                shell_executor,
                sandbox_state: None,
                lsp_manager: None,
                skill_manager: None,
                skill_usage_tracker: None,
                hook_registry: None,
                permission_requester: None,
                permission_evaluator: None,
                file_backup_store: None,
                question_responder: None,
            },
            agent: AgentContext {
                spawn_agent_fn: None,
                agent_cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
                killed_agents: Arc::new(Mutex::new(HashSet::new())),
                agent_output_dir: None,
                model_call_fn: None,
                parent_selections: None,
                team_name: None,
            },
            paths: SessionPaths::default(),
        }
    }

    /// Set the permission mode.
    pub fn with_permission_mode(mut self, mode: PermissionMode) -> Self {
        self.env.permission_mode = mode;
        self
    }

    /// Set the event channel.
    pub fn with_event_tx(mut self, tx: mpsc::Sender<CoreEvent>) -> Self {
        self.channels.event_tx = Some(tx);
        self
    }

    /// Set the cancellation token.
    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.channels.cancel_token = token;
        self
    }

    /// Set the approval store.
    pub fn with_approval_store(mut self, store: Arc<Mutex<ApprovalStore>>) -> Self {
        self.state.approval_store = store;
        self
    }

    /// Set the file tracker.
    pub fn with_file_tracker(mut self, tracker: Arc<Mutex<FileTracker>>) -> Self {
        self.state.file_tracker = tracker;
        self
    }

    /// Set the turn ID.
    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.identity.turn_id = turn_id.into();
        self
    }

    /// Set the turn number.
    pub fn with_turn_number(mut self, turn_number: i32) -> Self {
        self.identity.turn_number = turn_number;
        self
    }

    /// Set the agent ID.
    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.identity.agent_id = Some(agent_id.into());
        self
    }

    /// Set additional working directories.
    pub fn with_additional_working_directories(mut self, dirs: Vec<PathBuf>) -> Self {
        self.env.additional_working_directories = dirs;
        self
    }

    /// Set the spawn agent callback.
    pub fn with_spawn_agent_fn(mut self, f: SpawnAgentFn) -> Self {
        self.agent.spawn_agent_fn = Some(f);
        self
    }

    /// Set the shared agent cancel token registry.
    pub fn with_agent_cancel_tokens(mut self, tokens: AgentCancelTokens) -> Self {
        self.agent.agent_cancel_tokens = tokens;
        self
    }

    /// Set the shared killed agents registry.
    pub fn with_killed_agents(mut self, killed: KilledAgents) -> Self {
        self.agent.killed_agents = killed;
        self
    }

    /// Set the agent output directory.
    pub fn with_agent_output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.agent.agent_output_dir = Some(dir.into());
        self
    }

    /// Set the model call function for single-shot LLM calls.
    pub fn with_model_call_fn(mut self, f: ModelCallFn) -> Self {
        self.agent.model_call_fn = Some(f);
        self
    }

    /// Set plan mode state.
    pub fn with_plan_mode(mut self, is_active: bool, plan_file_path: Option<PathBuf>) -> Self {
        self.env.is_plan_mode = is_active;
        self.paths.plan_file_path = plan_file_path;
        self
    }

    /// Set the auto memory directory for write permission bypass.
    pub fn with_auto_memory_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.paths.auto_memory_dir = dir;
        self
    }

    /// Set the shell executor.
    pub fn with_shell_executor(mut self, executor: ShellExecutor) -> Self {
        self.services.shell_executor = executor;
        self
    }

    /// Set the sandbox state.
    pub fn with_sandbox_state(mut self, state: Arc<cocode_sandbox::SandboxState>) -> Self {
        self.services.sandbox_state = Some(state);
        self
    }

    /// Set the LSP server manager.
    pub fn with_lsp_manager(mut self, manager: Arc<LspServerManager>) -> Self {
        self.services.lsp_manager = Some(manager);
        self
    }

    /// Set the skill manager.
    pub fn with_skill_manager(mut self, manager: Arc<SkillManager>) -> Self {
        self.services.skill_manager = Some(manager);
        self
    }

    /// Set the skill usage tracker.
    pub fn with_skill_usage_tracker(mut self, tracker: Arc<SkillUsageTracker>) -> Self {
        self.services.skill_usage_tracker = Some(tracker);
        self
    }

    /// Set the hook registry.
    pub fn with_hook_registry(mut self, registry: Arc<HookRegistry>) -> Self {
        self.services.hook_registry = Some(registry);
        self
    }

    /// Set the session directory for persisting large tool results.
    pub fn with_session_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.paths.session_dir = Some(dir.into());
        self
    }

    /// Set the permission requester for interactive approval flow.
    pub fn with_permission_requester(mut self, requester: Arc<dyn PermissionRequester>) -> Self {
        self.services.permission_requester = Some(requester);
        self
    }

    /// Set the permission rule evaluator.
    pub fn with_permission_evaluator(mut self, evaluator: PermissionRuleEvaluator) -> Self {
        self.services.permission_evaluator = Some(evaluator);
        self
    }

    /// Set the question responder for AskUserQuestion tool.
    pub fn with_question_responder(mut self, responder: Arc<QuestionResponder>) -> Self {
        self.services.question_responder = Some(responder);
        self
    }

    /// Set the cocode home directory path.
    pub fn with_cocode_home(mut self, path: impl Into<PathBuf>) -> Self {
        self.paths.cocode_home = Some(path.into());
        self
    }

    /// Check if a write to the given path is allowed in plan mode.
    ///
    /// Returns `true` if not in plan mode, or if the path is the plan file.
    /// Returns `false` if in plan mode and the path is not the plan file.
    pub fn plan_mode_allows_write(&self, path: &Path) -> bool {
        if !self.env.is_plan_mode {
            return true;
        }
        cocode_plan_mode::is_safe_file(path, self.paths.plan_file_path.as_deref())
    }

    /// Check if a write to the given path should be auto-allowed
    /// because it's within the auto memory directory.
    pub fn auto_memory_allows_write(&self, path: &Path) -> bool {
        // Cowork mode disables memory write bypass to prevent
        // uncoordinated writes to shared remote directories.
        if self.paths.is_cowork_mode {
            return false;
        }
        self.paths
            .auto_memory_dir
            .as_deref()
            .is_some_and(|dir| cocode_auto_memory::is_auto_memory_path(path, dir))
    }

    /// Spawn a subagent using the configured callback.
    ///
    /// Returns an error if no spawn callback is configured.
    pub async fn spawn_agent(
        &self,
        input: SpawnAgentInput,
    ) -> std::result::Result<SpawnAgentResult, cocode_error::BoxedError> {
        let spawn_fn = self.agent.spawn_agent_fn.as_ref().ok_or_else(|| {
            cocode_error::boxed_err(
                crate::error::tool_error::InternalSnafu {
                    message: "No spawn_agent_fn configured".to_string(),
                }
                .build(),
            )
        })?;
        spawn_fn(input).await
    }

    /// Check if agent spawning is available.
    pub fn can_spawn_agent(&self) -> bool {
        self.agent.spawn_agent_fn.is_some()
    }

    /// Emit a core event.
    pub async fn emit_event(&self, event: CoreEvent) {
        if let Some(tx) = &self.channels.event_tx
            && let Err(e) = tx.send(event).await
        {
            debug!("Failed to emit event: {e}");
        }
    }

    /// Emit tool progress.
    pub async fn emit_progress(&self, message: impl Into<String>) {
        self.emit_event(CoreEvent::Tui(TuiEvent::ToolProgress {
            call_id: self.identity.call_id.clone(),
            progress: cocode_protocol::ToolProgressInfo {
                message: Some(message.into()),
                percentage: None,
                bytes_processed: None,
                total_bytes: None,
            },
        }))
        .await;
    }

    /// Emit tool progress with percentage.
    pub async fn emit_progress_percent(&self, message: impl Into<String>, percentage: i32) {
        self.emit_event(CoreEvent::Tui(TuiEvent::ToolProgress {
            call_id: self.identity.call_id.clone(),
            progress: cocode_protocol::ToolProgressInfo {
                message: Some(message.into()),
                percentage: Some(percentage),
                bytes_processed: None,
                total_bytes: None,
            },
        }))
        .await;
    }

    /// Check if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.channels.cancel_token.is_cancelled()
    }

    /// Wait for cancellation or completion.
    pub async fn cancelled(&self) {
        self.channels.cancel_token.cancelled().await
    }

    /// Record a file read (simple -- backward-compatible).
    pub async fn record_file_read(&self, path: impl Into<PathBuf>) {
        self.state.file_tracker.lock().await.record_read(path);
    }

    /// Record a file read with full state tracking.
    pub async fn record_file_read_with_state(
        &self,
        path: impl Into<PathBuf>,
        state: FileReadState,
    ) {
        self.state
            .file_tracker
            .lock()
            .await
            .record_read_with_state(path, state);
    }

    /// Register a file read with tool call ID for compaction cleanup.
    pub async fn register_file_read_id(&self, path: &Path) {
        let tracker = self.state.file_tracker.lock().await;
        tracker.register_tool_read(self.identity.call_id.clone(), path.to_path_buf());
    }

    /// Record a file modification.
    pub async fn record_file_modified(&self, path: impl Into<PathBuf>) {
        self.state.file_tracker.lock().await.record_modified(path);
    }

    /// Check if a file was read.
    pub async fn was_file_read(&self, path: &Path) -> bool {
        self.state.file_tracker.lock().await.was_read(path)
    }

    /// Get the read state for a file.
    pub async fn file_read_state(&self, path: &Path) -> Option<FileReadState> {
        self.state.file_tracker.lock().await.read_state(path)
    }

    /// Check if a file was modified.
    pub async fn was_file_modified(&self, path: &Path) -> bool {
        self.state.file_tracker.lock().await.was_modified(path)
    }

    /// Check if an action is approved.
    pub async fn is_approved(&self, tool_name: &str, pattern: &str) -> bool {
        self.state
            .approval_store
            .lock()
            .await
            .is_approved(tool_name, pattern)
    }

    /// Approve a specific pattern.
    pub async fn approve_pattern(&self, tool_name: &str, pattern: &str) {
        self.state
            .approval_store
            .lock()
            .await
            .approve_pattern(tool_name, pattern);
    }

    /// Approve a tool for the session.
    pub async fn approve_session(&self, tool_name: &str) {
        self.state
            .approval_store
            .lock()
            .await
            .approve_session(tool_name);
    }

    /// Persist a permission rule to `~/.cocode/settings.local.json`.
    ///
    /// Called when the user selects "Allow always" -- writes the pattern
    /// into `permissions.allow` so it's remembered across sessions.
    pub async fn persist_permission_rule(&self, tool_name: &str, pattern: &str) {
        let config_dir = cocode_config::default_config_dir();
        if let Err(e) = cocode_policy::persist_rule(&config_dir, tool_name, pattern).await {
            tracing::warn!("Failed to persist permission rule: {e}");
        }
    }

    /// Resolve a path relative to the working directory.
    pub fn resolve_path(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            self.env.cwd.join(path)
        }
    }
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("call_id", &self.identity.call_id)
            .field("session_id", &self.identity.session_id)
            .field("turn_id", &self.identity.turn_id)
            .field("agent_id", &self.identity.agent_id)
            .field("cwd", &self.env.cwd)
            .field("permission_mode", &self.env.permission_mode)
            .field("is_cancelled", &self.is_cancelled())
            .field("is_plan_mode", &self.env.is_plan_mode)
            .field("plan_file_path", &self.paths.plan_file_path)
            .field("auto_memory_dir", &self.paths.auto_memory_dir)
            .field("lsp_manager", &self.services.lsp_manager.is_some())
            .field("skill_manager", &self.services.skill_manager.is_some())
            .field("session_dir", &self.paths.session_dir)
            .field(
                "permission_requester",
                &self.services.permission_requester.is_some(),
            )
            .field(
                "permission_evaluator",
                &self.services.permission_evaluator.is_some(),
            )
            .finish_non_exhaustive()
    }
}

/// Builder for creating tool contexts.
///
/// Uses sub-structs directly for bulk configuration, with individual setters
/// for per-call identity fields.
pub struct ToolContextBuilder {
    identity: ToolCallIdentity,
    env: ToolEnvironment,
    channels: ToolChannels,
    state: ToolSharedState,
    services: ToolServices,
    agent: AgentContext,
    paths: SessionPaths,
    /// Shell executor override (if None, one is created from `env.cwd` in build).
    shell_executor_override: Option<ShellExecutor>,
}

impl ToolContextBuilder {
    /// Create a new builder with minimal identity fields.
    pub fn new(call_id: impl Into<String>, session_id: impl Into<String>) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        Self {
            identity: ToolCallIdentity {
                call_id: call_id.into(),
                session_id: session_id.into(),
                turn_id: String::new(),
                turn_number: 0,
                agent_id: None,
            },
            env: ToolEnvironment {
                cwd,
                additional_working_directories: Vec::new(),
                permission_mode: PermissionMode::Default,
                features: Features::with_defaults(),
                web_search_config: WebSearchConfig::default(),
                web_fetch_config: WebFetchConfig::default(),
                task_type_restrictions: None,
                is_plan_mode: false,
                is_ultraplan: false,
            },
            channels: ToolChannels {
                event_tx: None,
                cancel_token: CancellationToken::new(),
            },
            state: ToolSharedState {
                approval_store: Arc::new(Mutex::new(ApprovalStore::new())),
                file_tracker: Arc::new(Mutex::new(FileTracker::new())),
                invoked_skills: Arc::new(Mutex::new(Vec::new())),
                output_offsets: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            },
            services: ToolServices {
                shell_executor: ShellExecutor::new(PathBuf::from("/")),
                sandbox_state: None,
                lsp_manager: None,
                skill_manager: None,
                skill_usage_tracker: None,
                hook_registry: None,
                permission_requester: None,
                permission_evaluator: None,
                file_backup_store: None,
                question_responder: None,
            },
            agent: AgentContext {
                spawn_agent_fn: None,
                agent_cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
                killed_agents: Arc::new(Mutex::new(HashSet::new())),
                agent_output_dir: None,
                model_call_fn: None,
                parent_selections: None,
                team_name: None,
            },
            paths: SessionPaths::default(),
            shell_executor_override: None,
        }
    }

    // --- Per-call identity setters (always set individually) ---

    /// Set the working directory.
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.env.cwd = cwd.into();
        self
    }

    /// Set the turn ID.
    pub fn turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.identity.turn_id = turn_id.into();
        self
    }

    /// Set the turn number.
    pub fn turn_number(mut self, turn_number: i32) -> Self {
        self.identity.turn_number = turn_number;
        self
    }

    /// Set the agent ID.
    pub fn agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.identity.agent_id = Some(agent_id.into());
        self
    }

    /// Set the event channel.
    pub fn event_tx(mut self, tx: mpsc::Sender<CoreEvent>) -> Self {
        self.channels.event_tx = Some(tx);
        self
    }

    /// Set the cancellation token.
    pub fn cancel_token(mut self, token: CancellationToken) -> Self {
        self.channels.cancel_token = token;
        self
    }

    // --- Sub-struct bulk setters ---

    /// Set the full environment configuration.
    pub fn environment(mut self, env: ToolEnvironment) -> Self {
        self.env = env;
        self
    }

    /// Set shared mutable state (approval store, file tracker, etc.).
    pub fn shared_state(mut self, state: ToolSharedState) -> Self {
        self.state = state;
        self
    }

    /// Set service handles (shell, sandbox, LSP, skills, etc.).
    pub fn tool_services(mut self, services: ToolServices) -> Self {
        self.services = services;
        self
    }

    /// Set agent context (spawn fn, cancel tokens, etc.).
    pub fn agent_context(mut self, agent: AgentContext) -> Self {
        self.agent = agent;
        self
    }

    /// Set session paths.
    pub fn session_paths(mut self, paths: SessionPaths) -> Self {
        self.paths = paths;
        self
    }

    // --- Individual field setters for common overrides ---

    /// Set additional working directories.
    pub fn additional_working_directories(mut self, dirs: Vec<PathBuf>) -> Self {
        self.env.additional_working_directories = dirs;
        self
    }

    /// Set the permission mode.
    pub fn permission_mode(mut self, mode: PermissionMode) -> Self {
        self.env.permission_mode = mode;
        self
    }

    /// Set the approval store.
    pub fn approval_store(mut self, store: Arc<Mutex<ApprovalStore>>) -> Self {
        self.state.approval_store = store;
        self
    }

    /// Set the file tracker.
    pub fn file_tracker(mut self, tracker: Arc<Mutex<FileTracker>>) -> Self {
        self.state.file_tracker = tracker;
        self
    }

    /// Set the spawn agent callback.
    pub fn spawn_agent_fn(mut self, f: SpawnAgentFn) -> Self {
        self.agent.spawn_agent_fn = Some(f);
        self
    }

    /// Set the shared agent cancel token registry.
    pub fn agent_cancel_tokens(mut self, tokens: AgentCancelTokens) -> Self {
        self.agent.agent_cancel_tokens = tokens;
        self
    }

    /// Set the shared killed agents registry.
    pub fn killed_agents(mut self, killed: KilledAgents) -> Self {
        self.agent.killed_agents = killed;
        self
    }

    /// Set the agent output directory.
    pub fn agent_output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.agent.agent_output_dir = Some(dir.into());
        self
    }

    /// Set the model call function for single-shot LLM calls.
    pub fn model_call_fn(mut self, f: ModelCallFn) -> Self {
        self.agent.model_call_fn = Some(f);
        self
    }

    /// Set plan mode state.
    pub fn plan_mode(mut self, is_active: bool, plan_file_path: Option<PathBuf>) -> Self {
        self.env.is_plan_mode = is_active;
        self.paths.plan_file_path = plan_file_path;
        self
    }

    /// Set whether this is an ultraplan session.
    pub fn is_ultraplan(mut self, is_ultraplan: bool) -> Self {
        self.env.is_ultraplan = is_ultraplan;
        self
    }

    /// Set the auto memory directory for write permission bypass.
    pub fn auto_memory_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.paths.auto_memory_dir = dir;
        self
    }

    /// Set the shell executor.
    pub fn shell_executor(mut self, executor: ShellExecutor) -> Self {
        self.shell_executor_override = Some(executor);
        self
    }

    /// Set the sandbox state.
    pub fn sandbox_state(mut self, state: Arc<cocode_sandbox::SandboxState>) -> Self {
        self.services.sandbox_state = Some(state);
        self
    }

    /// Set the sandbox state from an Option (no-op if None).
    pub fn maybe_sandbox_state(mut self, state: Option<Arc<cocode_sandbox::SandboxState>>) -> Self {
        self.services.sandbox_state = state;
        self
    }

    /// Set the LSP server manager.
    pub fn lsp_manager(mut self, manager: Arc<LspServerManager>) -> Self {
        self.services.lsp_manager = Some(manager);
        self
    }

    /// Set the skill manager.
    pub fn skill_manager(mut self, manager: Arc<SkillManager>) -> Self {
        self.services.skill_manager = Some(manager);
        self
    }

    /// Set the skill usage tracker.
    pub fn skill_usage_tracker(mut self, tracker: Arc<SkillUsageTracker>) -> Self {
        self.services.skill_usage_tracker = Some(tracker);
        self
    }

    /// Set the hook registry.
    pub fn hook_registry(mut self, registry: Arc<HookRegistry>) -> Self {
        self.services.hook_registry = Some(registry);
        self
    }

    /// Set a shared invoked skills tracker.
    pub fn invoked_skills(mut self, skills: Arc<Mutex<Vec<InvokedSkill>>>) -> Self {
        self.state.invoked_skills = skills;
        self
    }

    /// Set the session directory for persisting large tool results.
    pub fn session_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.paths.session_dir = Some(dir.into());
        self
    }

    /// Set parent selections for subagent isolation.
    pub fn parent_selections(mut self, selections: RoleSelections) -> Self {
        self.agent.parent_selections = Some(selections);
        self
    }

    /// Set the team name for team-aware tool behavior.
    pub fn team_name(mut self, name: String) -> Self {
        self.agent.team_name = Some(name);
        self
    }

    /// Set the permission requester for interactive approval flow.
    pub fn permission_requester(mut self, requester: Arc<dyn PermissionRequester>) -> Self {
        self.services.permission_requester = Some(requester);
        self
    }

    /// Set the permission rule evaluator.
    pub fn permission_evaluator(mut self, evaluator: PermissionRuleEvaluator) -> Self {
        self.services.permission_evaluator = Some(evaluator);
        self
    }

    /// Set the feature flags.
    pub fn features(mut self, features: Features) -> Self {
        self.env.features = features;
        self
    }

    /// Set the web search configuration.
    pub fn web_search_config(mut self, config: WebSearchConfig) -> Self {
        self.env.web_search_config = config;
        self
    }

    /// Set the web fetch configuration.
    pub fn web_fetch_config(mut self, config: WebFetchConfig) -> Self {
        self.env.web_fetch_config = config;
        self
    }

    /// Set the file backup store for pre-modify snapshots.
    pub fn file_backup_store(mut self, store: Arc<cocode_file_backup::FileBackupStore>) -> Self {
        self.services.file_backup_store = Some(store);
        self
    }

    /// Set the question responder for AskUserQuestion tool.
    pub fn question_responder(mut self, responder: Arc<QuestionResponder>) -> Self {
        self.services.question_responder = Some(responder);
        self
    }

    /// Set the cocode home directory path.
    pub fn cocode_home(mut self, path: impl Into<PathBuf>) -> Self {
        self.paths.cocode_home = Some(path.into());
        self
    }

    /// Set the shared output offsets for delta reads.
    pub fn output_offsets(
        mut self,
        offsets: Arc<tokio::sync::Mutex<HashMap<String, u64>>>,
    ) -> Self {
        self.state.output_offsets = offsets;
        self
    }

    /// Set allowed subagent types for the Task tool.
    pub fn task_type_restrictions(mut self, restrictions: Vec<String>) -> Self {
        self.env.task_type_restrictions = Some(restrictions);
        self
    }

    /// Build the context.
    pub fn build(mut self) -> ToolContext {
        if let Some(executor) = self.shell_executor_override {
            self.services.shell_executor = executor;
        } else {
            self.services.shell_executor = ShellExecutor::new(self.env.cwd.clone());
        }
        ToolContext {
            identity: self.identity,
            env: self.env,
            channels: self.channels,
            state: self.state,
            services: self.services,
            agent: self.agent,
            paths: self.paths,
        }
    }
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
