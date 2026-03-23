//! Session state aggregate that wires together all components.
//!
//! [`SessionState`] is the main runtime container for an active session,
//! holding references to the API client, tool registry, hooks, and message history.

use std::sync::Arc;

use cocode_api::ApiClient;
use cocode_api::ModelHub;
use cocode_config::Config;
use cocode_context::ContextInjection;
use cocode_context::ConversationContext;
use cocode_context::EnvironmentInfo;
use cocode_context::InjectionPosition;
use cocode_hooks::HookDefinition;
use cocode_hooks::HookHandler;
use cocode_hooks::HookRegistry;
use cocode_hooks::HookSource;
use cocode_loop::AgentLoop;
use cocode_loop::FallbackConfig;
use cocode_loop::LoopConfig;
use cocode_loop::LoopResult;
use cocode_loop::StopReason;
use cocode_message::MessageHistory;
use cocode_plan_mode::PlanModeState;
use cocode_protocol::Feature;
use cocode_protocol::LoopEvent;
use cocode_protocol::PermissionMode;
use cocode_protocol::ProviderApi;
use cocode_protocol::RoleSelection;
use cocode_protocol::RoleSelections;
use cocode_protocol::SubagentType;
use cocode_protocol::ThinkingLevel;
use cocode_protocol::TokenUsage;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelSpec;
use cocode_rmcp_client::RmcpClient;
use cocode_shell::ShellExecutor;
use cocode_skill::SkillInterface;
use cocode_skill::SkillManager;
use cocode_subagent::AgentExecuteParams;
use cocode_subagent::AgentStatus as SubagentStatus;
use cocode_subagent::IsolationMode;
use cocode_subagent::MemoryScope;
use cocode_subagent::SubagentManager;
use cocode_system_reminder::BackgroundTaskInfo;
use cocode_system_reminder::BackgroundTaskStatus;
use cocode_system_reminder::BackgroundTaskType;
use cocode_system_reminder::QueuedCommandInfo;
use cocode_tools::ToolRegistry;

use std::sync::Mutex;

use serde::Deserialize;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::info;

use cocode_error::StatusCode;
use cocode_error::boxed_err;
use cocode_error::stack_trace_debug;
use snafu::ResultExt;
use snafu::Snafu;

use crate::session::Session;

#[stack_trace_debug]
#[derive(Snafu)]
#[snafu(visibility(pub(crate)), module)]
enum SessionStateError {
    #[snafu(display("Invalid model spec '{model_name}'"))]
    InvalidModelSpec {
        model_name: String,
        #[snafu(source)]
        error: cocode_protocol::ModelSpecParseError,
        #[snafu(implicit)]
        location: cocode_error::Location,
    },
}

impl cocode_error::ErrorExt for SessionStateError {
    fn status_code(&self) -> StatusCode {
        match self {
            SessionStateError::InvalidModelSpec { .. } => StatusCode::InvalidArguments,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Result of a single turn in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnResult {
    /// Final text response from the model.
    pub final_text: String,

    /// Number of turns completed so far.
    pub turns_completed: i32,

    /// Token usage for this turn.
    pub usage: TokenUsage,

    /// Whether the model requested more tool calls.
    pub has_pending_tools: bool,

    /// Whether the loop completed (model stop signal).
    pub is_complete: bool,

    /// The reason the loop stopped (preserved for plan mode exit handling).
    pub stop_reason: StopReason,
}

impl TurnResult {
    /// Create a turn result from a loop result.
    pub fn from_loop_result(result: &LoopResult) -> Self {
        Self {
            final_text: result.final_text.clone(),
            turns_completed: result.turns_completed,
            usage: TokenUsage::new(
                result.total_input_tokens as i64,
                result.total_output_tokens as i64,
            ),
            has_pending_tools: false,
            is_complete: true,
            stop_reason: result.stop_reason.clone(),
        }
    }
}

/// Result of a partial compaction (summarize from a user-selected turn).
#[derive(Debug, Clone)]
pub struct PartialCompactResult {
    /// Turn number from which summarization was requested.
    pub from_turn: i32,
    /// Estimated token count of the summary that was generated.
    pub summary_tokens: i32,
}

/// Session state aggregate for an active conversation.
///
/// This struct holds all the runtime components needed to drive a conversation:
/// - Session metadata
/// - Message history
/// - Tool registry
/// - Hook registry
/// - Skills
/// - API client
/// - Cancellation token
///
/// # Example
///
/// ```ignore
/// use cocode_session::{Session, SessionState};
/// use cocode_config::{ConfigManager, ConfigOverrides};
/// use cocode_protocol::ProviderApi;
/// use std::sync::Arc;
/// use std::path::PathBuf;
///
/// let session = Session::new(PathBuf::from("."), "gpt-5", ProviderApi::Openai);
/// let manager = ConfigManager::from_default()?;
/// let config = Arc::new(manager.build_config(ConfigOverrides::default())?);
/// let mut state = SessionState::new(session, config).await?;
///
/// // Run a turn
/// let result = state.run_turn("Hello!").await?;
/// println!("Response: {}", result.final_text);
///
/// // Cancel if needed
/// state.cancel();
/// ```
pub struct SessionState {
    /// Session metadata.
    pub session: Session,

    /// Message history for the conversation.
    pub message_history: MessageHistory,

    /// Tool registry (built-in + MCP tools).
    pub tool_registry: Arc<ToolRegistry>,

    /// Hook registry for event interception.
    pub hook_registry: Arc<HookRegistry>,

    /// Loaded skills.
    pub skills: Vec<SkillInterface>,

    /// Skill manager for loading and executing skills.
    skill_manager: Arc<SkillManager>,

    /// Plugin registry for tracking loaded plugins.
    plugin_registry: Option<cocode_plugin::PluginRegistry>,

    /// API client for model inference.
    api_client: ApiClient,

    /// Model hub for model acquisition and caching.
    ///
    /// Note: ModelHub is role-agnostic. Role selections are stored in
    /// `self.session.selections` and passed to ModelHub methods as parameters.
    model_hub: Arc<ModelHub>,

    // NOTE: Role selections are stored in `self.session.selections` (single source of truth).
    // This enables proper persistence when the session is saved.
    /// Cancellation token for graceful shutdown.
    cancel_token: CancellationToken,

    /// Loop configuration.
    loop_config: LoopConfig,

    /// Total turns run.
    total_turns: i32,

    /// Total input tokens consumed.
    total_input_tokens: i32,

    /// Total output tokens generated.
    total_output_tokens: i32,

    /// Context window size for the model.
    context_window: i32,

    /// Provider type for the current session.
    api: ProviderApi,

    /// Shell executor for command execution and background tasks.
    shell_executor: ShellExecutor,

    /// Queued commands for real-time steering (Enter during streaming).
    /// Shared via `Arc<Mutex>` with the running `AgentLoop` so the TUI driver
    /// can push commands while a turn is executing. Drained once per iteration
    /// in Step 6.5 and injected as steering system-reminders.
    queued_commands: Arc<Mutex<Vec<QueuedCommandInfo>>>,

    /// Optional suffix appended to the end of the system prompt.
    system_prompt_suffix: Option<String>,

    /// Subagent manager for Task tool agent spawning.
    ///
    /// Wrapped in `Arc<tokio::sync::Mutex>` so the `SpawnAgentFn` closure
    /// (which needs `&mut SubagentManager`) can be called concurrently
    /// with the session state.
    subagent_manager: Arc<tokio::sync::Mutex<SubagentManager>>,

    /// Active MCP clients from plugin servers (kept alive for session lifetime).
    _mcp_clients: Vec<Arc<RmcpClient>>,

    /// LSP server manager for language intelligence tools.
    lsp_manager: Arc<cocode_lsp::LspServerManager>,

    /// Configuration snapshot (immutable for session lifetime).
    config: Arc<Config>,

    /// Pre-configured permission rules loaded from config.
    permission_rules: Vec<cocode_policy::PermissionRule>,

    /// Current task list (updated by TodoWrite tool via ContextModifier).
    todos: serde_json::Value,

    /// Current structured tasks (updated by TaskCreate/TaskUpdate via ContextModifier).
    structured_tasks: serde_json::Value,

    /// Current cron jobs (updated by CronCreate/CronDelete via ContextModifier).
    cron_jobs: serde_json::Value,

    /// Optional OTel manager for metrics and traces.
    otel_manager: Option<Arc<cocode_otel::OtelManager>>,

    /// Runtime output style override.
    /// `None` = use config default, `Some(None)` = disabled, `Some(Some(name))` = active style.
    output_style_override: Option<Option<String>>,

    /// Output styles contributed by plugins.
    ///
    /// Checked as a fallback when `find_output_style()` doesn't find a
    /// built-in or custom style. Populated from `PluginRegistry::output_style_contributions()`.
    plugin_output_styles: Vec<(String, String)>,

    /// Optional snapshot manager for rewind support (file backups + ghost commits).
    snapshot_manager: Option<Arc<cocode_file_backup::SnapshotManager>>,

    /// Plan mode state persisted across AgentLoop runs.
    ///
    /// Each `run_turn_streaming()` creates a new `AgentLoop`, so plan mode
    /// state must live here and be passed to/extracted from the loop.
    plan_mode_state: PlanModeState,

    /// Question responder for AskUserQuestion tool.
    ///
    /// Shared across turns so the TUI driver can send responses that
    /// unblock the AskUserQuestion tool's oneshot channel.
    question_responder: Arc<cocode_tools::QuestionResponder>,

    /// Shared approval store for tool permissions (persists across turns).
    ///
    /// Pre-declared permissions from `ExitPlanMode`'s `allowedPrompts` are
    /// injected here so they survive across `AgentLoop` instances.
    shared_approval_store: Arc<tokio::sync::Mutex<cocode_policy::ApprovalStore>>,

    /// Reminder file tracker state persisted across AgentLoop runs.
    ///
    /// Each `run_turn_streaming()` creates a new `AgentLoop`, so the file tracker
    /// state must be extracted after each run and passed to the next one.
    /// This enables proper already-read detection across turns.
    ///
    /// The state is a list of (path, read_state) tuples representing files that
    /// have been read and their content/modification state.
    reminder_file_tracker_state: Vec<(std::path::PathBuf, cocode_tools::FileReadState)>,

    /// Shared set of agent IDs killed via TaskStop (persists across turns).
    ///
    /// When TaskStop cancels a background agent, the agent_id is inserted here.
    /// `collect_background_agent_tasks()` checks this set to report the agent
    /// as `Killed` rather than `Failed`.
    killed_agents: cocode_tools::context::KilledAgents,

    /// Auto memory state for the session (persists across turns).
    auto_memory_state: Arc<cocode_auto_memory::AutoMemoryState>,

    /// Team store for querying team membership.
    team_store: Arc<cocode_team::TeamStore>,

    /// Team mailbox for querying unread messages.
    team_mailbox: Arc<cocode_team::Mailbox>,
}

impl SessionState {
    /// Create a new session state from a session and configuration.
    ///
    /// This initializes all components including:
    /// - API client from the resolved provider/model
    /// - Tool registry with built-in tools
    /// - Hook registry (empty by default)
    /// - Skills (loaded from project/user directories)
    pub async fn new(session: Session, config: Arc<Config>) -> anyhow::Result<Self> {
        // Get the primary model info from session
        let primary_model = session
            .primary_model()
            .ok_or_else(|| anyhow::anyhow!("Session has no main model configured"))?;
        let provider_name = primary_model.provider().to_string();
        let model_name = primary_model.model_name().to_string();

        info!(
            session_id = %session.id,
            model = %model_name,
            provider = %provider_name,
            "Creating session state"
        );

        // Get provider API from session's ModelSpec.
        // IMPORTANT: This assumes the caller used ModelSpec::with_type() (not ModelSpec::new())
        // so that provider_api comes from config, not from string-based heuristic resolution.
        // All current callers (tui_runner, chat, session manager) satisfy this requirement.
        let api = primary_model.model.api;

        // Get model context window from Config snapshot (default to 200k)
        let context_window = config
            .resolve_model_info(&provider_name, &model_name)
            .and_then(|info| info.context_window)
            .map(|cw| cw as i32)
            .unwrap_or(200_000);

        // Create API client
        let api_client = ApiClient::new();
        let mut session = session;

        // Create ModelHub with Config snapshot (role-agnostic, just for model caching)
        let model_hub = Arc::new(ModelHub::new(config.clone()));

        // Create team stores and load persisted state from disk
        let (team_store, team_mailbox) = cocode_tools::builtin::create_default_team_stores();
        if let Err(e) = team_store.load_from_disk().await {
            tracing::warn!(error = %e, "Failed to load persisted teams from disk");
        }

        // Create tool registry with built-in tools
        let mut tool_registry = ToolRegistry::new();
        let builtin_stores = cocode_tools::builtin::register_builtin_tools(
            &mut tool_registry,
            &config.features,
            Arc::clone(&team_store),
            Arc::clone(&team_mailbox),
        );

        // Resolve auto memory configuration and create session state
        let resolved_auto_memory = cocode_auto_memory::resolve_auto_memory_config(
            &session.working_dir,
            &config.auto_memory_config,
            config.features.enabled(Feature::AutoMemory),
            config.features.enabled(Feature::RelevantMemories),
            config.features.enabled(Feature::MemoryExtraction),
        );
        let auto_memory_state = cocode_auto_memory::AutoMemoryState::new_arc(resolved_auto_memory);

        // Load durable cron jobs from disk and merge into the shared store
        if let Ok(durable_jobs) =
            cocode_tools::builtin::cron_state::load_durable_jobs(&config.cocode_home).await
            && !durable_jobs.is_empty()
        {
            let mut store = builtin_stores.cron_store.lock().await;
            for (id, job) in durable_jobs {
                store.insert(id, job);
            }
            tracing::info!(count = store.len(), "Loaded durable cron jobs from disk");
        }

        // Create hook registry and load hooks via aggregator (respects disable/managed-only)
        let hook_registry = HookRegistry::new();
        let hook_settings = cocode_hooks::HookSettings {
            disable_all_hooks: config.disable_all_hooks,
            allow_managed_hooks_only: config.allow_managed_hooks_only,
            workspace_trusted: true,
        };

        let mut aggregator = cocode_hooks::HookAggregator::new();

        // Session hooks from config.json
        let config_hooks = convert_config_hooks(&config.hooks);
        if !config_hooks.is_empty() {
            tracing::info!(count = config_hooks.len(), "Loaded hooks from config");
            aggregator.add_session_hooks(config_hooks);
        }

        // Session hooks from hooks.json
        let hooks_json_path = config.cocode_home.join("hooks.json");
        if hooks_json_path.is_file() {
            match cocode_hooks::load_hooks_from_json(&hooks_json_path) {
                Ok(json_hooks) => {
                    tracing::info!(
                        count = json_hooks.len(),
                        path = %hooks_json_path.display(),
                        "Loaded hooks from JSON"
                    );
                    aggregator.add_session_hooks(json_hooks);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load hooks.json");
                }
            }
        }

        // Build with settings (filters disabled/managed-only, sorts by priority)
        let aggregated = aggregator.build(&hook_settings);
        if !aggregated.is_empty() {
            tracing::info!(
                count = aggregated.len(),
                "Registering hooks after aggregation"
            );
            hook_registry.register_all(aggregated);
        }

        // Load skills (empty for now, can be populated later)
        let skills = Vec::new();

        // Create skill manager and load skills from standard directories
        let mut skill_manager = SkillManager::with_bundled();
        let mut skill_roots = Vec::new();
        // Project-local skills: <working_dir>/.cocode/skills/
        let project_skills = session.working_dir.join(".cocode").join("skills");
        if project_skills.is_dir() {
            skill_roots.push(project_skills);
        }
        // User-global skills: ~/.cocode/skills/
        let user_skills = config.cocode_home.join("skills");
        if user_skills.is_dir() {
            skill_roots.push(user_skills);
        }
        if !skill_roots.is_empty() {
            skill_manager.load_from_roots(&skill_roots);
        }

        // Create subagent manager and load builtin + custom agents
        let auto_background_timeout = std::env::var("COCODE_AUTO_BACKGROUND_TASKS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(std::time::Duration::from_secs);
        let mut subagent_manager = SubagentManager::new()
            .with_auto_background_timeout(auto_background_timeout)
            .with_auto_memory_state(Arc::clone(&auto_memory_state));
        let mut agent_defs =
            cocode_subagent::all_agents(&config.cocode_home, Some(session.working_dir.as_path()));

        // Merge CLI agents (highest priority) from --agents flag
        if let Ok(cli_json) = std::env::var("COCODE_CLI_AGENTS")
            && let Ok(cli_agents) =
                serde_json::from_str::<Vec<cocode_subagent::AgentDefinition>>(&cli_json)
        {
            cocode_subagent::merge_custom_agents(&mut agent_defs, cli_agents);
        }

        for def in agent_defs {
            subagent_manager.register_agent_type(def);
        }

        // Load plugins from standard directories and installed plugin cache
        let mut plugin_config = cocode_plugin::PluginIntegrationConfig::with_defaults(
            &config.cocode_home,
            Some(&session.working_dir),
        );

        // Pick up --plugin-dir flags passed via env var from CLI
        if let Ok(dirs_json) = std::env::var("COCODE_PLUGIN_DIRS")
            && let Ok(dirs) = serde_json::from_str::<Vec<std::path::PathBuf>>(&dirs_json)
            && !dirs.is_empty()
        {
            tracing::info!(count = dirs.len(), "Loading inline plugin directories");
            plugin_config = plugin_config.with_inline_dirs(dirs);
        }

        // Pass config-level enabled_plugins overrides (from user/project/local settings)
        if !config.enabled_plugins.is_empty() {
            plugin_config =
                plugin_config.with_config_enabled_plugins(config.enabled_plugins.clone());
        }

        // Pass extra marketplace sources from project settings
        if !config.extra_known_marketplaces.is_empty() {
            let extras = convert_extra_marketplaces(&config.extra_known_marketplaces);
            if !extras.is_empty() {
                tracing::info!(
                    count = extras.len(),
                    "Registering extra marketplaces from config"
                );
                plugin_config = plugin_config.with_extra_known_marketplaces(extras);
            }
        }

        let plugin_result = cocode_plugin::integrate_plugins(
            &plugin_config,
            &mut skill_manager,
            &hook_registry,
            Some(&mut subagent_manager),
        );

        // Apply default agent override from plugin settings.json
        let plugin_agent_suffix = if let Some(ref agent_type) = plugin_result.default_agent {
            if let Some(def) = subagent_manager
                .definitions()
                .iter()
                .find(|d| d.agent_type == *agent_type)
            {
                tracing::info!(
                    agent = agent_type,
                    identity = ?def.identity,
                    has_reminder = def.critical_reminder.is_some(),
                    "Applying plugin default agent to session"
                );

                // Override model selection if the agent specifies an identity
                match &def.identity {
                    Some(cocode_protocol::execution::ExecutionIdentity::Spec(spec)) => {
                        session.selections.set(
                            cocode_protocol::ModelRole::Main,
                            cocode_protocol::RoleSelection::new(spec.clone()),
                        );
                    }
                    Some(cocode_protocol::execution::ExecutionIdentity::Role(role)) => {
                        // Copy the role's selection to Main if different
                        if *role != cocode_protocol::ModelRole::Main
                            && let Some(sel) = session.selections.get(*role).cloned()
                        {
                            session
                                .selections
                                .set(cocode_protocol::ModelRole::Main, sel);
                        }
                    }
                    _ => {} // Inherit or None: keep current model
                }

                // Override max_turns if the agent specifies one
                if let Some(max_turns) = def.max_turns {
                    session.max_turns = Some(max_turns);
                }

                // Return critical_reminder to set as system_prompt_suffix later
                def.critical_reminder.clone()
            } else {
                tracing::warn!(
                    agent = agent_type,
                    "Plugin default agent not found in registered definitions"
                );
                None
            }
        } else {
            None
        };

        // Extract plugin output styles before consuming the registry
        let plugin_output_styles: Vec<(String, String)> = plugin_result
            .registry
            .output_style_contributions()
            .iter()
            .map(|(style, _plugin)| (style.name.clone(), style.prompt.clone()))
            .collect();

        let plugin_registry = if plugin_result.registry.is_empty() {
            None
        } else {
            Some(plugin_result.registry)
        };

        // Spawn best-effort background cache GC (fire-and-forget)
        if let Some(ref plugins_dir) = plugin_config.plugins_dir {
            let dir = plugins_dir.clone();
            tokio::spawn(async move {
                match cocode_plugin::cleanup_orphaned_cache(
                    &dir,
                    cocode_plugin::DEFAULT_CACHE_GRACE_PERIOD,
                ) {
                    Ok(removed) if removed > 0 => {
                        tracing::debug!(removed, "Plugin cache GC completed");
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "Plugin cache GC failed (non-fatal)");
                    }
                    _ => {}
                }
            });
        }

        // Connect plugin MCP servers (async: starts server processes and registers tools)
        let mcp_clients = if let Some(ref pr) = plugin_registry {
            cocode_plugin::connect_plugin_mcp_servers(pr, &mut tool_registry, &config.cocode_home)
                .await
        } else {
            Vec::new()
        };

        // Create LSP server manager and connect plugin-contributed LSP servers
        let lsp_manager = cocode_lsp::create_manager(
            Some(&config.cocode_home),
            Some(session.working_dir.clone()),
        );
        if let Some(ref pr) = plugin_registry {
            cocode_plugin::connect_plugin_lsp_servers(pr, &lsp_manager).await;
        }

        // Build loop config from session
        let loop_config = LoopConfig {
            max_turns: session.max_turns,
            ..LoopConfig::default()
        };

        // Load permission rules from config snapshot
        let permission_rules = match config.permissions {
            Some(ref perms) => cocode_policy::PermissionRuleEvaluator::rules_from_config(
                perms,
                cocode_protocol::RuleSource::User,
            ),
            None => Vec::new(),
        };

        // Create shell executor with default shell and start snapshotting
        let mut shell_executor = ShellExecutor::with_default_shell(session.working_dir.clone());
        shell_executor.start_snapshotting(config.cocode_home.clone(), &session.id.to_string());

        // Create OTel manager if OTel is configured
        let otel_manager = config.otel.as_ref().map(|_| {
            let mgr = Arc::new(cocode_otel::OtelManager::new(
                &session.id,
                &provider_name,
                &model_name,
                None,
                None,
                None,
                false,
                "tui".to_string(),
                "session",
            ));
            // Record session start events
            mgr.counter(
                "cocode.session.started",
                1,
                &[("provider", &provider_name), ("model", &model_name)],
            );
            mgr.conversation_starts(
                &provider_name,
                None,
                "",
                Some(context_window as i64),
                "default",
                &format!("{:?}", config.sandbox_mode),
                vec![],
                config.active_profile.clone(),
            );
            mgr
        });

        // Create snapshot manager for rewind support (file backups + ghost commits)
        let snapshot_manager = if config
            .features
            .enabled(cocode_protocol::Feature::FileCheckpointing)
        {
            let sessions_dir = config.cocode_home.join("sessions");
            match cocode_file_backup::FileBackupStore::new(&sessions_dir, &session.id).await {
                Ok(backup_store) => {
                    let is_git = detect_git_repo(&session.working_dir);
                    let sm = cocode_file_backup::SnapshotManager::new(
                        Arc::new(backup_store),
                        session.working_dir.clone(),
                        is_git,
                        cocode_file_backup::GhostConfig::default(),
                    );
                    Some(Arc::new(sm))
                }
                Err(e) => {
                    tracing::warn!("Failed to create file backup store: {e}");
                    None
                }
            }
        } else {
            tracing::debug!("File checkpointing disabled by feature flag");
            None
        };

        let state_result = Ok(Self {
            session,
            message_history: MessageHistory::new(),
            tool_registry: Arc::new(tool_registry),
            hook_registry: Arc::new(hook_registry),
            skills,
            skill_manager: Arc::new(skill_manager),
            plugin_registry,
            api_client,
            model_hub,
            subagent_manager: Arc::new(tokio::sync::Mutex::new(subagent_manager)),
            _mcp_clients: mcp_clients,
            lsp_manager,
            cancel_token: CancellationToken::new(),
            loop_config,
            total_turns: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            context_window,
            api,
            shell_executor,
            queued_commands: Arc::new(Mutex::new(Vec::new())),
            system_prompt_suffix: plugin_agent_suffix,
            config,
            permission_rules,
            todos: serde_json::json!([]),
            structured_tasks: serde_json::json!({}),
            cron_jobs: serde_json::json!({}),
            otel_manager,
            output_style_override: None,
            plugin_output_styles,
            snapshot_manager,
            plan_mode_state: PlanModeState::new(),
            question_responder: Arc::new(cocode_tools::QuestionResponder::new()),
            shared_approval_store: Arc::new(tokio::sync::Mutex::new(
                cocode_policy::ApprovalStore::new(),
            )),
            reminder_file_tracker_state: Vec::new(),
            killed_agents: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
            auto_memory_state,
            team_store,
            team_mailbox,
        });

        // Clean up orphaned worktrees at session startup (Gap 8 fix)
        if let Ok(ref state) = state_result {
            Self::cleanup_orphaned_worktrees(&state.session.working_dir).await;
        }

        state_result
    }

    /// Clean up orphaned worktrees matching the `agent/task-*` branch naming convention.
    ///
    /// At session startup, detect and silently remove worktrees whose branches match
    /// the auto-generated pattern and have no live session.
    async fn cleanup_orphaned_worktrees(cwd: &std::path::Path) {
        // Use git rev-parse to check if we're in a git repo (no cocode_git dep)
        let in_repo = tokio::process::Command::new("git")
            .current_dir(cwd)
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !in_repo {
            return;
        }
        let output = match tokio::process::Command::new("git")
            .current_dir(cwd)
            .args(["worktree", "list", "--porcelain"])
            .output()
            .await
        {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => return,
        };

        // Parse porcelain output for worktrees with agent/task-* branches
        let mut worktree_path: Option<String> = None;
        for line in output.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                worktree_path = Some(path.to_string());
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                if branch.starts_with("agent/task-")
                    && let Some(ref wt_path) = worktree_path
                {
                    // Silently remove the orphaned worktree
                    let _ = tokio::process::Command::new("git")
                        .current_dir(cwd)
                        .args(["worktree", "remove", "--force", wt_path])
                        .output()
                        .await;
                    tracing::debug!(worktree = wt_path, branch, "Cleaned up orphaned worktree");
                }
                worktree_path = None;
            } else if line.is_empty() {
                worktree_path = None;
            }
        }

        // Prune stale entries
        let _ = tokio::process::Command::new("git")
            .current_dir(cwd)
            .args(["worktree", "prune"])
            .output()
            .await;
    }

    /// Run a single turn with the given user input.
    ///
    /// This creates an agent loop and runs it to completion,
    /// returning the result of the conversation turn.
    pub async fn run_turn(&mut self, user_input: &str) -> anyhow::Result<TurnResult> {
        info!(
            session_id = %self.session.id,
            input_len = user_input.len(),
            "Running turn"
        );

        // Update session activity
        self.session.touch();

        // Create event channel
        let (event_tx, mut event_rx) = mpsc::channel::<LoopEvent>(256);

        // Spawn task to handle events (logging for now)
        let cancel_token = self.cancel_token.clone();
        let event_task = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if cancel_token.is_cancelled() {
                    break;
                }
                Self::handle_event(&event);
            }
        });

        // Build environment info
        let environment = EnvironmentInfo::builder()
            .cwd(&self.session.working_dir)
            .context_window(self.context_window)
            .max_output_tokens(16_384)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build environment: {e}"))?;

        // Build conversation context
        let mut ctx_builder = ConversationContext::builder()
            .environment(environment)
            .tool_names(self.tool_registry.tool_names())
            .injections(self.build_suffix_injections());

        if let Some(style_config) = self.resolve_output_style() {
            ctx_builder = ctx_builder.output_style(style_config);
        }

        let context = ctx_builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build context: {e}"))?;

        // Set the execute_fn, tool list, and event_tx on the subagent manager (fresh per-turn)
        {
            let execute_fn = self.build_execute_fn(event_tx.clone());
            let mut mgr = self.subagent_manager.lock().await;
            mgr.set_execute_fn(execute_fn);
            mgr.set_all_tools(self.tool_registry.tool_names());
            mgr.set_event_tx(event_tx.clone());
            mgr.set_background_stop_hook_fn(self.build_background_stop_hook_fn());
        }

        // Wire hook callbacks for LLM verification (Prompt handler) and agent spawning (Agent handler).
        // Both use spawn_agent_fn to avoid needing direct model access.
        {
            // Model call: spawn a 1-turn "explore" subagent with the system+user prompt combined
            let spawn_fn_for_model = self.build_spawn_agent_fn();
            self.hook_registry.set_model_call_fn(std::sync::Arc::new(
                move |system_prompt: String, user_message: String| {
                    let spawn_fn = spawn_fn_for_model.clone();
                    Box::pin(async move {
                        let combined = format!("{system_prompt}\n\n{user_message}");
                        let input = cocode_tools::SpawnAgentInput {
                            agent_type: SubagentType::Explore.as_str().to_string(),
                            prompt: combined,
                            model: None,
                            max_turns: Some(1),
                            run_in_background: None,
                            allowed_tools: Some(vec![]),
                            parent_selections: None,
                            permission_mode: None,
                            resume_from: None,
                            isolation: None,
                            name: None,
                            team_name: None,
                            mode: None,
                            cwd: None,
                            description: None,
                        };
                        let result = spawn_fn(input).await.map_err(|e| e.to_string())?;
                        Ok(result.output.unwrap_or_default())
                    })
                },
            ));

            // Agent callback: spawn an "explore" subagent with restricted tools
            let spawn_fn_for_agent = self.build_spawn_agent_fn();
            self.hook_registry.set_agent_fn(std::sync::Arc::new(
                move |prompt: String, allowed_tools: Vec<String>, max_turns: i32| {
                    let spawn_fn = spawn_fn_for_agent.clone();
                    Box::pin(async move {
                        let input = cocode_tools::SpawnAgentInput {
                            agent_type: SubagentType::Explore.as_str().to_string(),
                            prompt,
                            model: None,
                            max_turns: Some(max_turns),
                            run_in_background: None,
                            allowed_tools: Some(allowed_tools),
                            parent_selections: None,
                            permission_mode: None,
                            resume_from: None,
                            isolation: None,
                            name: None,
                            team_name: None,
                            mode: None,
                            cwd: None,
                            description: None,
                        };
                        let result = spawn_fn(input).await.map_err(|e| e.to_string())?;
                        Ok(result.output.unwrap_or_default())
                    })
                },
            ));
        }

        // Build and run the agent loop
        // Clone selections so the loop has its own copy (isolation)
        let mut builder = AgentLoop::builder(
            self.api_client.clone(),
            self.model_hub.clone(),
            self.session.selections.clone(),
            self.tool_registry.clone(),
            context,
            event_tx,
        )
        .config(self.loop_config.clone())
        .fallback_config(FallbackConfig::default())
        .hooks(self.hook_registry.clone())
        .cancel_token(self.cancel_token.clone())
        .queued_commands(self.queued_commands.clone())
        .features(self.config.features.clone())
        .web_search_config(self.config.web_search_config.clone())
        .web_fetch_config(self.config.web_fetch_config.clone())
        .permission_rules(self.permission_rules.clone())
        .shell_executor(self.shell_executor.clone())
        .skill_manager(self.skill_manager.clone())
        .otel_manager(self.otel_manager.clone())
        .lsp_manager(self.lsp_manager.clone())
        .spawn_agent_fn(self.build_spawn_agent_fn())
        .plan_mode_state(self.plan_mode_state.clone())
        .question_responder(self.question_responder.clone())
        .approval_store(self.shared_approval_store.clone())
        .reminder_file_tracker_state(self.reminder_file_tracker_state.clone())
        .message_history(self.message_history.clone())
        .cocode_home(self.config.cocode_home.clone())
        .killed_agents(self.killed_agents.clone())
        .auto_memory_state(Arc::clone(&self.auto_memory_state))
        .team_store(Arc::clone(&self.team_store))
        .team_mailbox(Arc::clone(&self.team_mailbox));

        // Wire snapshot manager for rewind support (main turn only)
        if let Some(ref sm) = self.snapshot_manager {
            builder = builder.snapshot_manager(sm.clone());
        }

        let mut loop_instance = builder.build();

        // Push background agent task info for system reminders
        loop_instance.set_background_agent_tasks(self.collect_background_agent_tasks().await);

        let result = loop_instance.run(user_input).await?;

        // Extract todos state before dropping the loop
        if let Some(todos) = loop_instance.take_todos() {
            self.todos = todos;
        }

        // Extract structured tasks state from the loop
        if let Some(tasks) = loop_instance.take_structured_tasks() {
            self.structured_tasks = tasks;
        }

        // Extract cron jobs state from the loop
        if let Some(jobs) = loop_instance.take_cron_jobs() {
            self.cron_jobs = jobs;
        }

        // Sync message history back from loop (persists across turns)
        self.message_history = loop_instance.message_history().clone();

        // Extract plan mode state from the loop (persists across turns)
        if let Some(plan_state) = loop_instance.take_plan_mode_state() {
            self.plan_mode_state = plan_state;
        }

        // Extract file tracker state from the loop (persists across turns)
        self.reminder_file_tracker_state = loop_instance.reminder_file_tracker_snapshot().await;

        // Inject allowed prompts from plan exit into the shared approval store
        if let StopReason::PlanModeExit {
            ref allowed_prompts,
            ..
        } = result.stop_reason
            && !allowed_prompts.is_empty()
        {
            self.inject_allowed_prompts(allowed_prompts).await;
            info!(
                count = allowed_prompts.len(),
                "Injected allowed prompts from plan exit into approval store"
            );
        }

        // Drop the event sender to signal end of events, then wait for task to complete
        drop(loop_instance);
        let _ = event_task.await;

        // Update totals
        self.total_turns += result.turns_completed;
        self.total_input_tokens += result.total_input_tokens;
        self.total_output_tokens += result.total_output_tokens;

        Ok(TurnResult::from_loop_result(&result))
    }

    /// Run a skill turn with optional model override.
    ///
    /// When `model_override` is provided, temporarily switches the main model
    /// for this turn. The model override can be:
    /// - A full spec like "provider/model"
    /// - A short name like "sonnet" (resolved using current provider)
    pub async fn run_skill_turn(
        &mut self,
        prompt: &str,
        model_override: Option<&str>,
    ) -> anyhow::Result<TurnResult> {
        // If model override is requested, temporarily switch the main selection
        let saved_selection = if let Some(model_name) = model_override {
            let current = self.session.selections.get(ModelRole::Main).cloned();
            let spec = if model_name.contains('/') {
                model_name
                    .parse::<cocode_protocol::model::ModelSpec>()
                    .map_err(|e| anyhow::anyhow!("Invalid model spec '{model_name}': {e}"))?
            } else {
                // Use current provider with the given model name
                let provider = self.provider().to_string();
                cocode_protocol::model::ModelSpec::new(provider, model_name)
            };
            info!(
                model = %spec,
                "Overriding model for skill turn"
            );
            self.session
                .selections
                .set(ModelRole::Main, RoleSelection::new(spec));
            current
        } else {
            None
        };

        let result = self.run_turn(prompt).await;

        // Restore original selection if we overrode it
        if let Some(original) = saved_selection {
            self.session.selections.set(ModelRole::Main, original);
        } else if model_override.is_some() {
            // Edge case: there was no previous main selection (shouldn't happen)
            // Just leave the new one in place
        }

        result
    }

    /// Run a skill turn with optional model override, streaming events.
    ///
    /// Same as [`run_skill_turn`] but forwards events to the provided channel.
    pub async fn run_skill_turn_streaming(
        &mut self,
        prompt: &str,
        model_override: Option<&str>,
        event_tx: mpsc::Sender<LoopEvent>,
    ) -> Result<TurnResult, cocode_error::BoxedError> {
        let saved_selection = if let Some(model_name) = model_override {
            let current = self.session.selections.get(ModelRole::Main).cloned();
            let spec = if model_name.contains('/') {
                model_name
                    .parse::<cocode_protocol::model::ModelSpec>()
                    .context(session_state_error::InvalidModelSpecSnafu {
                        model_name: model_name.to_string(),
                    })
                    .map_err(boxed_err)?
            } else {
                let provider = self.provider().to_string();
                cocode_protocol::model::ModelSpec::new(provider, model_name)
            };
            info!(
                model = %spec,
                "Overriding model for skill turn (streaming)"
            );
            self.session
                .selections
                .set(ModelRole::Main, RoleSelection::new(spec));
            current
        } else {
            None
        };

        let result = self.run_turn_streaming(prompt, event_tx).await;

        if let Some(original) = saved_selection {
            self.session.selections.set(ModelRole::Main, original);
        }

        result
    }

    /// Spawn a subagent for a skill with `context: fork`.
    ///
    /// Bridges the skill execution layer to the subagent manager,
    /// converting model name → `ExecutionIdentity` and invoking `spawn_full`.
    pub async fn spawn_subagent_for_skill(
        &mut self,
        agent_type: &str,
        prompt: &str,
        model: Option<&str>,
        allowed_tools: Option<Vec<String>>,
    ) -> anyhow::Result<cocode_tools::SpawnAgentResult> {
        // Convert model string → ExecutionIdentity
        let identity = model.map(|m| {
            if m.contains('/') {
                match m.parse::<ModelSpec>() {
                    Ok(spec) => ExecutionIdentity::Spec(spec),
                    Err(_) => ExecutionIdentity::Inherit,
                }
            } else {
                match m.to_lowercase().as_str() {
                    "haiku" => ExecutionIdentity::Role(ModelRole::Fast),
                    "sonnet" | "opus" => ExecutionIdentity::Role(ModelRole::Main),
                    _ => ExecutionIdentity::Inherit,
                }
            }
        });

        let spawn_input = cocode_subagent::SpawnInput {
            agent_type: agent_type.to_string(),
            prompt: prompt.to_string(),
            identity,
            max_turns: None,
            run_in_background: Some(false),
            allowed_tools,
            resume_from: None,
            name: None,
            team_name: None,
            mode: None,
            cwd: None,
            isolation_override: None,
            description: None,
        };

        let mut mgr = self.subagent_manager.lock().await;
        let result = mgr.spawn_full(spawn_input).await?;

        Ok(cocode_tools::SpawnAgentResult {
            agent_id: result.agent_id,
            output: result.output,
            output_file: result.background.as_ref().map(|bg| bg.output_file.clone()),
            cancel_token: result.cancel_token,
            color: result.color,
        })
    }

    /// Handle a loop event (logging).
    fn handle_event(event: &LoopEvent) {
        match event {
            LoopEvent::TurnStarted {
                turn_id,
                turn_number,
            } => {
                debug!(turn_id, turn_number, "Turn started");
            }
            LoopEvent::TurnCompleted { turn_id, usage } => {
                debug!(
                    turn_id,
                    input_tokens = usage.input_tokens,
                    output_tokens = usage.output_tokens,
                    "Turn completed"
                );
            }
            LoopEvent::TextDelta { delta, .. } => {
                // In a real implementation, this would stream to UI
                debug!(delta_len = delta.len(), "Text delta");
            }
            LoopEvent::ToolUseQueued { name, call_id, .. } => {
                debug!(name, call_id, "Tool queued");
            }
            LoopEvent::Error { error } => {
                tracing::error!(code = %error.code, message = %error.message, "Loop error");
            }
            _ => {
                debug!(?event, "Loop event");
            }
        }
    }

    /// Get the OTel manager (if configured).
    pub fn otel_manager(&self) -> Option<&Arc<cocode_otel::OtelManager>> {
        self.otel_manager.as_ref()
    }

    /// Cancel the current operation.
    pub fn cancel(&self) {
        info!(session_id = %self.session.id, "Cancelling session");
        self.cancel_token.cancel();
    }

    /// Check if the session is cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Get a clone of the cancellation token.
    ///
    /// The TUI driver uses this to cancel the running turn directly,
    /// bypassing the command channel for immediate effect.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }

    /// Replace the cancellation token with a fresh one.
    ///
    /// Call this after a turn is cancelled so the next turn can proceed.
    /// `CancellationToken` is one-shot — once cancelled it stays cancelled.
    pub fn reset_cancel_token(&mut self) {
        self.cancel_token = CancellationToken::new();
    }

    /// Reset the shell executor's CWD to the original working directory.
    ///
    /// Called during clear context after plan exit so the next turn
    /// starts from the project root instead of whatever CWD the shell
    /// navigated to during the planning phase.
    pub fn reset_shell_cwd(&mut self) {
        self.shell_executor
            .set_cwd(self.session.working_dir.clone());
    }

    /// Inject allowed prompts from a plan's `allowedPrompts` into the
    /// shared approval store so they persist across subsequent turns.
    pub async fn inject_allowed_prompts(&self, prompts: &[cocode_protocol::AllowedPrompt]) {
        let mut store = self.shared_approval_store.lock().await;
        for ap in prompts {
            store.approve_pattern(&ap.tool, &ap.prompt);
        }
    }

    /// Clear conversation context for plan exit (creates new session identity).
    ///
    /// This fires SessionEnd hooks for the old session, replaces it with
    /// a child session (new ID, parent tracking), clears message history,
    /// resets shell CWD, and fires SessionStart hooks for the new session.
    pub async fn clear_context(&mut self) {
        // Fire SessionEnd hooks with the OLD session ID
        let end_ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::SessionEnd,
            self.session.id.clone(),
            self.session.working_dir.clone(),
        )
        .with_reason("context_clear");
        self.hook_registry.execute(&end_ctx).await;

        // Create child session (new ID, parent tracking)
        let child = self.session.derive_child();
        self.session = child;

        // Clear message history
        self.message_history.clear();

        // Reset shell CWD to project root
        self.reset_shell_cwd();

        // Reset plan mode state for the fresh session
        self.plan_mode_state = PlanModeState::new();

        // Fire SessionStart hooks with the NEW session ID
        let start_ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::SessionStart,
            self.session.id.clone(),
            self.session.working_dir.clone(),
        )
        .with_source("context_clear");
        self.hook_registry.execute(&start_ctx).await;
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session.id
    }

    /// Get the model name.
    ///
    /// Returns the main model name, or an empty string if not configured.
    pub fn model(&self) -> &str {
        self.session.model().unwrap_or("")
    }

    /// Get the provider name.
    ///
    /// Returns the main provider name, or an empty string if not configured.
    pub fn provider(&self) -> &str {
        self.session.provider().unwrap_or("")
    }

    /// Get total turns run.
    pub fn total_turns(&self) -> i32 {
        self.total_turns
    }

    /// Get total input tokens consumed.
    pub fn total_input_tokens(&self) -> i32 {
        self.total_input_tokens
    }

    /// Get total output tokens generated.
    pub fn total_output_tokens(&self) -> i32 {
        self.total_output_tokens
    }

    /// Get the message history.
    pub fn history(&self) -> &MessageHistory {
        &self.message_history
    }

    /// Get mutable access to the message history.
    pub fn history_mut(&mut self) -> &mut MessageHistory {
        &mut self.message_history
    }

    /// Set the hook registry.
    pub fn set_hooks(&mut self, hooks: Arc<HookRegistry>) {
        self.hook_registry = hooks;
    }

    /// Execute SessionEnd hooks and perform cleanup.
    ///
    /// Call this before dropping the session state to give hooks a chance
    /// to run (e.g., saving state, logging).
    pub async fn close(&self) {
        let ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::SessionEnd,
            self.session.id.clone(),
            self.session.working_dir.clone(),
        );
        self.hook_registry.execute(&ctx).await;
    }

    /// Add a skill to the session.
    pub fn add_skill(&mut self, skill: SkillInterface) {
        self.skills.push(skill);
    }

    /// Get the loaded skills.
    pub fn skills(&self) -> &[SkillInterface] {
        &self.skills
    }

    /// Get the skill manager.
    pub fn skill_manager(&self) -> &Arc<SkillManager> {
        &self.skill_manager
    }

    /// Get the plugin registry, if any plugins are loaded.
    pub fn plugin_registry(&self) -> Option<&cocode_plugin::PluginRegistry> {
        self.plugin_registry.as_ref()
    }

    /// Build plugin summary data for the TUI Plugin Manager overlay.
    ///
    /// Returns `(installed_plugins, marketplace_summaries)`.
    pub fn plugin_summaries(
        &self,
    ) -> (
        Vec<cocode_protocol::PluginSummaryInfo>,
        Vec<cocode_protocol::MarketplaceSummaryInfo>,
    ) {
        let installed = if let Some(ref registry) = self.plugin_registry {
            registry
                .all()
                .map(|p| {
                    let skills = p.contributions.iter().filter(|c| c.is_skill()).count() as i32;
                    let hooks = p.contributions.iter().filter(|c| c.is_hook()).count() as i32;
                    let agents = p.contributions.iter().filter(|c| c.is_agent()).count() as i32;
                    cocode_protocol::PluginSummaryInfo {
                        name: p.name().to_string(),
                        description: p.manifest.plugin.description.clone(),
                        version: p.version().to_string(),
                        enabled: true, // if it's in the registry, it's enabled
                        scope: format!("{:?}", p.scope),
                        skills_count: skills,
                        hooks_count: hooks,
                        agents_count: agents,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        // Load marketplace data
        let plugins_dir = self.config.cocode_home.join("plugins");
        let marketplaces = if plugins_dir.is_dir() {
            let mm = cocode_plugin::MarketplaceManager::new(plugins_dir);
            mm.list()
                .into_iter()
                .map(|(name, km)| {
                    let (source_type, source) = marketplace_source_display(&km.source);
                    cocode_protocol::MarketplaceSummaryInfo {
                        name,
                        source_type,
                        source,
                        auto_update: km.auto_update,
                        plugin_count: 0, // would require loading manifest
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        (installed, marketplaces)
    }

    /// Get the subagent manager (shared handle).
    pub fn subagent_manager(&self) -> &Arc<tokio::sync::Mutex<SubagentManager>> {
        &self.subagent_manager
    }

    /// Update the loop configuration.
    pub fn set_loop_config(&mut self, config: LoopConfig) {
        self.loop_config = config;
    }

    /// Get the loop configuration.
    pub fn loop_config(&self) -> &LoopConfig {
        &self.loop_config
    }

    // ==========================================================
    // Role Selection API
    // ==========================================================

    /// Get all current role selections.
    ///
    /// Returns a clone of the session's selections.
    pub fn get_selections(&self) -> RoleSelections {
        self.session.selections.clone()
    }

    /// Get selection for a specific role.
    ///
    /// Falls back to Main if the role is not configured.
    pub fn selection(&self, role: ModelRole) -> Option<RoleSelection> {
        self.session.selections.get_or_main(role).cloned()
    }

    /// Get thinking level for a specific role.
    ///
    /// Returns the explicitly set thinking level for this role, or None
    /// if no override is set (model's default will be used).
    pub fn thinking_level(&self, role: ModelRole) -> Option<ThinkingLevel> {
        self.session
            .selections
            .get_or_main(role)
            .and_then(|s| s.thinking_level.clone())
    }

    /// Get the provider API for this session.
    pub fn provider_api(&self) -> ProviderApi {
        self.api
    }

    /// Get the model hub.
    ///
    /// The hub provides model acquisition and caching (role-agnostic).
    pub fn model_hub(&self) -> &Arc<ModelHub> {
        &self.model_hub
    }

    /// Get or create a model for a specific role.
    ///
    /// Get model for a specific role using the session's selections.
    /// Falls back to the main role if the requested role has no selection.
    ///
    /// # Returns
    ///
    /// A tuple of (model, provider_api) for the role, or None if no selection exists.
    pub fn get_model_for_role(
        &self,
        role: ModelRole,
    ) -> anyhow::Result<Option<(Arc<dyn cocode_api::LanguageModel>, ProviderApi)>> {
        match self
            .model_hub
            .get_model_for_role_with_selections(role, &self.session.selections)
        {
            Ok((model, api)) => Ok(Some((model, api))),
            Err(e) => {
                // If error is "no model configured", return None instead of error
                if e.is_no_model_configured() {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("{e}"))
                }
            }
        }
    }

    /// Get the main model (shorthand for get_model_for_role(ModelRole::Main)).
    ///
    /// Returns the main model using the session's selections.
    pub fn main_model(&self) -> anyhow::Result<Arc<dyn cocode_api::LanguageModel>> {
        self.model_hub
            .get_model_for_role_with_selections(ModelRole::Main, &self.session.selections)
            .map(|(m, _)| m)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Switch model for a specific role.
    ///
    /// Updates the session's role selections.
    pub fn switch_role(&mut self, role: ModelRole, selection: RoleSelection) {
        info!(
            role = %role,
            model = %selection.model,
            thinking = ?selection.thinking_level,
            "Switching role"
        );
        self.session.selections.set(role, selection);
    }

    /// Switch only the thinking level for a specific role.
    ///
    /// This updates the thinking level without changing the model.
    /// Returns `true` if the role selection exists and was updated.
    pub fn switch_thinking_level(&mut self, role: ModelRole, level: ThinkingLevel) -> bool {
        info!(
            role = %role,
            thinking = %level,
            "Switching thinking level for role"
        );
        self.session.selections.set_thinking_level(role, level)
    }

    /// Clear thinking level override for a specific role.
    ///
    /// Returns `true` if the role selection exists and was updated.
    pub fn clear_thinking_level(&mut self, role: ModelRole) -> bool {
        // Get current selection, clear thinking level, and set it back
        if let Some(mut selection) = self.session.selections.get(role).cloned() {
            selection.clear_thinking_level();
            self.session.selections.set(role, selection);
            info!(role = %role, "Cleared thinking level for role");
            true
        } else {
            false
        }
    }

    /// Build provider options from current thinking level for a role.
    ///
    /// Returns the provider-specific options needed to configure thinking
    /// for the current session's provider, or None if no thinking is configured.
    ///
    /// Note: If `model_info` is None, default ModelInfo is used, which means
    /// no reasoning_summary or include_thoughts overrides will be applied.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Get options for main role with model info
    /// if let Some(opts) = state.build_thinking_options(ModelRole::Main, Some(&model_info)) {
    ///     request = request.provider_options(opts);
    /// }
    /// ```
    pub fn build_thinking_options(
        &self,
        role: ModelRole,
        model_info: Option<&cocode_protocol::ModelInfo>,
    ) -> Option<cocode_api::ProviderOptions> {
        let thinking_level = self.thinking_level(role)?;
        let default_model_info = cocode_protocol::ModelInfo::default();
        let model_info = model_info.unwrap_or(&default_model_info);
        cocode_api::thinking_convert::to_provider_options(&thinking_level, model_info, self.api)
    }

    // ==========================================================
    // System Prompt Suffix API
    // ==========================================================

    /// Set a suffix to append to the end of the system prompt.
    pub fn set_system_prompt_suffix(&mut self, suffix: String) {
        self.system_prompt_suffix = Some(suffix);
    }

    /// Resolve the active output style for prompt generation.
    ///
    /// Priority: runtime override > config file setting.
    /// Style lookup: built-in/custom > plugin-contributed.
    fn resolve_output_style(&self) -> Option<cocode_context::OutputStylePromptConfig> {
        // Determine the effective style name
        let style_name: Option<&str> = match &self.output_style_override {
            Some(override_style) => override_style.as_deref(),
            None => self.config.output_style.as_deref(),
        };

        let name = style_name?;

        // Try built-in and custom styles first (project-level > user-level > built-in)
        if let Some(info) = cocode_config::builtin::find_output_style(
            name,
            &self.config.cocode_home,
            Some(&self.config.cwd),
        ) {
            return Some(cocode_context::OutputStylePromptConfig {
                name: info.name,
                content: info.content,
                keep_coding_instructions: info.keep_coding_instructions,
            });
        }

        // Fall back to plugin-contributed styles
        let name_lower = name.to_lowercase();
        if let Some((style_name, prompt)) = self
            .plugin_output_styles
            .iter()
            .find(|(n, _)| n.to_lowercase() == name_lower)
        {
            return Some(cocode_context::OutputStylePromptConfig {
                name: style_name.clone(),
                content: prompt.clone(),
                keep_coding_instructions: false,
            });
        }

        None
    }

    /// Build context injections from the system prompt suffix.
    fn build_suffix_injections(&self) -> Vec<ContextInjection> {
        self.system_prompt_suffix
            .as_ref()
            .map(|suffix| {
                vec![ContextInjection {
                    label: "system-prompt-suffix".to_string(),
                    content: suffix.clone(),
                    position: InjectionPosition::EndOfPrompt,
                }]
            })
            .unwrap_or_default()
    }

    // ==========================================================
    // Queued Commands API
    // ==========================================================

    /// Queue a command for real-time steering.
    ///
    /// Thread-safe: can be called while a turn is running. The shared mutex
    /// ensures commands queued here are visible to the running `AgentLoop`
    /// at its next Step 6.5 drain.
    ///
    /// Returns the command ID.
    pub fn queue_command(&self, prompt: impl Into<String>) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let id = uuid::Uuid::new_v4().to_string();
        let cmd = QueuedCommandInfo {
            id: id.clone(),
            prompt: prompt.into(),
            queued_at: now,
        };
        self.queued_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(cmd);
        id
    }

    /// Get the number of queued commands.
    pub fn queued_count(&self) -> usize {
        self.queued_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Take all queued commands (for passing to AgentLoop).
    pub fn take_queued_commands(&self) -> Vec<QueuedCommandInfo> {
        std::mem::take(
            &mut *self
                .queued_commands
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Clear all queued commands.
    pub fn clear_queued_commands(&self) {
        self.queued_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    /// Set the active output style.
    ///
    /// `None` disables the output style; `Some(name)` activates the named style.
    /// Takes effect on the next turn (system prompt regeneration).
    pub fn set_output_style(&mut self, style: Option<String>) {
        info!(?style, "Setting output style");
        self.output_style_override = Some(style);
    }

    /// Get the current output style name (for display in /output-style status).
    ///
    /// Priority: runtime override > config file setting.
    pub fn current_output_style_name(&self) -> Option<&str> {
        match &self.output_style_override {
            Some(override_style) => override_style.as_deref(),
            None => self.config.output_style.as_deref(),
        }
    }

    /// Get the project directory (working directory) for project-level lookups.
    pub fn project_dir(&self) -> &std::path::Path {
        &self.session.working_dir
    }

    /// Get the cocode home directory.
    pub fn cocode_home(&self) -> &std::path::Path {
        &self.config.cocode_home
    }

    /// Get the plugin-contributed output styles.
    pub fn plugin_output_styles(&self) -> &[(String, String)] {
        &self.plugin_output_styles
    }

    /// Get the plan mode state.
    pub fn plan_mode_state(&self) -> &PlanModeState {
        &self.plan_mode_state
    }

    /// Get a mutable reference to plan mode state.
    pub fn plan_mode_state_mut(&mut self) -> &mut PlanModeState {
        &mut self.plan_mode_state
    }

    /// Set the permission mode for the session.
    ///
    /// Updates the loop config's permission mode. If switching to Plan mode,
    /// also saves the pre-plan mode for restoration on exit.
    pub fn set_permission_mode(&mut self, mode: PermissionMode) {
        let old_mode = self.loop_config.permission_mode;
        self.loop_config.permission_mode = mode;

        if mode == PermissionMode::Plan && old_mode != PermissionMode::Plan {
            // Entering plan mode: save the old mode for restoration
            self.plan_mode_state.pre_plan_mode = Some(old_mode);
            self.plan_mode_state.is_active = true;
        } else if mode != PermissionMode::Plan && old_mode == PermissionMode::Plan {
            // Leaving plan mode via mode cycle (not via ExitPlanMode tool)
            self.plan_mode_state.is_active = false;
            self.plan_mode_state.pre_plan_mode = None;
        }
    }

    /// Get the snapshot manager (if configured for rewind support).
    pub fn snapshot_manager(&self) -> Option<&Arc<cocode_file_backup::SnapshotManager>> {
        self.snapshot_manager.as_ref()
    }

    /// Set the snapshot manager for rewind support.
    pub fn set_snapshot_manager(&mut self, mgr: Arc<cocode_file_backup::SnapshotManager>) {
        self.snapshot_manager = Some(mgr);
    }

    /// Get a shared handle to the queued commands.
    ///
    /// The TUI driver uses this to push commands while a turn is running,
    /// without needing `&mut self`.
    pub fn shared_queued_commands(&self) -> Arc<Mutex<Vec<QueuedCommandInfo>>> {
        self.queued_commands.clone()
    }

    /// Get a shared handle to the question responder.
    ///
    /// The TUI driver extracts this before a turn starts and passes it to
    /// `handle_in_flight_command` so question responses can unblock the
    /// AskUserQuestion tool immediately during the turn.
    pub fn question_responder(&self) -> Arc<cocode_tools::QuestionResponder> {
        self.question_responder.clone()
    }

    /// Get the current task list from the most recent TodoWrite tool call.
    ///
    /// Reads from the dedicated `todos` field, updated by `ContextModifier::TodosUpdated`
    /// after each agent loop turn.
    pub fn current_todos(&self) -> String {
        let todos = match self.todos.as_array() {
            Some(arr) if !arr.is_empty() => arr,
            _ => return "No tasks.".to_string(),
        };
        let mut output = String::new();
        for (i, todo) in todos.iter().enumerate() {
            let id = todo["id"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| format!("{}", i + 1));
            let title = todo["subject"]
                .as_str()
                .or_else(|| todo["content"].as_str())
                .unwrap_or("?");
            let status = todo["status"].as_str().unwrap_or("?");
            let marker = match status {
                "completed" => "[x]",
                "in_progress" => "[>]",
                _ => "[ ]",
            };
            output.push_str(&format!("{marker} {id}: {title}\n"));
        }
        output
    }

    /// Replace the current task list.
    ///
    /// Used by rewind to reconstruct todo state from retained message history.
    pub fn set_todos(&mut self, todos: serde_json::Value) {
        self.todos = todos;
    }

    // ==========================================================
    // Reminder File Tracker State
    // ==========================================================

    /// Get the reminder file tracker state.
    ///
    /// This state is persisted across AgentLoop runs to maintain file read
    /// tracking for already-read detection.
    pub fn reminder_file_tracker_state(
        &self,
    ) -> &[(std::path::PathBuf, cocode_tools::FileReadState)] {
        &self.reminder_file_tracker_state
    }

    /// Set the reminder file tracker state.
    ///
    /// Called after each AgentLoop turn to persist file tracker state.
    pub fn set_reminder_file_tracker_state(
        &mut self,
        state: Vec<(std::path::PathBuf, cocode_tools::FileReadState)>,
    ) {
        self.reminder_file_tracker_state = state;
    }

    /// Apply rewind mode to conversation state.
    ///
    /// Handles the three rewind modes:
    /// - `CodeAndConversation`: Truncate history, rebuild todos/tracker
    /// - `ConversationOnly`: Truncate history only, rebuild todos/tracker
    /// - `CodeOnly`: Keep history, rebuild file tracker from retained history
    ///
    /// File restoration is handled externally by `SnapshotManager` before
    /// this method is called. This method only manages conversation state.
    ///
    /// # Returns
    ///
    /// A tuple of (messages_removed, restored_prompt). The restored_prompt is the
    /// user message text from the rewound turn, which can be used to restore the
    /// UI input field after rewind.
    pub fn apply_rewind_mode_for_turn(
        &mut self,
        rewound_turn: i32,
        mode: cocode_protocol::RewindMode,
    ) -> (i32, Option<String>) {
        match mode {
            cocode_protocol::RewindMode::CodeAndConversation => {
                let (messages_removed, restored_prompt) =
                    self.rewind_conversation_state_from_turn(rewound_turn);
                tracing::debug!(
                    rewound_turn,
                    messages_removed,
                    has_prompt = restored_prompt.is_some(),
                    "Applied CodeAndConversation rewind"
                );
                (messages_removed, restored_prompt)
            }
            cocode_protocol::RewindMode::ConversationOnly => {
                let (messages_removed, restored_prompt) =
                    self.rewind_conversation_state_from_turn(rewound_turn);
                tracing::debug!(
                    rewound_turn,
                    messages_removed,
                    has_prompt = restored_prompt.is_some(),
                    "Applied ConversationOnly rewind"
                );
                (messages_removed, restored_prompt)
            }
            cocode_protocol::RewindMode::CodeOnly => {
                // Keep history but rebuild tracker from retained history
                self.rebuild_reminder_file_tracker_from_history();
                tracing::debug!(rewound_turn, "Applied CodeOnly rewind");
                (0, None)
            }
        }
    }

    /// Truncate message history from a specific turn.
    ///
    /// Removes all turns at or after the given turn number.
    fn truncate_history_from_turn(&mut self, from_turn: i32) -> i32 {
        let before = self.message_history.turn_count();
        self.message_history.truncate_from_turn(from_turn);
        before - self.message_history.turn_count()
    }

    /// Rebuild todos from retained message history.
    ///
    /// Scans through the message history for TodoWrite/TodoUpdate tool calls
    /// in reverse order to find the most recent todo state.
    fn rebuild_todos_from_history(&mut self) {
        let todos = self.reconstruct_todos_from_history();
        self.set_todos(todos);
    }

    /// Reconstruct todos from message history by finding the most recent TodoWrite result.
    ///
    /// Walks turns in reverse to find the most recent TodoWrite tool call
    /// with a successful output, then parses and returns the todo list.
    fn reconstruct_todos_from_history(&self) -> serde_json::Value {
        use cocode_protocol::ToolResultContent;

        let todo_write_name = cocode_protocol::ToolName::TodoWrite.as_str();

        // Walk turns in reverse to find the most recent TodoWrite result
        for turn in self.message_history.turns().iter().rev() {
            for tc in &turn.tool_calls {
                if tc.name == todo_write_name
                    && let Some(ref output) = tc.output
                {
                    match output {
                        ToolResultContent::Text(text) => {
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) {
                                return parsed;
                            }
                        }
                        ToolResultContent::Structured(value) => {
                            return value.clone();
                        }
                    }
                }
            }
        }

        // No todos found, return empty array
        serde_json::Value::Array(vec![])
    }

    /// Rebuild reminder file tracker from retained history.
    ///
    /// Extracts `ContextModifier::FileRead` entries from tool calls in the
    /// message history to reconstruct the file tracker state.
    fn rebuild_reminder_file_tracker_from_history(&mut self) {
        use cocode_system_reminder::build_file_read_state_from_modifiers;

        // Build iterator of (tool_name, modifiers, turn_number, is_completed)
        let state = build_file_read_state_from_modifiers(
            self.message_history.turns().iter().flat_map(|turn| {
                turn.tool_calls.iter().map(move |tc| {
                    (
                        tc.name.as_str(),
                        tc.modifiers.as_slice(),
                        turn.number,
                        tc.status.is_terminal(),
                    )
                })
            }),
            100,
        );
        self.reminder_file_tracker_state = state;
    }

    /// Rewind conversation state from a specific turn.
    ///
    /// This is a unified method for conversation state rollback that handles:
    /// - History truncation
    /// - Todos rebuilding
    /// - File tracker state rebuilding
    /// - Prompt capture for UI restoration
    ///
    /// # Arguments
    ///
    /// * `from_turn` - The turn number to rewind from (turns >= from_turn are removed)
    ///
    /// # Returns
    ///
    /// A tuple of (messages_removed, restored_prompt). The restored_prompt is the
    /// user message text from the rewound turn, which can be used to restore the
    /// UI input field after rewind.
    pub fn rewind_conversation_state_from_turn(&mut self, from_turn: i32) -> (i32, Option<String>) {
        // Capture prompt at the rewind turn BEFORE truncating
        let restored_prompt = self
            .message_history
            .turns()
            .iter()
            .find(|t| t.number == from_turn)
            .map(|t| t.user_message.text());

        let messages_removed = self.truncate_history_from_turn(from_turn);
        self.rebuild_todos_from_history();
        self.prune_reminder_file_tracker_for_turn_boundary(from_turn);
        (messages_removed, restored_prompt)
    }

    /// Prune reminder file tracker for turn boundary using merge-based approach.
    ///
    /// Instead of a simple retain, this method:
    /// 1. Rebuilds state from retained history turns
    /// 2. Prunes existing state to entries before the boundary and filters internal files
    /// 3. Merges: rebuilt has priority for same paths
    ///
    /// This handles:
    /// - Same-path overwrite drift (newer dropped reads hiding older retained reads)
    /// - Mention-driven reads that exist only in persisted snapshot
    /// - Internal file exclusion
    ///
    /// # Arguments
    ///
    /// * `boundary_turn` - The turn boundary; entries at or after this turn are removed
    pub fn prune_reminder_file_tracker_for_turn_boundary(&mut self, boundary_turn: i32) {
        use cocode_system_reminder::build_file_read_state_from_modifiers;
        use cocode_system_reminder::merge_file_read_state;
        use cocode_system_reminder::should_skip_tracked_file;

        // 1. Rebuild from retained history turns
        let retained_turns = self.message_history.turns();
        let rebuilt = build_file_read_state_from_modifiers(
            retained_turns
                .iter()
                .filter(|turn| turn.number < boundary_turn)
                .flat_map(|turn| {
                    turn.tool_calls.iter().map(move |tc| {
                        (
                            tc.name.as_str(),
                            tc.modifiers.as_slice(),
                            turn.number,
                            tc.status.is_terminal(),
                        )
                    })
                }),
            100,
        );

        // 2. Prune existing state: keep entries before boundary, filter internal files
        let plan_path = self.plan_mode_state.plan_file_path.as_ref();
        let pruned: Vec<_> = self
            .reminder_file_tracker_state
            .iter()
            .filter(|(_, s)| s.read_turn < boundary_turn)
            .filter(|(p, _)| {
                !should_skip_tracked_file(p, plan_path.map(std::path::PathBuf::as_path), None, &[])
            })
            .cloned()
            .collect();

        // 3. Merge: rebuilt has priority for same paths (more accurate from history)
        self.reminder_file_tracker_state = merge_file_read_state(pruned, rebuilt);
    }

    /// Rebuild reminder file tracker with session memory exclusion.
    ///
    /// Rebuilds file tracker state from message history, filtering out
    /// internal files like session memory and plan files.
    ///
    /// # Arguments
    ///
    /// * `session_memory_path` - Optional path to the session memory file to exclude
    pub fn rebuild_reminder_file_tracker_with_session_memory(
        &mut self,
        session_memory_path: Option<&std::path::PathBuf>,
    ) {
        use cocode_system_reminder::build_file_read_state_from_modifiers;
        use cocode_system_reminder::should_skip_tracked_file;

        let state = build_file_read_state_from_modifiers(
            self.message_history.turns().iter().flat_map(|turn| {
                turn.tool_calls.iter().map(move |tc| {
                    (
                        tc.name.as_str(),
                        tc.modifiers.as_slice(),
                        turn.number,
                        tc.status.is_terminal(),
                    )
                })
            }),
            100,
        );
        let plan_path = self.plan_mode_state.plan_file_path.as_ref();
        self.reminder_file_tracker_state = state
            .into_iter()
            .filter(|(p, _)| {
                !should_skip_tracked_file(
                    p,
                    plan_path.map(std::path::PathBuf::as_path),
                    session_memory_path.map(std::path::PathBuf::as_path),
                    &[],
                )
            })
            .collect();
    }

    // ==========================================================
    // Subagent Wiring
    // ==========================================================

    /// Build the `AgentExecuteFn` closure that the `SubagentManager` calls
    /// to actually run a child `AgentLoop`.
    ///
    /// Captures per-turn state snapshots so the child loop is isolated.
    /// The `parent_event_tx` is forwarded to the child loop so subagent
    /// progress, text deltas, and tool activity are visible to the TUI.
    fn build_execute_fn(
        &self,
        parent_event_tx: mpsc::Sender<LoopEvent>,
    ) -> cocode_subagent::AgentExecuteFn {
        let api_client = self.api_client.clone();
        let model_hub = self.model_hub.clone();
        let tool_registry = self.tool_registry.clone();
        let hook_registry = self.hook_registry.clone();
        let shell_executor = self.shell_executor.clone();
        let working_dir = self.session.working_dir.clone();
        let cocode_home = self.config.cocode_home.clone();
        let context_window = self.context_window;
        let features = self.config.features.clone();
        let web_search_config = self.config.web_search_config.clone();
        let web_fetch_config = self.config.web_fetch_config.clone();
        let permission_rules = self.permission_rules.clone();
        let skill_manager = self.skill_manager.clone();
        let lsp_manager = self.lsp_manager.clone();
        let selections = self.session.selections.clone();
        let message_history = self.message_history.clone();
        let team_store = Arc::clone(&self.team_store);
        let team_mailbox = Arc::clone(&self.team_mailbox);

        Box::new(move |params: AgentExecuteParams| {
            let api_client = api_client.clone();
            let model_hub = model_hub.clone();
            let tool_registry = tool_registry.clone();
            let hook_registry = hook_registry.clone();
            let shell_executor = shell_executor.clone();
            let working_dir = working_dir.clone();
            let cocode_home = cocode_home.clone();
            let features = features.clone();
            let web_search_config = web_search_config.clone();
            let web_fetch_config = web_fetch_config.clone();
            let permission_rules = permission_rules.clone();
            let skill_manager = skill_manager.clone();
            let lsp_manager = lsp_manager.clone();
            let selections = selections.clone();
            let message_history = message_history.clone();
            let parent_event_tx = parent_event_tx.clone();
            let team_store = team_store.clone();
            let team_mailbox = team_mailbox.clone();

            Box::pin(async move {
                // ── CWD override from spawn input ──────────────────────
                let base_working_dir = if let Some(ref cwd_override) = params.cwd {
                    std::path::PathBuf::from(cwd_override)
                } else {
                    working_dir.clone()
                };

                // ── G5: Worktree isolation ──────────────────────────────
                let (effective_working_dir, worktree_path) = if params.isolation
                    == Some(IsolationMode::Worktree)
                {
                    let wt_path = base_working_dir
                        .join(".cocode")
                        .join("worktrees")
                        .join(format!(
                            "{}-{}",
                            params.agent_type,
                            uuid::Uuid::new_v4().simple()
                        ));
                    let output = tokio::process::Command::new("git")
                        .args(["worktree", "add", "--detach", &wt_path.to_string_lossy()])
                        .current_dir(&base_working_dir)
                        .output()
                        .await;
                    match output {
                        Ok(o) if o.status.success() => {
                            tracing::info!(
                                path = %wt_path.display(),
                                agent_type = %params.agent_type,
                                "Created git worktree for agent isolation"
                            );
                            (wt_path.clone(), Some(wt_path))
                        }
                        Ok(o) => {
                            tracing::warn!(
                                stderr = %String::from_utf8_lossy(&o.stderr),
                                "Failed to create worktree, falling back to shared CWD"
                            );
                            (base_working_dir.clone(), None)
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "Failed to run git worktree command, falling back to shared CWD"
                            );
                            (base_working_dir.clone(), None)
                        }
                    }
                } else {
                    (base_working_dir.clone(), None)
                };

                // Fork shell executor for isolated CWD tracking
                let forked_shell = shell_executor.fork_for_subagent(effective_working_dir.clone());

                // Build environment info for the child
                let environment = EnvironmentInfo::builder()
                    .cwd(&effective_working_dir)
                    .context_window(context_window)
                    .max_output_tokens(16_384)
                    .build()
                    .map_err(cocode_error::boxed_err)?;

                // GAP-3: Resolve preloaded skills into context injections
                let mut injections = Vec::new();
                if !params.skills.is_empty() {
                    for skill_name in &params.skills {
                        if let Some(skill) = skill_manager.get(skill_name) {
                            injections.push(ContextInjection {
                                label: format!("agent-skill:{skill_name}"),
                                content: skill.prompt.clone(),
                                position: InjectionPosition::EndOfPrompt,
                            });
                            tracing::debug!(
                                agent_type = %params.agent_type,
                                skill = %skill_name,
                                "Preloaded skill into subagent context"
                            );
                        } else {
                            tracing::warn!(
                                agent_type = %params.agent_type,
                                skill = %skill_name,
                                "Skill not found for preload, skipping"
                            );
                        }
                    }
                }

                let mut ctx_builder = ConversationContext::builder()
                    .environment(environment)
                    .tool_names(tool_registry.tool_names());
                if !injections.is_empty() {
                    ctx_builder = ctx_builder.injections(injections);
                }
                let context = ctx_builder.build().map_err(cocode_error::boxed_err)?;

                // ── G1: Memory injection ───────────────────────────────
                let mut effective_prompt = params.prompt.clone();
                if let Some(ref scope) = params.memory {
                    let memory_dir = match scope {
                        MemoryScope::User => {
                            cocode_home.join("agent-memory").join(&params.agent_type)
                        }
                        MemoryScope::Project => effective_working_dir
                            .join(".cocode")
                            .join("agent-memory")
                            .join(&params.agent_type),
                        MemoryScope::Local => effective_working_dir
                            .join(".cocode")
                            .join("agent-memory-local")
                            .join(&params.agent_type),
                    };
                    tokio::fs::create_dir_all(&memory_dir).await.ok();
                    let memory_file = memory_dir.join("MEMORY.md");
                    if memory_file.exists() {
                        match tokio::fs::read_to_string(&memory_file).await {
                            Ok(content) => {
                                let truncated: String =
                                    content.lines().take(200).collect::<Vec<_>>().join("\n");
                                if !truncated.is_empty() {
                                    effective_prompt = format!(
                                        "## Agent Memory\n\n{truncated}\n\n{effective_prompt}"
                                    );
                                    tracing::debug!(
                                        agent_type = %params.agent_type,
                                        memory_lines = content.lines().count().min(200),
                                        "Injected agent memory into prompt"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    path = %memory_file.display(),
                                    "Failed to read agent MEMORY.md"
                                );
                            }
                        }
                    }
                }

                // ── G2: Skills filtering ───────────────────────────────
                if !params.skills.is_empty() {
                    let mut skill_prefix = String::new();
                    for skill_name in &params.skills {
                        if let Some(skill) = skill_manager.get(skill_name) {
                            skill_prefix.push_str(&format!(
                                "\n<skill name=\"{skill_name}\">\n{}\n</skill>\n",
                                skill.prompt
                            ));
                        } else {
                            tracing::warn!(
                                skill = %skill_name,
                                agent_type = %params.agent_type,
                                "Skill not found for agent"
                            );
                        }
                    }
                    if !skill_prefix.is_empty() {
                        effective_prompt = format!("{skill_prefix}\n{effective_prompt}");
                        tracing::debug!(
                            agent_type = %params.agent_type,
                            skills = ?params.skills,
                            "Injected skill prompts into agent prompt"
                        );
                    }
                }

                // Resolve selections: env var override > params.identity > parent
                // COCODE_SUBAGENT_MODEL env var takes highest priority
                let env_identity = std::env::var("COCODE_SUBAGENT_MODEL").ok().map(|m| {
                    if m.contains('/') {
                        match m.parse::<ModelSpec>() {
                            Ok(spec) => ExecutionIdentity::Spec(spec),
                            Err(_) => ExecutionIdentity::Inherit,
                        }
                    } else {
                        match m.to_lowercase().as_str() {
                            "haiku" => ExecutionIdentity::Role(ModelRole::Fast),
                            "sonnet" | "opus" => ExecutionIdentity::Role(ModelRole::Main),
                            _ => ExecutionIdentity::Inherit,
                        }
                    }
                });
                let effective_identity = env_identity.as_ref().or(params.identity.as_ref());
                let child_selections = if let Some(identity) = effective_identity {
                    let mut sel = selections.clone();
                    match identity {
                        ExecutionIdentity::Role(role) => {
                            // Use the model from the specified role
                            if let Some(role_sel) = sel.get(*role).cloned() {
                                sel.set(ModelRole::Main, role_sel);
                            }
                        }
                        ExecutionIdentity::Spec(spec) => {
                            sel.set(ModelRole::Main, RoleSelection::new(spec.clone()));
                        }
                        ExecutionIdentity::Inherit => {
                            // Keep parent selections as-is
                        }
                    }
                    sel
                } else {
                    selections.clone()
                };

                // Child loop config with permission mode from agent definition
                let child_config = LoopConfig {
                    max_turns: Some(params.max_turns.unwrap_or(10)),
                    permission_mode: params.permission_mode.unwrap_or_default(),
                    ..LoopConfig::default()
                };

                // ── G3: Agent-scoped hook registration ─────────────────
                let hook_group_id = format!(
                    "agent-{}-{}",
                    params.agent_type,
                    uuid::Uuid::new_v4().simple()
                );
                let has_agent_hooks = params.hooks.is_some();
                if let Some(ref agent_hooks) = params.hooks {
                    let hook_defs: Vec<HookDefinition> = agent_hooks
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, h)| {
                            // Remap Stop → SubagentStop
                            let event_str = if h.event == "Stop" || h.event == "stop" {
                                "SubagentStop"
                            } else {
                                &h.event
                            };
                            let event_type =
                                match event_str.parse::<cocode_protocol::HookEventType>() {
                                    Ok(et) => et,
                                    Err(e) => {
                                        tracing::warn!(
                                            event = %h.event,
                                            error = %e,
                                            "Skipping agent hook with unknown event type"
                                        );
                                        return None;
                                    }
                                };
                            let matcher = h
                                .matcher
                                .as_ref()
                                .map(|m| cocode_hooks::HookMatcher::Regex { pattern: m.clone() });
                            Some(HookDefinition {
                                name: format!("{hook_group_id}-hook-{idx}"),
                                event_type,
                                matcher,
                                handler: HookHandler::Command {
                                    command: h.command.clone(),
                                },
                                source: HookSource::Session,
                                enabled: true,
                                timeout_secs: h.timeout.unwrap_or(30) as i32,
                                once: false,
                                status_message: None,
                                group_id: None, // set by register_group
                                is_async: false,
                                force_sync_execution: false,
                            })
                        })
                        .collect();
                    if !hook_defs.is_empty() {
                        hook_registry.register_group(&hook_group_id, hook_defs);
                        tracing::debug!(
                            group_id = %hook_group_id,
                            agent_type = %params.agent_type,
                            "Registered agent-scoped hooks"
                        );
                    }
                }

                // Build the child loop — forward parent event_tx so the TUI
                // sees subagent progress, text deltas, and tool activity.
                let mut builder = AgentLoop::builder(
                    api_client,
                    model_hub,
                    child_selections,
                    tool_registry,
                    context,
                    parent_event_tx,
                )
                .config(child_config)
                .fallback_config(FallbackConfig::default())
                .hooks(hook_registry.clone())
                .cancel_token(params.cancel_token)
                .features(features)
                .web_search_config(web_search_config)
                .web_fetch_config(web_fetch_config)
                .permission_rules(permission_rules)
                .shell_executor(forked_shell)
                .skill_manager(skill_manager)
                .lsp_manager(lsp_manager)
                .is_subagent(true)
                .task_type_restrictions(params.task_type_restrictions)
                .cocode_home(cocode_home.clone())
                .team_store(team_store.clone())
                .team_mailbox(team_mailbox.clone());
                // NO .spawn_agent_fn() — prevents infinite recursion

                // Wire auto memory from parent into child loop
                if let Some(ref state) = params.auto_memory_state {
                    builder = builder.auto_memory_state(Arc::clone(state));
                }

                // Apply custom system prompt if the agent definition requested it
                if let Some(ref custom_prompt) = params.custom_system_prompt {
                    builder = builder.custom_system_prompt(custom_prompt.clone());
                }

                // Apply system prompt suffix (critical_reminder at system prompt level)
                if let Some(ref suffix) = params.system_prompt_suffix {
                    builder = builder.system_prompt_suffix(suffix.clone());
                }

                // Fork parent context if requested
                if params.fork_context {
                    builder = builder.message_history(message_history.clone());
                }

                let mut loop_instance = builder.build();

                // ── Agent identity propagation ─────────────────────────
                let parent = cocode_subagent::current_agent();
                let parent_id = parent.as_ref().map(|a| a.agent_id.clone());
                let parent_depth = parent.as_ref().map_or(0, |a| a.depth);
                let identity = cocode_subagent::AgentIdentity {
                    agent_id: uuid::Uuid::new_v4().to_string(),
                    agent_type: params.agent_type.clone(),
                    parent_agent_id: parent_id,
                    depth: parent_depth + 1,
                    name: params.name.clone(),
                    team_name: params.team_name.clone(),
                    color: params.color.clone(),
                    plan_mode_required: params.plan_mode_required,
                };
                let result = cocode_subagent::CURRENT_AGENT
                    .scope(identity, loop_instance.run(&effective_prompt))
                    .await;

                // ── G3: Unregister agent-scoped hooks ──────────────────
                if has_agent_hooks {
                    hook_registry.unregister_group(&hook_group_id);
                    tracing::debug!(
                        group_id = %hook_group_id,
                        "Unregistered agent-scoped hooks"
                    );
                }

                // ── G5: Worktree cleanup ───────────────────────────────
                if let Some(ref wt_path) = worktree_path {
                    let remove_output = tokio::process::Command::new("git")
                        .args(["worktree", "remove", "--force", &wt_path.to_string_lossy()])
                        .current_dir(&base_working_dir)
                        .output()
                        .await;
                    match remove_output {
                        Ok(o) if o.status.success() => {
                            tracing::info!(
                                path = %wt_path.display(),
                                "Removed git worktree after agent completion"
                            );
                        }
                        Ok(o) => {
                            tracing::warn!(
                                stderr = %String::from_utf8_lossy(&o.stderr),
                                path = %wt_path.display(),
                                "Failed to remove git worktree"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                path = %wt_path.display(),
                                "Failed to run git worktree remove"
                            );
                        }
                    }
                }

                let result = result.map_err(cocode_error::boxed_err)?;
                Ok(result.final_text)
            })
        })
    }

    /// Collect background agent task info from the subagent manager.
    ///
    /// Called before each `loop_instance.run()` to populate the system reminder
    /// feedback loop with current agent statuses.
    async fn collect_background_agent_tasks(&self) -> Vec<BackgroundTaskInfo> {
        let mut mgr = self.subagent_manager.lock().await;
        let killed = self.killed_agents.lock().await;

        // Promote Failed → Killed for agents explicitly stopped via TaskStop.
        // After cancellation the completion handler marks them Failed; this
        // upgrades to Killed so GC, status reporting, and match arms work.
        if !killed.is_empty() {
            mgr.promote_killed(&killed);
        }

        // Auto-GC stale agents (completed/failed/killed for >5 min).
        mgr.gc_stale(std::time::Duration::from_secs(300));

        mgr.agent_infos()
            .into_iter()
            .map(|info| {
                let is_completed = matches!(
                    info.status,
                    SubagentStatus::Completed | SubagentStatus::Failed | SubagentStatus::Killed
                );
                BackgroundTaskInfo {
                    task_id: info.id,
                    task_type: BackgroundTaskType::AsyncAgent,
                    command: info.name.unwrap_or_else(|| info.agent_type.clone()),
                    status: match info.status {
                        SubagentStatus::Running | SubagentStatus::Backgrounded => {
                            BackgroundTaskStatus::Running
                        }
                        SubagentStatus::Completed => BackgroundTaskStatus::Completed,
                        SubagentStatus::Failed | SubagentStatus::Killed => {
                            BackgroundTaskStatus::Failed
                        }
                    },
                    exit_code: None,
                    has_new_output: false,
                    progress_message: None,
                    is_completion_notification: is_completed,
                    delta_summary: None,
                    description: None,
                }
            })
            .collect()
    }

    /// Build the callback for firing SubagentStop hooks when background agents complete.
    fn build_background_stop_hook_fn(&self) -> cocode_subagent::BackgroundStopHookFn {
        let hook_registry = self.hook_registry.clone();
        let session_id = self.session.id.clone();
        let cwd = self.session.working_dir.clone();

        Arc::new(move |agent_type: String, agent_id: String| {
            let hook_registry = hook_registry.clone();
            let session_id = session_id.clone();
            let cwd = cwd.clone();
            Box::pin(async move {
                let hook_ctx = cocode_hooks::HookContext::new(
                    cocode_hooks::HookEventType::SubagentStop,
                    session_id,
                    cwd,
                )
                .with_metadata("agent_type", agent_type)
                .with_metadata("agent_id", agent_id);
                let outcomes = hook_registry.execute(&hook_ctx).await;
                for outcome in &outcomes {
                    if let cocode_hooks::HookResult::Reject { reason } = &outcome.result {
                        tracing::warn!(
                            hook = %outcome.hook_name,
                            %reason,
                            "SubagentStop hook rejected (ignored, background agent already completed)"
                        );
                    }
                }
            })
        })
    }

    /// Build the `SpawnAgentFn` closure that the Task tool calls.
    ///
    /// Bridges `SpawnAgentInput` (tools layer) to `SpawnInput` (subagent layer)
    /// and delegates to `SubagentManager::spawn_full()`.
    fn build_spawn_agent_fn(&self) -> cocode_tools::SpawnAgentFn {
        let subagent_manager = self.subagent_manager.clone();

        Arc::new(move |input: cocode_tools::SpawnAgentInput| {
            let subagent_manager = subagent_manager.clone();

            Box::pin(async move {
                // Convert model string → ExecutionIdentity
                let identity = input.model.as_deref().map(|m| {
                    if m.contains('/') {
                        // Full spec: "provider/model"
                        match m.parse::<ModelSpec>() {
                            Ok(spec) => ExecutionIdentity::Spec(spec),
                            Err(_) => ExecutionIdentity::Inherit,
                        }
                    } else {
                        // Short name: map to role
                        match m.to_lowercase().as_str() {
                            "haiku" => ExecutionIdentity::Role(ModelRole::Fast),
                            "sonnet" | "opus" => ExecutionIdentity::Role(ModelRole::Main),
                            _ => ExecutionIdentity::Inherit,
                        }
                    }
                });

                let spawn_input = cocode_subagent::SpawnInput {
                    agent_type: input.agent_type,
                    prompt: input.prompt,
                    identity,
                    max_turns: input.max_turns,
                    run_in_background: input.run_in_background,
                    allowed_tools: input.allowed_tools,
                    resume_from: input.resume_from,
                    name: input.name,
                    team_name: input.team_name,
                    mode: input.mode,
                    cwd: input.cwd,
                    isolation_override: input.isolation,
                    description: input.description,
                };

                let mut mgr = subagent_manager.lock().await;
                let result = mgr
                    .spawn_full(spawn_input)
                    .await
                    .map_err(cocode_error::boxed_err)?;

                Ok(cocode_tools::SpawnAgentResult {
                    agent_id: result.agent_id,
                    output: result.output,
                    output_file: result.background.as_ref().map(|bg| bg.output_file.clone()),
                    cancel_token: result.cancel_token,
                    color: result.color,
                })
            })
        })
    }

    // ==========================================================
    // Streaming Turn API
    // ==========================================================

    /// Run a single turn with the given user input, streaming events to the provided channel.
    ///
    /// This is similar to `run_turn` but forwards all events to the provided channel
    /// instead of handling them internally. This enables real-time streaming to a TUI
    /// or other consumer.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tokio::sync::mpsc;
    /// use cocode_protocol::LoopEvent;
    ///
    /// let (event_tx, mut event_rx) = mpsc::channel::<LoopEvent>(256);
    ///
    /// // Spawn task to handle events
    /// tokio::spawn(async move {
    ///     while let Some(event) = event_rx.recv().await {
    ///         // Process event (update TUI, etc.)
    ///     }
    /// });
    ///
    /// let result = state.run_turn_streaming("Hello!", event_tx).await?;
    /// ```
    pub async fn run_turn_streaming(
        &mut self,
        user_input: &str,
        event_tx: mpsc::Sender<LoopEvent>,
    ) -> Result<TurnResult, cocode_error::BoxedError> {
        info!(
            session_id = %self.session.id,
            input_len = user_input.len(),
            "Running turn with streaming"
        );

        // Update session activity
        self.session.touch();

        // Build environment info
        let environment = EnvironmentInfo::builder()
            .cwd(&self.session.working_dir)
            .context_window(self.context_window)
            .max_output_tokens(16_384)
            .build()
            .map_err(boxed_err)?;

        // Build conversation context
        let mut ctx_builder = ConversationContext::builder()
            .environment(environment)
            .tool_names(self.tool_registry.tool_names())
            .injections(self.build_suffix_injections());

        if let Some(style_config) = self.resolve_output_style() {
            ctx_builder = ctx_builder.output_style(style_config);
        }

        let context = ctx_builder.build().map_err(boxed_err)?;

        // Set the execute_fn, tool list, and event_tx on the subagent manager (fresh per-turn)
        {
            let execute_fn = self.build_execute_fn(event_tx.clone());
            let mut mgr = self.subagent_manager.lock().await;
            mgr.set_execute_fn(execute_fn);
            mgr.set_all_tools(self.tool_registry.tool_names());
            mgr.set_event_tx(event_tx.clone());
            mgr.set_background_stop_hook_fn(self.build_background_stop_hook_fn());
        }

        // Build and run the agent loop with the provided event channel
        // Clone selections so the loop has its own copy (isolation)
        // Pass queued commands for consume-then-remove steering injection
        let mut builder = AgentLoop::builder(
            self.api_client.clone(),
            self.model_hub.clone(),
            self.session.selections.clone(),
            self.tool_registry.clone(),
            context,
            event_tx,
        )
        .config(self.loop_config.clone())
        .fallback_config(FallbackConfig::default())
        .hooks(self.hook_registry.clone())
        .cancel_token(self.cancel_token.clone())
        .queued_commands(self.queued_commands.clone())
        .features(self.config.features.clone())
        .web_search_config(self.config.web_search_config.clone())
        .web_fetch_config(self.config.web_fetch_config.clone())
        .permission_rules(self.permission_rules.clone())
        .shell_executor(self.shell_executor.clone())
        .skill_manager(self.skill_manager.clone())
        .otel_manager(self.otel_manager.clone())
        .lsp_manager(self.lsp_manager.clone())
        .spawn_agent_fn(self.build_spawn_agent_fn())
        .plan_mode_state(self.plan_mode_state.clone())
        .question_responder(self.question_responder.clone())
        .approval_store(self.shared_approval_store.clone())
        .reminder_file_tracker_state(self.reminder_file_tracker_state.clone())
        .message_history(self.message_history.clone())
        .cocode_home(self.config.cocode_home.clone())
        .killed_agents(self.killed_agents.clone())
        .auto_memory_state(Arc::clone(&self.auto_memory_state))
        .team_store(Arc::clone(&self.team_store))
        .team_mailbox(Arc::clone(&self.team_mailbox));

        // Wire snapshot manager for rewind support (skill turn)
        if let Some(ref sm) = self.snapshot_manager {
            builder = builder.snapshot_manager(sm.clone());
        }

        let mut loop_instance = builder.build();

        // Push background agent task info for system reminders
        loop_instance.set_background_agent_tasks(self.collect_background_agent_tasks().await);

        // Queued commands are consumed as steering in core_message_loop Step 6.5.
        // No post-idle re-execution needed — steering asks the model to address
        // each message ("Please address this message and continue").
        // The shared Arc<Mutex> means any commands queued by the TUI driver during
        // the turn are visible to the loop immediately — no take-back needed.
        let result = loop_instance.run(user_input).await.map_err(boxed_err)?;

        // Extract todos state from the loop
        if let Some(todos) = loop_instance.take_todos() {
            self.todos = todos;
        }

        // Extract structured tasks state from the loop
        if let Some(tasks) = loop_instance.take_structured_tasks() {
            self.structured_tasks = tasks;
        }

        // Extract cron jobs state from the loop
        if let Some(jobs) = loop_instance.take_cron_jobs() {
            self.cron_jobs = jobs;
        }

        // Sync message history back from loop (persists across turns)
        self.message_history = loop_instance.message_history().clone();

        // Extract plan mode state from the loop (persists across turns)
        if let Some(plan_state) = loop_instance.take_plan_mode_state() {
            self.plan_mode_state = plan_state;
        }

        // Extract file tracker state from the loop (persists across turns)
        self.reminder_file_tracker_state = loop_instance.reminder_file_tracker_snapshot().await;

        // Update totals
        self.total_turns += result.turns_completed;
        self.total_input_tokens += result.total_input_tokens;
        self.total_output_tokens += result.total_output_tokens;

        Ok(TurnResult::from_loop_result(&result))
    }

    /// Run partial compaction (summarize) from a specific turn onward.
    ///
    /// This summarizes the conversation from `from_turn_number` to the end,
    /// replacing those turns with an LLM-generated summary while keeping all
    /// earlier turns intact.
    ///
    /// If `user_context` is provided, it is included in the summarization prompt
    /// to guide what the summary should focus on.
    pub async fn run_partial_compact(
        &mut self,
        from_turn_number: i32,
        event_tx: mpsc::Sender<LoopEvent>,
        user_context: Option<&str>,
    ) -> anyhow::Result<PartialCompactResult> {
        info!(
            from_turn_number,
            ?user_context,
            "Running partial compaction (summarize from turn)"
        );

        let _ = event_tx.send(LoopEvent::CompactionStarted).await;

        // 1. Build conversation text from turns at/after from_turn_number.
        // Extract Message objects (role + content) from turns, matching the
        // format used by the existing compact() in core/loop driver.
        let conversation_text: String = self
            .message_history
            .turns()
            .iter()
            .filter(|t| t.number >= from_turn_number)
            .flat_map(|t| {
                let mut msgs = vec![&t.user_message.inner];
                if let Some(ref asst) = t.assistant_message {
                    msgs.push(&asst.inner);
                }
                msgs
            })
            .map(|m| format!("{m:?}"))
            .collect::<Vec<_>>()
            .join("\n");

        if conversation_text.is_empty() {
            anyhow::bail!("No turns found at or after turn {from_turn_number}");
        }

        // 2. Build summarization prompt
        let max_output_tokens = 4096;
        let system_prompt = cocode_loop::build_compact_instructions(max_output_tokens);
        let context_instruction = match user_context {
            Some(ctx) if !ctx.is_empty() => {
                format!("\n\nThe user has requested that the summary focus on: {ctx}")
            }
            _ => String::new(),
        };
        let user_prompt = format!(
            "Please summarize the following conversation:\n\n---\n\n{conversation_text}\n\n---\n\nProvide your summary using the required section format.{context_instruction}"
        );

        let summary_messages = vec![
            cocode_api::LanguageModelMessage::system(&system_prompt),
            cocode_api::LanguageModelMessage::user_text(&user_prompt),
        ];

        // 3. Call LLM for summary
        let session_id = format!("summarize-{from_turn_number}");
        let turn_count = self.message_history.turn_count();
        let (ctx, compact_model) = self
            .model_hub
            .prepare_compact_with_selections(&self.session.selections, &session_id, turn_count)
            .map_err(|e| anyhow::anyhow!("Failed to prepare compact model: {e}"))?;

        let summary_request = cocode_api::RequestBuilder::new(ctx)
            .messages(summary_messages)
            .max_tokens(max_output_tokens as u64)
            .build();

        let response = self
            .api_client
            .generate(&*compact_model, summary_request)
            .await
            .map_err(|e| anyhow::anyhow!("Summarization LLM call failed: {e}"))?;

        let summary_text: String = response
            .content
            .iter()
            .filter_map(|b| match b {
                cocode_api::AssistantContentPart::Text(tp) => Some(tp.text.as_str()),
                _ => None,
            })
            .collect();

        if summary_text.is_empty() {
            anyhow::bail!("Summarization produced empty output");
        }

        // 4. Apply: truncate turns from from_turn_number, store summary
        let pre_tokens = self.message_history.estimate_tokens();

        // Calculate keep_turns: number of turns AFTER the summarized portion
        // (apply_compaction_with_metadata keeps the LAST keep_turns turns).
        // We want to keep turns BEFORE from_turn_number, so:
        //   keep_turns = number of turns with number < from_turn_number
        let keep_turns = self
            .message_history
            .turns()
            .iter()
            .filter(|t| t.number < from_turn_number)
            .count() as i32;

        // Use the standard compaction path, which:
        // - Stores the summary
        // - Removes older turns (keeping the last `keep_turns`)
        // - Records compaction boundary
        //
        // NOTE: apply_compaction_with_metadata keeps the LAST N turns.
        // For partial compact, we want the FIRST N turns (before from_turn).
        // So instead, we truncate from from_turn and set the summary directly.
        self.message_history.truncate_from_turn(from_turn_number);

        // Append to or replace the compacted summary
        let existing = self.message_history.compacted_summary().map(String::from);
        let final_summary = match existing {
            Some(prev) => format!("{prev}\n\n---\n\n{summary_text}"),
            None => summary_text,
        };
        let remaining_turns = self.message_history.turn_count();
        self.message_history.apply_compaction_with_metadata(
            final_summary,
            remaining_turns,
            "summarize",
            pre_tokens.saturating_sub(self.message_history.estimate_tokens()),
            cocode_protocol::CompactTrigger::Manual,
            pre_tokens,
            None,
            true,
        );

        let post_tokens = self.message_history.estimate_tokens();

        // 5. Set compaction boundary on snapshot manager
        if let Some(ref sm) = self.snapshot_manager {
            sm.set_compaction_boundary(from_turn_number).await;
        }

        let _ = event_tx
            .send(LoopEvent::CompactionCompleted {
                removed_messages: 0,
                summary_tokens: post_tokens,
            })
            .await;

        // Rebuild todos and file tracker after partial compaction
        // This ensures state consistency with the retained history
        self.rebuild_todos_from_history();
        self.rebuild_reminder_file_tracker_from_history();

        info!(
            from_turn_number,
            pre_tokens, post_tokens, keep_turns, "Partial compaction completed"
        );

        Ok(PartialCompactResult {
            from_turn: from_turn_number,
            summary_tokens: post_tokens,
        })
    }
}

/// Convert a `MarketplaceSource` to `(source_type, source_display)` for UI.
fn marketplace_source_display(source: &cocode_plugin::MarketplaceSource) -> (String, String) {
    match source {
        cocode_plugin::MarketplaceSource::Github { repo, .. } => {
            ("github".to_string(), repo.clone())
        }
        cocode_plugin::MarketplaceSource::Git { url, .. } => ("git".to_string(), url.clone()),
        cocode_plugin::MarketplaceSource::File { path } => {
            ("file".to_string(), path.display().to_string())
        }
        cocode_plugin::MarketplaceSource::Directory { path } => {
            ("directory".to_string(), path.display().to_string())
        }
        cocode_plugin::MarketplaceSource::Url { url } => ("url".to_string(), url.clone()),
    }
}

/// Convert config extra marketplace entries to plugin-crate entries.
fn convert_extra_marketplaces(
    extras: &[cocode_config::json_config::ExtraMarketplaceConfig],
) -> Vec<cocode_plugin::ExtraMarketplaceEntry> {
    use cocode_config::json_config::MarketplaceSourceConfig;
    use cocode_plugin::MarketplaceSource;

    extras
        .iter()
        .map(|cfg| {
            let source = match &cfg.source {
                MarketplaceSourceConfig::Github { repo, git_ref } => MarketplaceSource::Github {
                    repo: repo.clone(),
                    git_ref: git_ref.clone(),
                },
                MarketplaceSourceConfig::Git { url, git_ref } => MarketplaceSource::Git {
                    url: url.clone(),
                    git_ref: git_ref.clone(),
                },
                MarketplaceSourceConfig::Directory { path } => MarketplaceSource::Directory {
                    path: std::path::PathBuf::from(path),
                },
                MarketplaceSourceConfig::Url { url } => MarketplaceSource::Url { url: url.clone() },
            };
            cocode_plugin::ExtraMarketplaceEntry {
                name: cfg.name.clone(),
                source,
                auto_update: cfg.auto_update,
            }
        })
        .collect()
}

/// Convert config hook entries to hook definitions.
///
/// Each matcher group can contain multiple handlers; each handler becomes
/// a separate `HookDefinition`.
fn convert_config_hooks(
    hooks_map: &std::collections::HashMap<
        cocode_protocol::HookEventType,
        Vec<cocode_config::json_config::HookMatcherGroup>,
    >,
) -> Vec<cocode_hooks::HookDefinition> {
    use cocode_config::json_config::HookHandlerConfig;

    let mut defs = Vec::new();
    for (event_key, groups) in hooks_map {
        let event_type = event_key.clone();

        for (g_idx, group) in groups.iter().enumerate() {
            // Parse matcher: Claude Code matchers are regex patterns
            let matcher = group.matcher.as_deref().and_then(|m| {
                if m.is_empty() {
                    None // empty string = match all
                } else {
                    Some(cocode_hooks::HookMatcher::Regex {
                        pattern: m.to_string(),
                    })
                }
            });

            // Each handler becomes a separate HookDefinition
            for (h_idx, handler_cfg) in group.hooks.iter().enumerate() {
                let (handler, timeout_secs, once, status_message) = match handler_cfg {
                    HookHandlerConfig::Command {
                        command,
                        timeout,
                        once,
                        status_message,
                    } => (
                        cocode_hooks::HookHandler::Command {
                            command: command.clone(),
                        },
                        timeout.unwrap_or(30),
                        once.unwrap_or(false),
                        status_message.clone(),
                    ),
                    HookHandlerConfig::Prompt {
                        prompt,
                        model,
                        timeout,
                        once,
                    } => (
                        cocode_hooks::HookHandler::Prompt {
                            template: prompt.clone(),
                            model: model.clone(),
                        },
                        timeout.unwrap_or(30),
                        once.unwrap_or(false),
                        None,
                    ),
                    HookHandlerConfig::Agent {
                        prompt,
                        model: _,
                        timeout,
                        once,
                    } => (
                        cocode_hooks::HookHandler::Agent {
                            max_turns: 50,
                            prompt: Some(prompt.clone()),
                            timeout: timeout.unwrap_or(60),
                        },
                        timeout.unwrap_or(60),
                        once.unwrap_or(false),
                        None,
                    ),
                };

                defs.push(cocode_hooks::HookDefinition {
                    name: format!("config-{event_key}-{g_idx}-{h_idx}"),
                    event_type: event_type.clone(),
                    matcher: matcher.clone(),
                    handler,
                    source: cocode_hooks::HookSource::Session,
                    enabled: true,
                    timeout_secs,
                    once,
                    status_message,
                    group_id: None,
                    is_async: false,
                    force_sync_execution: false,
                });
            }
        }
    }
    defs
}

/// Detect whether a directory is inside a git repository by walking up
/// the directory tree looking for a `.git` directory or file.
fn detect_git_repo(path: &std::path::Path) -> bool {
    let mut current = Some(path);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return true;
        }
        current = dir.parent();
    }
    false
}

#[cfg(test)]
#[path = "state.test.rs"]
mod tests;
