//! Builder for constructing an [`AgentLoop`].

use std::sync::Arc;
use std::sync::Mutex;

use cocode_context::ConversationContext;
use cocode_hooks::AsyncHookTracker;
use cocode_hooks::HookRegistry;
use cocode_inference::ApiClient;
use cocode_inference::ModelHub;
use cocode_message::MessageHistory;
use cocode_policy::ApprovalStore;
use cocode_protocol::AgentStatus;
use cocode_protocol::CompactConfig;
use cocode_protocol::LoopConfig;
use cocode_protocol::LoopEvent;
use cocode_protocol::RoleSelections;
use cocode_shell::ShellExecutor;
use cocode_skill::SkillManager;
use cocode_system_reminder::QueuedCommandInfo;
use cocode_system_reminder::SystemReminderConfig;
use cocode_system_reminder::SystemReminderOrchestrator;
use cocode_tools::FileReadState;
use cocode_tools::FileTracker;
use cocode_tools::PermissionRequester;
use cocode_tools::SpawnAgentFn;
use cocode_tools::ToolRegistry;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::fallback::FallbackConfig;
use crate::fallback::FallbackState;
use crate::session_memory_agent::SessionMemoryExtractionAgent;

use super::AgentLoop;

/// Builder for constructing an [`AgentLoop`].
pub struct AgentLoopBuilder {
    // Required fields (passed via `new()`)
    api_client: ApiClient,
    model_hub: Arc<ModelHub>,
    selections: RoleSelections,
    tool_registry: Arc<ToolRegistry>,
    context: ConversationContext,
    event_tx: mpsc::Sender<LoopEvent>,

    // Optional fields (set via builder methods)
    message_history: Option<MessageHistory>,
    config: LoopConfig,
    fallback_config: FallbackConfig,
    compact_config: CompactConfig,
    system_reminder_config: SystemReminderConfig,
    hooks: Option<Arc<HookRegistry>>,
    cancel_token: CancellationToken,
    extraction_agent: Option<Arc<SessionMemoryExtractionAgent>>,
    is_subagent: bool,
    custom_system_prompt: Option<String>,
    system_prompt_suffix: Option<String>,
    plan_mode_state: Option<cocode_plan_mode::PlanModeState>,
    auto_memory_state: Option<Arc<cocode_auto_memory::AutoMemoryState>>,
    team_store: Option<Arc<cocode_team::TeamStore>>,
    team_mailbox: Option<Arc<cocode_team::Mailbox>>,
    shell_executor: Option<ShellExecutor>,
    sandbox_state: Option<std::sync::Arc<cocode_sandbox::SandboxState>>,
    spawn_agent_fn: Option<SpawnAgentFn>,
    skill_manager: Option<Arc<SkillManager>>,
    queued_commands: Arc<Mutex<Vec<QueuedCommandInfo>>>,
    status_tx: Option<watch::Sender<AgentStatus>>,
    features: cocode_protocol::Features,
    web_search_config: cocode_protocol::WebSearchConfig,
    web_fetch_config: cocode_protocol::WebFetchConfig,
    permission_rules: Vec<cocode_policy::PermissionRule>,
    otel_manager: Option<Arc<cocode_otel::OtelManager>>,
    lsp_manager: Option<Arc<cocode_lsp::LspServerManager>>,
    task_type_restrictions: Option<Vec<String>>,
    snapshot_manager: Option<Arc<cocode_file_backup::SnapshotManager>>,
    question_responder: Option<Arc<cocode_tools::QuestionResponder>>,
    approval_store: Option<Arc<tokio::sync::Mutex<ApprovalStore>>>,
    /// Initial file tracker state for session resumption.
    /// Vector of (path, read_state) pairs to restore into the FileTracker.
    /// Named to clarify this is the reminder-level snapshot, distinct from the
    /// shared tools-level tracker.
    reminder_file_tracker_state: Vec<(std::path::PathBuf, FileReadState)>,
    cocode_home: Option<std::path::PathBuf>,
    /// Shared set of agent IDs killed via TaskStop (persists across turns).
    killed_agents: cocode_tools::context::KilledAgents,
    /// Optional permission requester for interactive approval flow (SDK mode).
    permission_requester: Option<Arc<dyn PermissionRequester>>,
}

impl AgentLoopBuilder {
    /// Create a new builder with the 6 required fields.
    ///
    /// All other fields can be set via builder methods.
    pub fn new(
        api_client: ApiClient,
        model_hub: Arc<ModelHub>,
        selections: RoleSelections,
        tool_registry: Arc<ToolRegistry>,
        context: ConversationContext,
        event_tx: mpsc::Sender<LoopEvent>,
    ) -> Self {
        Self {
            api_client,
            model_hub,
            selections,
            tool_registry,
            context,
            event_tx,
            message_history: None,
            config: LoopConfig::default(),
            fallback_config: FallbackConfig::default(),
            compact_config: CompactConfig::default(),
            system_reminder_config: SystemReminderConfig::default(),
            hooks: None,
            cancel_token: CancellationToken::new(),
            extraction_agent: None,
            is_subagent: false,
            custom_system_prompt: None,
            system_prompt_suffix: None,
            plan_mode_state: None,
            auto_memory_state: None,
            team_store: None,
            team_mailbox: None,
            shell_executor: None,
            sandbox_state: None,
            spawn_agent_fn: None,
            skill_manager: None,
            queued_commands: Arc::new(Mutex::new(Vec::new())),
            status_tx: None,
            features: cocode_protocol::Features::with_defaults(),
            web_search_config: cocode_protocol::WebSearchConfig::default(),
            web_fetch_config: cocode_protocol::WebFetchConfig::default(),
            permission_rules: Vec::new(),
            otel_manager: None,
            lsp_manager: None,
            task_type_restrictions: None,
            snapshot_manager: None,
            question_responder: None,
            approval_store: None,
            reminder_file_tracker_state: Vec::new(),
            cocode_home: None,
            killed_agents: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
            permission_requester: None,
        }
    }

    pub fn message_history(mut self, history: MessageHistory) -> Self {
        self.message_history = Some(history);
        self
    }

    pub fn config(mut self, config: LoopConfig) -> Self {
        self.config = config;
        self
    }

    pub fn fallback_config(mut self, config: FallbackConfig) -> Self {
        self.fallback_config = config;
        self
    }

    /// Set the compact configuration.
    pub fn compact_config(mut self, config: CompactConfig) -> Self {
        self.compact_config = config;
        self
    }

    /// Set the system reminder configuration.
    pub fn system_reminder_config(mut self, config: SystemReminderConfig) -> Self {
        self.system_reminder_config = config;
        self
    }

    pub fn hooks(mut self, hooks: Arc<HookRegistry>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    pub fn cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = token;
        self
    }

    /// Set the background session memory extraction agent.
    pub fn extraction_agent(mut self, agent: Arc<SessionMemoryExtractionAgent>) -> Self {
        self.extraction_agent = Some(agent);
        self
    }

    /// Mark this loop as a subagent (spawned via Task tool).
    ///
    /// Subagents skip MainAgentOnly tier system reminders.
    pub fn is_subagent(mut self, is_subagent: bool) -> Self {
        self.is_subagent = is_subagent;
        self
    }

    /// Set a custom system prompt for this agent.
    ///
    /// When set, this replaces the standard `SystemPromptBuilder::build()` output.
    /// Used by subagents with `use_custom_prompt: true` in their definition.
    pub fn custom_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.custom_system_prompt = Some(prompt.into());
        self
    }

    /// Set a suffix to append to the generated system prompt.
    ///
    /// Used for `critical_reminder` enforcement (CC's `criticalSystemReminder_EXPERIMENTAL`).
    /// Appended at the end of the system prompt for highest authority positioning.
    /// Ignored when `custom_system_prompt` is set (the custom prompt is used as-is).
    pub fn system_prompt_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.system_prompt_suffix = Some(suffix.into());
        self
    }

    /// Set initial plan mode state (for session resumption).
    pub fn plan_mode_state(mut self, state: cocode_plan_mode::PlanModeState) -> Self {
        self.plan_mode_state = Some(state);
        self
    }

    /// Set the auto memory state.
    pub fn auto_memory_state(mut self, state: Arc<cocode_auto_memory::AutoMemoryState>) -> Self {
        self.auto_memory_state = Some(state);
        self
    }

    /// Set the team store for querying team membership.
    pub fn team_store(mut self, store: Arc<cocode_team::TeamStore>) -> Self {
        self.team_store = Some(store);
        self
    }

    /// Set the team mailbox for querying unread messages.
    pub fn team_mailbox(mut self, mailbox: Arc<cocode_team::Mailbox>) -> Self {
        self.team_mailbox = Some(mailbox);
        self
    }

    /// Set the shell executor for command execution and background tasks.
    pub fn shell_executor(mut self, executor: ShellExecutor) -> Self {
        self.shell_executor = Some(executor);
        self
    }

    /// Set the sandbox state from an Option (no-op if None).
    pub fn maybe_sandbox_state(
        mut self,
        state: Option<std::sync::Arc<cocode_sandbox::SandboxState>>,
    ) -> Self {
        self.sandbox_state = state;
        self
    }

    /// Set the spawn agent callback for the Task tool.
    pub fn spawn_agent_fn(mut self, f: SpawnAgentFn) -> Self {
        self.spawn_agent_fn = Some(f);
        self
    }

    /// Set the skill manager for loading and executing skills.
    pub fn skill_manager(mut self, manager: Arc<SkillManager>) -> Self {
        self.skill_manager = Some(manager);
        self
    }

    /// Set the shared queued-commands handle for real-time steering.
    ///
    /// The same `Arc<Mutex<Vec>>` is held by the TUI driver so it can push
    /// new commands while the loop is running. Commands are drained once per
    /// iteration in `core_message_loop` Step 6.5 and injected as steering
    /// system-reminders.
    pub fn queued_commands(mut self, commands: Arc<Mutex<Vec<QueuedCommandInfo>>>) -> Self {
        self.queued_commands = commands;
        self
    }

    /// Set the status watch channel sender.
    ///
    /// This enables efficient status polling without processing all events.
    /// If not set, a new channel will be created internally (the receiver
    /// will be accessible via `AgentLoop::status_receiver()`).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tokio::sync::watch;
    /// use cocode_protocol::AgentStatus;
    ///
    /// let (status_tx, status_rx) = watch::channel(AgentStatus::default());
    /// let loop_builder = AgentLoop::builder(api_client, model_hub, selections, tool_registry, context, event_tx)
    ///     .status_tx(status_tx)
    ///     // ... other config
    ///     .build();
    /// // status_rx can be used to poll status efficiently
    /// ```
    pub fn status_tx(mut self, tx: watch::Sender<AgentStatus>) -> Self {
        self.status_tx = Some(tx);
        self
    }

    /// Set the feature flags.
    pub fn features(mut self, features: cocode_protocol::Features) -> Self {
        self.features = features;
        self
    }

    /// Set the web search configuration.
    pub fn web_search_config(mut self, config: cocode_protocol::WebSearchConfig) -> Self {
        self.web_search_config = config;
        self
    }

    /// Set the web fetch configuration.
    pub fn web_fetch_config(mut self, config: cocode_protocol::WebFetchConfig) -> Self {
        self.web_fetch_config = config;
        self
    }

    /// Set pre-configured permission rules.
    pub fn permission_rules(mut self, rules: Vec<cocode_policy::PermissionRule>) -> Self {
        self.permission_rules = rules;
        self
    }

    /// Set the LSP server manager for language intelligence tools.
    pub fn lsp_manager(mut self, manager: Arc<cocode_lsp::LspServerManager>) -> Self {
        self.lsp_manager = Some(manager);
        self
    }

    /// Set the OTel manager for metrics and traces.
    pub fn otel_manager(mut self, otel: Option<Arc<cocode_otel::OtelManager>>) -> Self {
        self.otel_manager = otel;
        self
    }

    /// Set Task type restrictions for subagent spawning.
    pub fn task_type_restrictions(mut self, restrictions: Option<Vec<String>>) -> Self {
        self.task_type_restrictions = restrictions;
        self
    }

    /// Set the snapshot manager for rewind support.
    pub fn snapshot_manager(mut self, mgr: Arc<cocode_file_backup::SnapshotManager>) -> Self {
        self.snapshot_manager = Some(mgr);
        self
    }

    /// Set the question responder for AskUserQuestion tool.
    pub fn question_responder(mut self, responder: Arc<cocode_tools::QuestionResponder>) -> Self {
        self.question_responder = Some(responder);
        self
    }

    /// Set a shared approval store (persists across turns).
    ///
    /// If not set, a fresh approval store is created per loop instance.
    pub fn approval_store(mut self, store: Arc<tokio::sync::Mutex<ApprovalStore>>) -> Self {
        self.approval_store = Some(store);
        self
    }

    /// Set the permission requester for interactive approval flow (SDK mode).
    pub fn permission_requester(mut self, requester: Arc<dyn PermissionRequester>) -> Self {
        self.permission_requester = Some(requester);
        self
    }

    /// Set the initial file tracker state for session resumption.
    ///
    /// This state is used to initialize the FileTracker with previously tracked
    /// file reads, enabling proper already-read detection across session restarts.
    pub fn reminder_file_tracker_state(
        mut self,
        state: Vec<(std::path::PathBuf, FileReadState)>,
    ) -> Self {
        self.reminder_file_tracker_state = state;
        self
    }

    /// Set the shared killed agents registry (persists across turns).
    pub fn killed_agents(mut self, killed: cocode_tools::context::KilledAgents) -> Self {
        self.killed_agents = killed;
        self
    }

    /// Set the cocode home directory for durable cron persistence.
    pub fn cocode_home(mut self, path: std::path::PathBuf) -> Self {
        self.cocode_home = Some(path);
        self
    }

    /// Build the [`AgentLoop`].
    pub fn build(self) -> AgentLoop {
        let model_name = self
            .config
            .fallback_model
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        // Create system reminder components
        let reminder_orchestrator = SystemReminderOrchestrator::new(self.system_reminder_config);
        // Create shared file tracker for tool execution and change detection (persists across turns).
        //
        // Initialization strategy (Claude Code alignment):
        // 1. Rebuild state from message history via ContextModifier::FileRead
        // 2. Merge with persisted reminder_file_tracker_state (persisted has priority for same paths)
        // 3. This ensures consistency with both history and persisted state at startup
        let shared_tools_file_tracker = {
            let history_state = if let Some(ref mh) = self.message_history {
                cocode_system_reminder::build_file_read_state_from_modifiers(
                    mh.turns().iter().flat_map(|turn| {
                        turn.tool_calls.iter().map(move |tc| {
                            (
                                tc.name.as_str(),
                                tc.modifiers.as_slice(),
                                turn.number,
                                tc.status.is_terminal(),
                            )
                        })
                    }),
                    crate::compaction::LRU_MAX_ENTRIES,
                )
            } else {
                Vec::new()
            };

            let merged = if !self.reminder_file_tracker_state.is_empty() {
                // Merge: persisted state has priority for same paths (newer reads)
                cocode_system_reminder::merge_file_read_state(
                    history_state,
                    self.reminder_file_tracker_state,
                )
            } else {
                history_state
            };

            let tracker = FileTracker::new();
            for (path, state) in merged {
                tracker.record_read_with_state(path, state);
            }
            Arc::new(tokio::sync::Mutex::new(tracker))
        };
        // Use provided approval store or create a fresh one
        let shared_approval_store = self
            .approval_store
            .unwrap_or_else(|| Arc::new(tokio::sync::Mutex::new(ApprovalStore::new())));

        // Create status channel if not provided
        let status_tx = self
            .status_tx
            .unwrap_or_else(|| watch::channel(AgentStatus::default()).0);

        let cwd: std::path::PathBuf = self.context.environment.cwd.clone();
        let shell_executor = self
            .shell_executor
            .unwrap_or_else(|| ShellExecutor::new(cwd));

        // Create the extraction outcome channel (bounded to 4 to avoid unbounded growth)
        let (extraction_result_tx, extraction_result_rx) =
            mpsc::channel::<crate::session_memory_agent::ExtractionOutcome>(4);

        AgentLoop {
            api_client: self.api_client,
            model_hub: self.model_hub,
            selections: self.selections,
            tool_registry: self.tool_registry,
            message_history: self.message_history.unwrap_or_default(),
            context: self.context,
            config: self.config,
            fallback_config: self.fallback_config,
            compact_config: self.compact_config,
            reminder_orchestrator,
            shared_tools_file_tracker,
            shared_approval_store,
            hooks: self.hooks.unwrap_or_else(|| Arc::new(HookRegistry::new())),
            async_hook_tracker: Arc::new(AsyncHookTracker::new()),
            event_tx: self.event_tx,
            turn_number: 0,
            cancel_token: self.cancel_token,
            fallback_state: FallbackState::new(model_name),
            total_input_tokens: 0,
            total_output_tokens: 0,
            extraction_agent: self.extraction_agent,
            extraction_result_rx,
            extraction_result_tx,
            compact_failure_count: 0,
            circuit_breaker_open: false,
            is_subagent: self.is_subagent,
            custom_system_prompt: self.custom_system_prompt,
            system_prompt_suffix: self.system_prompt_suffix,
            // Initially true - the first turn always has user input
            current_turn_has_user_input: true,
            plan_mode_state: self.plan_mode_state.unwrap_or_default(),
            auto_memory_state: self.auto_memory_state,
            team_store: self.team_store,
            team_mailbox: self.team_mailbox,
            shell_executor,
            sandbox_state: self.sandbox_state,
            spawn_agent_fn: self.spawn_agent_fn,
            skill_manager: self.skill_manager,
            invoked_skills_tracker: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            active_skill_allowed_tools: None,
            model_override: None,
            current_todos: None,
            current_structured_tasks: None,
            current_cron_jobs: None,
            delegate_mode: false,
            queued_commands: self.queued_commands.clone(),
            features: self.features,
            web_search_config: self.web_search_config,
            web_fetch_config: self.web_fetch_config,
            permission_rules: self.permission_rules,
            status_tx,
            otel_manager: self.otel_manager,
            lsp_manager: self.lsp_manager,
            task_type_restrictions: self.task_type_restrictions,
            snapshot_manager: self.snapshot_manager,
            question_responder: self
                .question_responder
                .unwrap_or_else(|| Arc::new(cocode_tools::QuestionResponder::new())),
            pending_compacted_large_files: Vec::new(),
            cocode_home: self.cocode_home,
            background_agent_tasks: Vec::new(),
            killed_agents: self.killed_agents,
            permission_requester: self.permission_requester,
        }
    }
}
