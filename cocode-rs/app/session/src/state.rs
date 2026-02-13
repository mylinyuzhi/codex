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
use cocode_hooks::HookRegistry;
use cocode_loop::AgentLoop;
use cocode_loop::CompactionConfig;
use cocode_loop::FallbackConfig;
use cocode_loop::LoopConfig;
use cocode_loop::LoopResult;
use cocode_message::MessageHistory;
use cocode_protocol::LoopEvent;
use cocode_protocol::ProviderType;
use cocode_protocol::RoleSelection;
use cocode_protocol::RoleSelections;
use cocode_protocol::ThinkingLevel;
use cocode_protocol::TokenUsage;
use cocode_protocol::model::ModelRole;
use cocode_rmcp_client::RmcpClient;
use cocode_shell::ShellExecutor;
use cocode_skill::SkillInterface;
use cocode_skill::SkillManager;
use cocode_subagent::SubagentManager;
use cocode_system_reminder::QueuedCommandInfo;
use cocode_tools::ToolRegistry;

use std::sync::Mutex;

use serde::Deserialize;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::info;

use crate::session::Session;

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
        }
    }
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
/// use cocode_protocol::ProviderType;
/// use std::sync::Arc;
/// use std::path::PathBuf;
///
/// let session = Session::new(PathBuf::from("."), "gpt-5", ProviderType::Openai);
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
    provider_type: ProviderType,

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
    subagent_manager: SubagentManager,

    /// Active MCP clients from plugin servers (kept alive for session lifetime).
    _mcp_clients: Vec<Arc<RmcpClient>>,

    /// Configuration snapshot (immutable for session lifetime).
    config: Arc<Config>,

    /// Pre-configured permission rules loaded from config.
    permission_rules: Vec<cocode_tools::PermissionRule>,

    /// Current task list (updated by TodoWrite tool via ContextModifier).
    todos: serde_json::Value,

    /// Optional OTel manager for metrics and traces.
    otel_manager: Option<Arc<cocode_otel::OtelManager>>,
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

        // Get provider type from session's ModelSpec.
        // IMPORTANT: This assumes the caller used ModelSpec::with_type() (not ModelSpec::new())
        // so that provider_type comes from config, not from string-based heuristic resolution.
        // All current callers (tui_runner, chat, session manager) satisfy this requirement.
        let provider_type = primary_model.model.provider_type;

        // Get model context window from Config snapshot (default to 200k)
        let context_window = config
            .resolve_model_info(&provider_name, &model_name)
            .and_then(|info| info.context_window)
            .map(|cw| cw as i32)
            .unwrap_or(200_000);

        // Create API client
        let api_client = ApiClient::new();

        // Ensure main model is set in session selections
        let mut session = session;
        let main_spec = cocode_protocol::model::ModelSpec::new(&provider_name, &model_name);
        if session.selections.get(ModelRole::Main).is_none() {
            session
                .selections
                .set(ModelRole::Main, RoleSelection::new(main_spec));
        }

        // Create ModelHub with Config snapshot (role-agnostic, just for model caching)
        let model_hub = Arc::new(ModelHub::new(config.clone()));

        // Create tool registry with built-in tools
        let mut tool_registry = ToolRegistry::new();
        cocode_tools::builtin::register_builtin_tools(&mut tool_registry);

        // Create hook registry and load hooks from config
        let hook_registry = HookRegistry::new();
        let config_hooks = convert_config_hooks(&config.hooks);
        if !config_hooks.is_empty() {
            tracing::info!(count = config_hooks.len(), "Loaded hooks from config");
            hook_registry.register_all(config_hooks);
        }

        // Load hooks from TOML file if it exists (~/.cocode/hooks.toml)
        let hooks_toml_path = config.cocode_home.join("hooks.toml");
        if hooks_toml_path.is_file() {
            match cocode_hooks::load_hooks_from_toml(&hooks_toml_path) {
                Ok(toml_hooks) => {
                    tracing::info!(
                        count = toml_hooks.len(),
                        path = %hooks_toml_path.display(),
                        "Loaded hooks from TOML"
                    );
                    hook_registry.register_all(toml_hooks);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load hooks.toml");
                }
            }
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

        // Create subagent manager for plugin agent contributions
        let mut subagent_manager = SubagentManager::new();

        // Load plugins from standard directories and installed plugin cache
        let plugin_config = cocode_plugin::PluginIntegrationConfig::with_defaults(
            &config.cocode_home,
            Some(&session.working_dir),
        );
        let plugin_registry = cocode_plugin::integrate_plugins(
            &plugin_config,
            &mut skill_manager,
            &hook_registry,
            Some(&mut subagent_manager),
        );
        let plugin_registry = if plugin_registry.is_empty() {
            None
        } else {
            Some(plugin_registry)
        };

        // Connect plugin MCP servers (async: starts server processes and registers tools)
        let mcp_clients = if let Some(ref pr) = plugin_registry {
            cocode_plugin::connect_plugin_mcp_servers(pr, &mut tool_registry, &config.cocode_home)
                .await
        } else {
            Vec::new()
        };

        // Build loop config from session
        let loop_config = LoopConfig {
            max_turns: session.max_turns,
            ..LoopConfig::default()
        };

        // Load permission rules from config snapshot
        let permission_rules = match config.permissions {
            Some(ref perms) => cocode_tools::PermissionRuleEvaluator::rules_from_config(
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

        Ok(Self {
            session,
            message_history: MessageHistory::new(),
            tool_registry: Arc::new(tool_registry),
            hook_registry: Arc::new(hook_registry),
            skills,
            skill_manager: Arc::new(skill_manager),
            plugin_registry,
            api_client,
            model_hub,
            subagent_manager,
            _mcp_clients: mcp_clients,
            cancel_token: CancellationToken::new(),
            loop_config,
            total_turns: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            context_window,
            provider_type,
            shell_executor,
            queued_commands: Arc::new(Mutex::new(Vec::new())),
            system_prompt_suffix: None,
            config,
            permission_rules,
            todos: serde_json::json!([]),
            otel_manager,
        })
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
        let model_name = self
            .session
            .model()
            .ok_or_else(|| anyhow::anyhow!("Session has no main model"))?;
        let environment = EnvironmentInfo::builder()
            .cwd(&self.session.working_dir)
            .model(model_name)
            .context_window(self.context_window)
            .max_output_tokens(16_384)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build environment: {e}"))?;

        // Build conversation context
        let context = ConversationContext::builder()
            .environment(environment)
            .tool_names(self.tool_registry.tool_names())
            .injections(self.build_suffix_injections())
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build context: {e}"))?;

        // Build and run the agent loop
        // Clone selections so the loop has its own copy (isolation)
        let mut loop_instance = AgentLoop::builder()
            .api_client(self.api_client.clone())
            .model_hub(self.model_hub.clone())
            .selections(self.session.selections.clone())
            .tool_registry(self.tool_registry.clone())
            .context(context)
            .config(self.loop_config.clone())
            .fallback_config(FallbackConfig::default())
            .compaction_config(CompactionConfig::default())
            .hooks(self.hook_registry.clone())
            .event_tx(event_tx)
            .cancel_token(self.cancel_token.clone())
            .queued_commands(self.queued_commands.clone())
            .features(self.config.features.clone())
            .web_search_config(self.config.web_search_config.clone())
            .web_fetch_config(self.config.web_fetch_config.clone())
            .permission_rules(self.permission_rules.clone())
            .shell_executor(self.shell_executor.clone())
            .skill_manager(self.skill_manager.clone())
            .otel_manager(self.otel_manager.clone())
            .build();

        let result = loop_instance.run(user_input).await?;

        // Extract todos state before dropping the loop
        if let Some(todos) = loop_instance.take_todos() {
            self.todos = todos;
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
                    .map_err(|e| anyhow::anyhow!("Invalid model spec '{}': {}", model_name, e))?
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
    ) -> anyhow::Result<TurnResult> {
        let saved_selection = if let Some(model_name) = model_override {
            let current = self.session.selections.get(ModelRole::Main).cloned();
            let spec = if model_name.contains('/') {
                model_name
                    .parse::<cocode_protocol::model::ModelSpec>()
                    .map_err(|e| anyhow::anyhow!("Invalid model spec '{}': {}", model_name, e))?
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

    /// Get the subagent manager.
    pub fn subagent_manager(&self) -> &SubagentManager {
        &self.subagent_manager
    }

    /// Get mutable access to the subagent manager.
    pub fn subagent_manager_mut(&mut self) -> &mut SubagentManager {
        &mut self.subagent_manager
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

    /// Get the provider type for this session.
    pub fn provider_type(&self) -> ProviderType {
        self.provider_type
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
    /// A tuple of (model, provider_type) for the role, or None if no selection exists.
    pub fn get_model_for_role(
        &self,
        role: ModelRole,
    ) -> anyhow::Result<Option<(Arc<dyn hyper_sdk::Model>, ProviderType)>> {
        match self
            .model_hub
            .get_model_for_role_with_selections(role, &self.session.selections)
        {
            Ok((model, provider_type)) => Ok(Some((model, provider_type))),
            Err(e) => {
                // If error is "no model configured", return None instead of error
                if e.is_no_model_configured() {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("{}", e))
                }
            }
        }
    }

    /// Get the main model (shorthand for get_model_for_role(ModelRole::Main)).
    ///
    /// Returns the main model using the session's selections.
    pub fn main_model(&self) -> anyhow::Result<Arc<dyn hyper_sdk::Model>> {
        self.model_hub
            .get_model_for_role_with_selections(ModelRole::Main, &self.session.selections)
            .map(|(m, _)| m)
            .map_err(|e| anyhow::anyhow!("{}", e))
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
    ) -> Option<hyper_sdk::options::ProviderOptions> {
        let thinking_level = self.thinking_level(role)?;
        let default_model_info = cocode_protocol::ModelInfo::default();
        let model_info = model_info.unwrap_or(&default_model_info);
        cocode_api::thinking_convert::to_provider_options(
            &thinking_level,
            model_info,
            self.provider_type,
        )
    }

    // ==========================================================
    // System Prompt Suffix API
    // ==========================================================

    /// Set a suffix to append to the end of the system prompt.
    pub fn set_system_prompt_suffix(&mut self, suffix: String) {
        self.system_prompt_suffix = Some(suffix);
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
        self.queued_commands.lock().unwrap().push(cmd);
        id
    }

    /// Get the number of queued commands.
    pub fn queued_count(&self) -> usize {
        self.queued_commands.lock().unwrap().len()
    }

    /// Take all queued commands (for passing to AgentLoop).
    pub fn take_queued_commands(&self) -> Vec<QueuedCommandInfo> {
        std::mem::take(&mut *self.queued_commands.lock().unwrap())
    }

    /// Clear all queued commands.
    pub fn clear_queued_commands(&self) {
        self.queued_commands.lock().unwrap().clear();
    }

    /// Get a shared handle to the queued commands.
    ///
    /// The TUI driver uses this to push commands while a turn is running,
    /// without needing `&mut self`.
    pub fn shared_queued_commands(&self) -> Arc<Mutex<Vec<QueuedCommandInfo>>> {
        self.queued_commands.clone()
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
    ) -> anyhow::Result<TurnResult> {
        info!(
            session_id = %self.session.id,
            input_len = user_input.len(),
            "Running turn with streaming"
        );

        // Update session activity
        self.session.touch();

        // Build environment info
        let model_name = self
            .session
            .model()
            .ok_or_else(|| anyhow::anyhow!("Session has no main model"))?;
        let environment = EnvironmentInfo::builder()
            .cwd(&self.session.working_dir)
            .model(model_name)
            .context_window(self.context_window)
            .max_output_tokens(16_384)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build environment: {e}"))?;

        // Build conversation context
        let context = ConversationContext::builder()
            .environment(environment)
            .tool_names(self.tool_registry.tool_names())
            .injections(self.build_suffix_injections())
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build context: {e}"))?;

        // Build and run the agent loop with the provided event channel
        // Clone selections so the loop has its own copy (isolation)
        // Pass queued commands for consume-then-remove steering injection
        let mut loop_instance = AgentLoop::builder()
            .api_client(self.api_client.clone())
            .model_hub(self.model_hub.clone())
            .selections(self.session.selections.clone())
            .tool_registry(self.tool_registry.clone())
            .context(context)
            .config(self.loop_config.clone())
            .fallback_config(FallbackConfig::default())
            .compaction_config(CompactionConfig::default())
            .hooks(self.hook_registry.clone())
            .event_tx(event_tx)
            .cancel_token(self.cancel_token.clone())
            .queued_commands(self.queued_commands.clone())
            .features(self.config.features.clone())
            .web_search_config(self.config.web_search_config.clone())
            .web_fetch_config(self.config.web_fetch_config.clone())
            .permission_rules(self.permission_rules.clone())
            .skill_manager(self.skill_manager.clone())
            .otel_manager(self.otel_manager.clone())
            .build();

        // Queued commands are consumed as steering in core_message_loop Step 6.5.
        // No post-idle re-execution needed — steering asks the model to address
        // each message ("Please address this message and continue").
        // The shared Arc<Mutex> means any commands queued by the TUI driver during
        // the turn are visible to the loop immediately — no take-back needed.
        let result = loop_instance.run_and_process_queue(user_input).await?;

        // Extract todos state from the loop
        if let Some(todos) = loop_instance.take_todos() {
            self.todos = todos;
        }

        // Update totals
        self.total_turns += result.turns_completed;
        self.total_input_tokens += result.total_input_tokens;
        self.total_output_tokens += result.total_output_tokens;

        Ok(TurnResult::from_loop_result(&result))
    }
}

/// Convert config hook entries to hook definitions.
///
/// Each `HookConfig` can contain multiple handlers; each handler becomes
/// a separate `HookDefinition`.
fn convert_config_hooks(
    configs: &[cocode_config::json_config::HookConfig],
) -> Vec<cocode_hooks::HookDefinition> {
    use cocode_config::json_config::HookHandlerConfig;

    let mut defs = Vec::new();
    for (idx, cfg) in configs.iter().enumerate() {
        // Parse event type
        let event_type = match cfg.event.as_str() {
            "pre_tool_use" => cocode_hooks::HookEventType::PreToolUse,
            "post_tool_use" => cocode_hooks::HookEventType::PostToolUse,
            "post_tool_use_failure" => cocode_hooks::HookEventType::PostToolUseFailure,
            "user_prompt_submit" => cocode_hooks::HookEventType::UserPromptSubmit,
            "session_start" => cocode_hooks::HookEventType::SessionStart,
            "session_end" => cocode_hooks::HookEventType::SessionEnd,
            "stop" => cocode_hooks::HookEventType::Stop,
            "subagent_start" => cocode_hooks::HookEventType::SubagentStart,
            "subagent_stop" => cocode_hooks::HookEventType::SubagentStop,
            "pre_compact" => cocode_hooks::HookEventType::PreCompact,
            "notification" => cocode_hooks::HookEventType::Notification,
            "permission_request" => cocode_hooks::HookEventType::PermissionRequest,
            other => {
                tracing::warn!(event = %other, "Unknown hook event type in config, skipping");
                continue;
            }
        };

        // Parse matcher: pipe-separated pattern becomes Or matcher
        let matcher = cfg.matcher.as_deref().map(|m| {
            if m.contains('|') {
                let parts: Vec<cocode_hooks::HookMatcher> = m
                    .split('|')
                    .map(|p| cocode_hooks::HookMatcher::Exact {
                        value: p.trim().to_string(),
                    })
                    .collect();
                cocode_hooks::HookMatcher::Or { matchers: parts }
            } else {
                cocode_hooks::HookMatcher::Exact {
                    value: m.to_string(),
                }
            }
        });

        // Each handler becomes a separate HookDefinition
        for (h_idx, handler_cfg) in cfg.hooks.iter().enumerate() {
            let (handler, timeout_secs) = match handler_cfg {
                HookHandlerConfig::Command {
                    command,
                    args,
                    timeout_secs,
                } => (
                    cocode_hooks::HookHandler::Command {
                        command: command.clone(),
                        args: args.clone(),
                    },
                    *timeout_secs,
                ),
            };

            defs.push(cocode_hooks::HookDefinition {
                name: format!("config-hook-{idx}-{h_idx}"),
                event_type: event_type.clone(),
                matcher: matcher.clone(),
                handler,
                source: cocode_hooks::HookSource::Session,
                enabled: true,
                timeout_secs,
                once: false,
            });
        }
    }
    defs
}

#[cfg(test)]
#[path = "state.test.rs"]
mod tests;
