//! Agent loop driver - the core 18-step conversation loop.

use std::sync::Arc;
use std::time::Instant;

use cocode_api::ApiClient;
use cocode_api::CollectedResponse;
use cocode_api::ModelHub;
use cocode_api::QueryResultType;
use cocode_api::RequestBuilder;
use cocode_api::StreamOptions;
use cocode_context::ConversationContext;
use cocode_hooks::AsyncHookTracker;
use cocode_hooks::HookRegistry;
use cocode_message::MessageHistory;
use cocode_message::TrackedMessage;
use cocode_message::Turn;
use cocode_prompt::SystemPromptBuilder;
use cocode_protocol::AgentStatus;
use cocode_protocol::AutoCompactTracking;
use cocode_protocol::CompactConfig;
use cocode_protocol::ContextModifier;
use cocode_protocol::HookEventType;
use cocode_protocol::LoopConfig;
use cocode_protocol::LoopEvent;
use cocode_protocol::QueryTracking;
use cocode_protocol::RoleSelections;
use cocode_protocol::TokenUsage;
use cocode_protocol::ToolResultContent;
use cocode_skill::SkillManager;
use cocode_system_reminder::ApprovedPlanInfo;
use cocode_system_reminder::AsyncHookResponseInfo;
use cocode_system_reminder::GeneratorContext;
use cocode_system_reminder::HookBlockingInfo;
use cocode_system_reminder::HookContextInfo;
use cocode_system_reminder::HookState;
use cocode_system_reminder::InjectedBlock;
use cocode_system_reminder::InjectedMessage;
use cocode_system_reminder::InvokedSkillInfo;
use cocode_system_reminder::MentionReadRecord;
use cocode_system_reminder::QueuedCommandInfo;
use cocode_system_reminder::SkillInfo;
use cocode_system_reminder::SystemReminderConfig;
use cocode_system_reminder::SystemReminderOrchestrator;
use cocode_system_reminder::create_injected_messages;
use cocode_tools::ApprovalStore;
use cocode_tools::ExecutorConfig;
use cocode_tools::FileReadState;
use cocode_tools::FileTracker;
use cocode_tools::ModelCallFn;
use cocode_tools::ModelCallInput;
use cocode_tools::ModelCallResult;
use cocode_tools::SpawnAgentFn;
use cocode_tools::StreamingToolExecutor;
use cocode_tools::ToolExecutionResult;
use cocode_tools::ToolRegistry;
use hyper_sdk::ContentBlock;
use hyper_sdk::FinishReason;
use hyper_sdk::Message;
use hyper_sdk::ToolCall;
use hyper_sdk::ToolDefinition;
use std::sync::Mutex;

use snafu::ResultExt;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::compaction::CompactionConfig;
use crate::compaction::FileRestoration;
use crate::compaction::FileRestorationConfig;
use crate::compaction::InvokedSkillRestoration;
use crate::compaction::LRU_MAX_ENTRIES;
use crate::compaction::SessionMemorySummary;
use crate::compaction::TaskStatusRestoration;
use crate::compaction::ThresholdStatus;
use crate::compaction::build_compact_instructions;
use crate::compaction::build_context_restoration_with_config;
use crate::compaction::build_file_read_state;
use crate::compaction::calculate_keep_start_index;
use crate::compaction::format_restoration_message;
use crate::compaction::is_internal_file;
use crate::compaction::map_message_index_to_keep_turns;
use crate::compaction::try_session_memory_compact;
use crate::compaction::write_session_memory;
use crate::error::agent_loop_error;
use crate::fallback::FallbackConfig;
use crate::fallback::FallbackState;
use crate::result::LoopResult;
use crate::session_memory_agent::SessionMemoryExtractionAgent;
use cocode_plan_mode::PlanModeState;
use cocode_shell::ShellExecutor;

/// Maximum number of retry attempts for output-token exhaustion recovery.
const MAX_OUTPUT_TOKEN_RECOVERY: i32 = 3;

/// The main agent loop that drives multi-turn conversations with LLM providers.
///
/// `AgentLoop` manages streaming API calls, concurrent tool execution,
/// context compaction, model fallback, and event emission.
pub struct AgentLoop {
    // Provider / model
    api_client: ApiClient,
    /// Model hub for unified model resolution.
    ///
    /// Provides model acquisition and caching. Note: ModelHub is role-agnostic;
    /// role resolution uses `selections` which are passed to ModelHub methods.
    model_hub: Arc<ModelHub>,
    /// Role selections for this agent loop.
    ///
    /// Owned by the loop (cloned from Session at creation time). This enables
    /// proper isolation: subagents get their own copy and are unaffected by
    /// changes to the parent's model settings.
    selections: RoleSelections,

    // Tool system
    tool_registry: Arc<ToolRegistry>,

    // Conversation state
    message_history: MessageHistory,
    context: ConversationContext,

    // Config
    config: LoopConfig,
    fallback_config: FallbackConfig,
    compaction_config: CompactionConfig,
    /// Protocol-level compact configuration with all threshold constants.
    compact_config: CompactConfig,

    // System reminders
    reminder_orchestrator: SystemReminderOrchestrator,
    /// Shared FileTracker for tool execution and change detection (persists across turns).
    /// Named to clarify this is the shared tools-level tracker, distinct from the
    /// reminder-level file tracker state snapshot.
    shared_tools_file_tracker: Arc<tokio::sync::Mutex<FileTracker>>,
    /// Shared ApprovalStore for tool execution (persists across turns).
    shared_approval_store: Arc<tokio::sync::Mutex<ApprovalStore>>,

    // Hooks
    hooks: Arc<HookRegistry>,
    /// Shared async hook tracker (persists across turns for background hooks).
    async_hook_tracker: Arc<AsyncHookTracker>,

    // Event channel
    event_tx: mpsc::Sender<LoopEvent>,

    // State tracking
    turn_number: i32,
    cancel_token: CancellationToken,
    fallback_state: FallbackState,
    total_input_tokens: i32,
    total_output_tokens: i32,

    // Background extraction agent (optional)
    extraction_agent: Option<Arc<SessionMemoryExtractionAgent>>,

    // Agent type tracking (for tier filtering in system reminders)
    /// Whether this is a subagent (spawned by Task tool).
    /// When true, MainAgentOnly tier reminders are skipped.
    is_subagent: bool,
    /// Optional custom system prompt that replaces the default `SystemPromptBuilder::build()`.
    ///
    /// When set (and `is_subagent` is true), the agent uses this as its full system prompt
    /// instead of the standard multi-section generated prompt. This allows agent definitions
    /// to provide focused, minimal prompts without inheriting the parent's full instructions.
    custom_system_prompt: Option<String>,
    /// Whether the current turn has user input.
    /// When false, UserPrompt tier reminders are skipped.
    current_turn_has_user_input: bool,

    // Plan mode tracking
    /// Plan mode state for the session.
    plan_mode_state: PlanModeState,

    // Subagent spawning
    /// Shell executor for command execution and background tasks.
    shell_executor: ShellExecutor,

    /// Optional callback for spawning subagents (used by Task tool).
    spawn_agent_fn: Option<SpawnAgentFn>,

    // Skill system
    /// Optional skill manager for loading and executing skills.
    skill_manager: Option<Arc<SkillManager>>,
    /// Shared tracker for skills invoked via the Skill tool during execution.
    /// Persists across turns so invoked skills can be injected into system reminders.
    invoked_skills_tracker: Arc<tokio::sync::Mutex<Vec<cocode_tools::InvokedSkill>>>,
    /// Active skill-level tool restrictions.
    /// Set when a skill with `allowed_tools` is invoked via the Skill tool.
    /// Applied to the executor on the next turn iteration.
    active_skill_allowed_tools: Option<std::collections::HashSet<String>>,

    // Task list state (updated by TodoWrite tool via ContextModifier)
    /// Latest task list from the most recent TodoWrite tool call.
    current_todos: Option<serde_json::Value>,

    // Real-time steering
    /// Queued commands from user (Enter during streaming).
    /// Shared via `Arc<Mutex>` so the TUI driver can push commands while the
    /// loop is running. Drained once per iteration in Step 6.5 and injected
    /// as steering system-reminders.
    queued_commands: Arc<Mutex<Vec<QueuedCommandInfo>>>,

    // Feature flags
    /// Feature flags for tool enablement and feature gating.
    features: cocode_protocol::Features,

    // Web search config
    /// Web search configuration (provider, api_key, max_results).
    web_search_config: cocode_protocol::WebSearchConfig,

    // Web fetch config
    /// Web fetch configuration (timeout, max_content_length, user_agent).
    web_fetch_config: cocode_protocol::WebFetchConfig,

    // Permission rules
    /// Pre-configured permission rules loaded from settings files.
    permission_rules: Vec<cocode_tools::PermissionRule>,

    // Status broadcast
    /// Watch channel sender for broadcasting agent status.
    /// This allows efficient status polling without processing all events.
    status_tx: watch::Sender<AgentStatus>,

    // OpenTelemetry
    /// Optional OTel manager for metrics and traces.
    otel_manager: Option<Arc<cocode_otel::OtelManager>>,

    // LSP
    /// Optional LSP server manager for language intelligence tools.
    lsp_manager: Option<Arc<cocode_lsp::LspServerManager>>,

    // Task type restrictions
    /// Allowed subagent types when `Task(type1, type2)` is in the agent's tools.
    task_type_restrictions: Option<Vec<String>>,

    // Rewind / snapshot system
    /// Optional snapshot manager for file backups and ghost commits.
    /// Provides two-tier rewind: Tier 1 (file backup) for all workspaces,
    /// Tier 2 (ghost commit) for git repos.
    snapshot_manager: Option<Arc<cocode_file_backup::SnapshotManager>>,

    /// Question responder for AskUserQuestion tool.
    ///
    /// Shared across turns so the TUI driver can send responses that
    /// unblock the AskUserQuestion tool's oneshot channel.
    question_responder: Arc<cocode_tools::QuestionResponder>,

    /// Large files that were compacted but not restored inline.
    ///
    /// Populated during context restoration when a file exceeds the per-file
    /// token limit. Consumed once on the next turn by the
    /// `CompactFileReferenceGenerator` to notify the model.
    pending_compacted_large_files: Vec<cocode_protocol::CompactedLargeFileRef>,
}

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
    compaction_config: CompactionConfig,
    compact_config: CompactConfig,
    system_reminder_config: SystemReminderConfig,
    hooks: Option<Arc<HookRegistry>>,
    cancel_token: CancellationToken,
    extraction_agent: Option<Arc<SessionMemoryExtractionAgent>>,
    is_subagent: bool,
    custom_system_prompt: Option<String>,
    plan_mode_state: Option<PlanModeState>,
    shell_executor: Option<ShellExecutor>,
    spawn_agent_fn: Option<SpawnAgentFn>,
    skill_manager: Option<Arc<SkillManager>>,
    queued_commands: Arc<Mutex<Vec<QueuedCommandInfo>>>,
    status_tx: Option<watch::Sender<AgentStatus>>,
    features: cocode_protocol::Features,
    web_search_config: cocode_protocol::WebSearchConfig,
    web_fetch_config: cocode_protocol::WebFetchConfig,
    permission_rules: Vec<cocode_tools::PermissionRule>,
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
            compaction_config: CompactionConfig::default(),
            compact_config: CompactConfig::default(),
            system_reminder_config: SystemReminderConfig::default(),
            hooks: None,
            cancel_token: CancellationToken::new(),
            extraction_agent: None,
            is_subagent: false,
            custom_system_prompt: None,
            plan_mode_state: None,
            shell_executor: None,
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

    pub fn compaction_config(mut self, config: CompactionConfig) -> Self {
        self.compaction_config = config;
        self
    }

    /// Set the protocol-level compact configuration.
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

    /// Set initial plan mode state (for session resumption).
    pub fn plan_mode_state(mut self, state: PlanModeState) -> Self {
        self.plan_mode_state = Some(state);
        self
    }

    /// Set the shell executor for command execution and background tasks.
    pub fn shell_executor(mut self, executor: ShellExecutor) -> Self {
        self.shell_executor = Some(executor);
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
    pub fn permission_rules(mut self, rules: Vec<cocode_tools::PermissionRule>) -> Self {
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
                let tool_calls: Vec<(&str, &[ContextModifier], i32, bool)> = mh
                    .turns()
                    .iter()
                    .flat_map(|turn| {
                        turn.tool_calls.iter().map(move |tc| {
                            (
                                tc.name.as_str(),
                                tc.modifiers.as_slice(),
                                turn.number,
                                tc.status.is_terminal(),
                            )
                        })
                    })
                    .collect();
                cocode_system_reminder::build_file_read_state_from_modifiers(
                    tool_calls.into_iter(),
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

        AgentLoop {
            api_client: self.api_client,
            model_hub: self.model_hub,
            selections: self.selections,
            tool_registry: self.tool_registry,
            message_history: self.message_history.unwrap_or_default(),
            context: self.context,
            config: self.config,
            fallback_config: self.fallback_config,
            compaction_config: self.compaction_config,
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
            is_subagent: self.is_subagent,
            custom_system_prompt: self.custom_system_prompt,
            // Initially true - the first turn always has user input
            current_turn_has_user_input: true,
            plan_mode_state: self.plan_mode_state.unwrap_or_default(),
            shell_executor,
            spawn_agent_fn: self.spawn_agent_fn,
            skill_manager: self.skill_manager,
            invoked_skills_tracker: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            active_skill_allowed_tools: None,
            current_todos: None,
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
        }
    }
}

impl AgentLoop {
    /// Create a builder for constructing an agent loop.
    pub fn builder(
        api_client: ApiClient,
        model_hub: Arc<ModelHub>,
        selections: RoleSelections,
        tool_registry: Arc<ToolRegistry>,
        context: ConversationContext,
        event_tx: mpsc::Sender<LoopEvent>,
    ) -> AgentLoopBuilder {
        AgentLoopBuilder::new(
            api_client,
            model_hub,
            selections,
            tool_registry,
            context,
            event_tx,
        )
    }

    /// Queue a command for real-time steering.
    ///
    /// Queued commands are consumed once in `core_message_loop` Step 6.5 and
    /// injected as steering system-reminders. The steering prompt asks the model
    /// to address the message and continue, so no separate post-idle execution
    /// is needed (consume-then-remove pattern).
    #[allow(clippy::unwrap_used)]
    pub fn queue_command(&self, prompt: impl Into<String>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let cmd = QueuedCommandInfo {
            id: uuid::Uuid::new_v4().to_string(),
            prompt: prompt.into(),
            queued_at: now,
        };
        self.queued_commands.lock().unwrap().push(cmd);
    }

    /// Drain all queued commands.
    #[allow(clippy::unwrap_used)]
    pub fn take_queued_commands(&self) -> Vec<QueuedCommandInfo> {
        std::mem::take(&mut *self.queued_commands.lock().unwrap())
    }

    /// Get the number of queued commands.
    #[allow(clippy::unwrap_used)]
    pub fn queued_count(&self) -> usize {
        self.queued_commands.lock().unwrap().len()
    }

    /// Get a shared handle to the queued commands.
    ///
    /// This allows the TUI driver to push commands while the loop is running.
    pub fn shared_queued_commands(&self) -> Arc<Mutex<Vec<QueuedCommandInfo>>> {
        self.queued_commands.clone()
    }

    /// Take the current task list (if any) set by a TodoWrite tool call.
    pub fn take_todos(&mut self) -> Option<serde_json::Value> {
        self.current_todos.take()
    }

    /// Take the plan mode state for persistence across loop runs.
    ///
    /// Called by `SessionState` after the loop finishes to preserve plan mode
    /// state (is_active, has_exited, needs_exit_attachment, etc.) for the next turn.
    pub fn take_plan_mode_state(&mut self) -> Option<PlanModeState> {
        Some(std::mem::take(&mut self.plan_mode_state))
    }

    /// Get the current file tracker state for persistence.
    ///
    /// Returns a read-only snapshot of all tracked files and their read state.
    /// Called by `SessionState` after the loop finishes to preserve file tracker
    /// state for already-read detection across turns.
    pub async fn reminder_file_tracker_snapshot(&self) -> Vec<(std::path::PathBuf, FileReadState)> {
        let tracker = self.shared_tools_file_tracker.lock().await;
        tracker.read_files_snapshot()
    }

    /// Subscribe to status updates.
    ///
    /// Returns a watch receiver that can be used to efficiently poll
    /// the current agent status without processing all events.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut status_rx = agent_loop.subscribe_status();
    /// loop {
    ///     let status = status_rx.borrow().clone();
    ///     if status.is_busy() {
    ///         println!("Agent is busy: {status}");
    ///     }
    ///     status_rx.changed().await.ok();
    /// }
    /// ```
    pub fn subscribe_status(&self) -> watch::Receiver<AgentStatus> {
        self.status_tx.subscribe()
    }

    /// Get the current agent status.
    pub fn current_status(&self) -> AgentStatus {
        self.status_tx.borrow().clone()
    }

    /// Update the agent status.
    ///
    /// This is called internally at key state transitions.
    fn set_status(&self, status: AgentStatus) {
        // Ignore send errors - if all receivers are dropped, that's fine
        let _ = self.status_tx.send(status);
    }

    /// Run the agent loop to completion, starting with an initial user message.
    ///
    /// Returns a `LoopResult` describing how the loop terminated along with
    /// aggregate token usage and the final response text.
    pub async fn run(&mut self, initial_message: &str) -> crate::error::Result<LoopResult> {
        info!(
            max_turns = ?self.config.max_turns,
            "Starting agent loop"
        );

        let session_id = uuid::Uuid::new_v4().to_string();

        // Execute SessionStart hooks at real session start (turn_number == 0 means first run)
        if self.turn_number == 0 {
            let model_name = self
                .config
                .fallback_model
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            let ctx = cocode_hooks::HookContext::new(
                cocode_hooks::HookEventType::SessionStart,
                session_id.clone(),
                self.context.environment.cwd.clone(),
            )
            .with_source("startup")
            .with_model(model_name);
            self.execute_lifecycle_hooks(ctx).await;
        }

        // Execute UserPromptSubmit hooks before processing the prompt
        {
            let ctx = cocode_hooks::HookContext::new(
                cocode_hooks::HookEventType::UserPromptSubmit,
                session_id.clone(),
                self.context.environment.cwd.clone(),
            )
            .with_prompt(initial_message);
            self.execute_lifecycle_hooks(ctx).await;
        }

        // Record user prompt in OTel
        if let Some(otel) = &self.otel_manager {
            otel.user_prompt(initial_message, initial_message.len());
        }

        // Add user message to history
        let turn_id = uuid::Uuid::new_v4().to_string();
        let user_msg = TrackedMessage::user(initial_message, &turn_id);
        let turn = Turn::new(1, user_msg);
        self.message_history.add_turn(turn);

        // Mark that this turn has user input (new conversation start)
        self.current_turn_has_user_input = true;

        // Initialize tracking
        let mut query_tracking = QueryTracking::new_root(uuid::Uuid::new_v4().to_string());
        let mut auto_compact_tracking = AutoCompactTracking::new();

        let result = self
            .core_message_loop(&mut query_tracking, &mut auto_compact_tracking)
            .await;

        // Execute Stop hooks when the agent loop finishes.
        // If a Stop hook rejects, re-enter the loop once (guard prevents infinite loops).
        let result = {
            let mut ctx = cocode_hooks::HookContext::new(
                cocode_hooks::HookEventType::Stop,
                session_id.clone(),
                self.context.environment.cwd.clone(),
            )
            .with_stop_hook_active(false);
            // Pass last assistant message if available
            if let Ok(ref loop_result) = result {
                if !loop_result.final_text.is_empty() {
                    ctx = ctx.with_last_assistant_message(&loop_result.final_text);
                }
            }
            let stop_rejected = self.execute_lifecycle_hooks(ctx).await;

            if stop_rejected {
                // Hook forced continuation — add a user-side system-reminder and re-enter.
                let turn_id = uuid::Uuid::new_v4().to_string();
                let steering = TrackedMessage::system_reminder(
                    "A Stop hook blocked the stop and requested continuation. \
                     Please continue with your tasks.",
                    "stop_hook_continuation",
                    &turn_id,
                );
                let turn = Turn::new(1, steering);
                self.message_history.add_turn(turn);

                // Re-enter with stop_hook_active=true to prevent infinite loops
                let re_result = self
                    .core_message_loop(&mut query_tracking, &mut auto_compact_tracking)
                    .await;

                // Fire Stop hooks again with stop_hook_active=true (rejection is ignored)
                let mut ctx2 = cocode_hooks::HookContext::new(
                    cocode_hooks::HookEventType::Stop,
                    session_id.clone(),
                    self.context.environment.cwd.clone(),
                )
                .with_stop_hook_active(true);
                if let Ok(ref lr) = re_result {
                    if !lr.final_text.is_empty() {
                        ctx2 = ctx2.with_last_assistant_message(&lr.final_text);
                    }
                }
                self.execute_lifecycle_hooks(ctx2).await;

                re_result
            } else {
                result
            }
        };

        // Execute SessionEnd hooks to signal session lifecycle completion
        {
            let reason = match &result {
                Ok(r) => match r.stop_reason {
                    crate::result::StopReason::MaxTurnsReached => "max_turns",
                    crate::result::StopReason::ModelStopSignal => "end_turn",
                    crate::result::StopReason::UserInterrupted => "user_interrupted",
                    crate::result::StopReason::Error { .. } => "error",
                    crate::result::StopReason::PlanModeExit { .. } => "plan_mode_exit",
                    crate::result::StopReason::HookStopped => "hook_stopped",
                },
                Err(_) => "error",
            };
            let ctx = cocode_hooks::HookContext::new(
                cocode_hooks::HookEventType::SessionEnd,
                session_id,
                self.context.environment.cwd.clone(),
            )
            .with_reason(reason);
            self.execute_lifecycle_hooks(ctx).await;
        }

        result
    }

    /// Run the agent loop, consuming any queued commands as steering.
    ///
    /// Queued commands are consumed in `core_message_loop` Step 6.5 via
    /// `std::mem::take` and injected as steering system-reminders. The steering
    /// prompt explicitly asks the model to "address this message and continue
    /// with your tasks", so no post-idle re-execution is needed.
    pub async fn run_and_process_queue(
        &mut self,
        initial_message: &str,
    ) -> crate::error::Result<LoopResult> {
        self.run(initial_message).await
    }

    /// The 18-step core message loop.
    ///
    /// This implements the algorithm from `docs/arch/core-loop.md`:
    ///
    /// SETUP (1-6): emit events, query tracking, normalize, micro-compact,
    ///   auto-compact, init state.
    /// EXECUTION (7-10): resolve model, check token limit, stream with tools
    ///   + retry, record telemetry.
    /// POST-PROCESSING (11-18): check tool calls, execute queue, abort handling,
    ///   hooks, tracking, queued commands, max turns, recurse.
    async fn core_message_loop(
        &mut self,
        query_tracking: &mut QueryTracking,
        auto_compact_tracking: &mut AutoCompactTracking,
    ) -> crate::error::Result<LoopResult> {
        // ── STEP 1: Signal stream_request_start ──
        self.emit(LoopEvent::StreamRequestStart).await;

        // ── STEP 2: Setup query tracking ──
        query_tracking.depth += 1;
        let turn_id = uuid::Uuid::new_v4().to_string();

        // ── STEP 3: Normalize messages ──
        // Messages are already normalized through MessageHistory::messages_for_api().

        // ── STEP 4: Micro-compaction (PRE-API) ──
        if self.config.enable_micro_compaction {
            let (removed, tokens_saved) = self.micro_compact().await;
            if removed > 0 {
                self.emit(LoopEvent::MicroCompactionApplied {
                    removed_results: removed,
                    tokens_saved,
                })
                .await;
            }
        }

        // ── STEP 5: Auto-compaction check ──
        // Use ThresholdStatus for accurate threshold calculations
        let estimated_tokens = self.message_history.estimate_tokens();
        let context_window = self.context.environment.context_window;

        // Apply safety margin to token estimate
        let estimated_with_margin = self
            .compact_config
            .estimate_tokens_with_margin(estimated_tokens);

        let status =
            ThresholdStatus::calculate(estimated_with_margin, context_window, &self.compact_config);

        debug!(
            estimated_tokens,
            estimated_with_margin,
            context_window,
            percent_left = %format!("{:.1}%", status.percent_left * 100.0),
            status = status.status_description(),
            "Context usage check"
        );

        // Emit warning event if above warning but below auto-compact
        if status.is_above_warning_threshold && !status.is_above_auto_compact_threshold {
            let target = self.compact_config.auto_compact_target(context_window);
            let warning_threshold = self.compact_config.warning_threshold(target);
            self.emit(LoopEvent::ContextUsageWarning {
                estimated_tokens: estimated_with_margin,
                warning_threshold,
                percent_left: status.percent_left,
            })
            .await;

            self.fire_notification_hook(
                "context_warning",
                "Context window warning",
                &format!(
                    "Context usage at {:.0}% ({:.0}% remaining)",
                    (1.0 - status.percent_left) * 100.0,
                    status.percent_left * 100.0,
                ),
            )
            .await;
        }

        // Trigger auto-compact if above threshold (and auto-compact is enabled)
        if status.is_above_auto_compact_threshold && self.compact_config.is_auto_compact_enabled() {
            // Tier 1: Try session memory first (zero API cost)
            // Only if session memory compact is enabled
            if self.compaction_config.session_memory.enable_sm_compact {
                if let Some(summary) =
                    try_session_memory_compact(&self.compaction_config.session_memory)
                {
                    self.apply_session_memory_summary(summary, &turn_id, auto_compact_tracking)
                        .await?;
                } else {
                    // Tier 2: Fall back to LLM-based compaction
                    self.compact(auto_compact_tracking, &turn_id, query_tracking)
                        .await?;
                }
            } else {
                // Session memory compact disabled, go directly to Tier 2
                debug!("Session memory compact disabled, using LLM-based compaction");
                self.compact(auto_compact_tracking, &turn_id, query_tracking)
                    .await?;
            }
        }

        // ── STEP 6: Initialize state ──
        self.turn_number += 1;
        let turn_start = Instant::now();
        // Update status to streaming
        self.set_status(AgentStatus::streaming(turn_id.clone()));
        self.emit(LoopEvent::TurnStarted {
            turn_id: turn_id.clone(),
            turn_number: self.turn_number,
        })
        .await;
        if let Some(otel) = &self.otel_manager {
            otel.counter("cocode.turn.started", 1, &[]);
        }

        // ── STEP 6.1: Start turn snapshot (rewind support) ──
        // Start a new snapshot for this turn: sets the current turn on the backup
        // store (Tier 1) and creates a ghost commit in the background (Tier 2, git only).
        let create_ghost = self.features.enabled(cocode_protocol::Feature::GhostCommit);
        let turn_ghost_commit = if let Some(ref sm) = self.snapshot_manager {
            sm.start_turn_snapshot(&turn_id, self.turn_number, create_ghost)
                .await
        } else {
            None
        };

        // ── STEP 6.5: Generate system reminders ──
        // System reminders provide dynamic context (file changes, plan mode, etc.)
        // that is visible to the model but hidden from the user.
        // The unified FileTracker is shared between tools and system-reminder generators.

        // Collect completed async hooks from previous turns
        let completed_hooks = self.async_hook_tracker.take_completed();
        let async_responses: Vec<AsyncHookResponseInfo> = completed_hooks
            .iter()
            .map(|h| AsyncHookResponseInfo {
                hook_name: h.hook_name.clone(),
                additional_context: h.additional_context.clone(),
                was_blocking: h.was_blocking,
                blocking_reason: h.blocking_reason.clone(),
                duration_ms: h.duration_ms,
            })
            .collect();

        // Separate blocking and context hooks for their dedicated generators
        let blocking_hooks: Vec<HookBlockingInfo> = completed_hooks
            .iter()
            .filter(|h| h.was_blocking)
            .map(|h| HookBlockingInfo {
                hook_name: h.hook_name.clone(),
                event_type: "async".to_string(),
                tool_name: None,
                reason: h
                    .blocking_reason
                    .clone()
                    .unwrap_or_else(|| "Hook blocked execution".to_string()),
            })
            .collect();

        let context_hooks: Vec<HookContextInfo> = completed_hooks
            .into_iter()
            .filter(|h| h.additional_context.is_some() && !h.was_blocking)
            .map(|h| HookContextInfo {
                hook_name: h.hook_name,
                event_type: "async".to_string(),
                tool_name: None,
                additional_context: h.additional_context.unwrap_or_default(),
            })
            .collect();

        let reminder_config = self.reminder_orchestrator.config().clone();

        // Extract user prompt text for @mention parsing
        let user_prompt_text: Option<String> = self
            .message_history
            .current_turn()
            .map(|turn| turn.user_message.text().to_string());

        // Capture data needed for GeneratorContext before locking
        let is_main_agent = !self.is_subagent;
        let has_user_input = self.current_turn_has_user_input;
        let context_window = self.context.environment.context_window;
        let cwd = self.context.environment.cwd.clone();
        let is_plan_mode = self.plan_mode_state.is_active;
        let is_plan_reentry = self.plan_mode_state.is_reentry();
        let is_plan_interview_phase = self.plan_mode_state.is_active
            && self
                .features
                .enabled(cocode_protocol::Feature::PlanModeInterview);
        let plan_file_path = self.plan_mode_state.plan_file_path.clone();
        let needs_plan_reference = self.plan_mode_state.needs_plan_reference;
        let needs_exit_attachment = self.plan_mode_state.needs_exit_attachment;
        let exited_at_turn = self.plan_mode_state.exited_at_turn;
        let turn_number = self.turn_number;

        // Handle rewind info BEFORE acquiring file_tracker lock
        // This restores FileTracker state to match the target turn
        // Get rewind info from snapshot manager
        let rewind_info_value = if let Some(ref sm) = self.snapshot_manager {
            sm.take_rewind_info().await
        } else {
            None
        };

        // Extract the turn number and context info (copy values to avoid borrow issues)
        let rewind_turn_number = rewind_info_value
            .as_ref()
            .map(|info| info.rewound_turn_number);
        let rewind_context_for_builder =
            rewind_info_value
                .as_ref()
                .map(|info| cocode_system_reminder::RewindContextInfo {
                    rewound_turn_number: info.rewound_turn_number,
                    restored_file_count: info.restored_file_count,
                    used_git_restore: info.restored_commit_id.is_some(),
                    rewind_mode: info.mode,
                });

        // Drop the original value to release the borrow
        drop(rewind_info_value);

        // Now restore FileTracker state (requires mutable borrow of self)
        if let Some(to_turn) = rewind_turn_number {
            self.restore_file_tracker_for_rewind(to_turn).await;
        }

        // Build per-turn derived tracker view (snapshot + release lock immediately)
        // This avoids holding the tools tracker lock during the entire generation phase.
        let reminder_tracker_view = self.build_reminder_tracker_view().await;

        // Create shared mention_read_records buffer for generators to push into
        let mention_read_records =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::<MentionReadRecord>::new()));

        // Generate system reminders with derived tracker view (lock NOT held)
        let injected_messages = {
            let mut builder = GeneratorContext::builder()
                .config(&reminder_config)
                .turn_number(turn_number)
                .is_main_agent(is_main_agent)
                .has_user_input(has_user_input)
                .context_window(context_window)
                .cwd(cwd.clone())
                .file_tracker(&reminder_tracker_view)
                .is_plan_mode(is_plan_mode)
                .is_plan_reentry(is_plan_reentry)
                .is_plan_interview_phase(is_plan_interview_phase)
                .mention_read_records(mention_read_records.clone())
                .hook_state(HookState {
                    async_responses,
                    contexts: context_hooks,
                    blocking: blocking_hooks,
                });

            // Drain pending compacted large files (one-shot: populated during restoration)
            if !self.pending_compacted_large_files.is_empty() {
                let drained = std::mem::take(&mut self.pending_compacted_large_files);
                let large_files: Vec<cocode_system_reminder::CompactedLargeFile> = drained
                    .into_iter()
                    .map(|r| cocode_system_reminder::CompactedLargeFile {
                        path: r.path,
                        line_count: r.original_tokens as usize, // approximate
                        byte_size: r.original_size as usize,
                    })
                    .collect();
                builder = builder.compacted_large_files(large_files);
            }

            // Pass user prompt for @mention file injection
            if let Some(ref prompt) = user_prompt_text {
                builder = builder.user_prompt(prompt.as_str());
            }

            // Add plan file path if available
            if let Some(ref path) = plan_file_path {
                builder = builder.plan_file_path(path.clone());
            }

            // Inject plan file reference after compaction (one-shot)
            if needs_plan_reference {
                if let Some(ref plan_path) = plan_file_path {
                    if let Ok(content) = std::fs::read_to_string(plan_path) {
                        builder = builder.restored_plan(cocode_system_reminder::RestoredPlanInfo {
                            content,
                            file_path: plan_path.clone(),
                        });
                    }
                }
            }

            // Inject approved plan content after ExitPlanMode (one-shot)
            if needs_exit_attachment {
                builder = builder.plan_mode_exit_pending(true);
                if let Some(ref plan_path) = plan_file_path {
                    if let Ok(content) = std::fs::read_to_string(plan_path) {
                        builder = builder.approved_plan(ApprovedPlanInfo {
                            content,
                            approved_turn: exited_at_turn.unwrap_or(turn_number),
                        });
                    }
                }
            }

            // Add available skills to generator context
            if let Some(ref sm) = self.skill_manager {
                let skill_infos: Vec<SkillInfo> = sm
                    .llm_invocable_skills()
                    .into_iter()
                    .map(|skill| SkillInfo {
                        name: skill.name.clone(),
                        description: skill.description.clone(),
                        when_to_use: skill.when_to_use.clone(),
                    })
                    .collect();
                if !skill_infos.is_empty() {
                    builder = builder.available_skills(skill_infos);
                }
            }

            // Add invoked skills to generator context
            {
                let invoked = self.invoked_skills_tracker.lock().await;
                if !invoked.is_empty() {
                    let skill_infos: Vec<InvokedSkillInfo> = invoked
                        .iter()
                        .map(|skill| InvokedSkillInfo {
                            name: skill.name.clone(),
                            prompt_content: String::new(),
                        })
                        .collect();
                    builder = builder.invoked_skills(skill_infos);
                }
            }

            // Consume queued commands for steering injection
            {
                #[allow(clippy::unwrap_used)]
                let drained = std::mem::take(&mut *self.queued_commands.lock().unwrap());
                if !drained.is_empty() {
                    builder = builder.queued_commands(drained);
                }
            }

            // Get rewind info if available (already extracted earlier)
            if let Some(rewind_info) = rewind_context_for_builder.clone() {
                builder = builder.rewind_info(rewind_info);
            }

            let gen_ctx = builder.build();
            let reminders = self.reminder_orchestrator.generate_all(gen_ctx).await;

            // Emit SystemReminderDisplay for silent reminders (UI notification only)
            for reminder in &reminders {
                if reminder.is_silent {
                    if let Some(ref metadata) = reminder.metadata {
                        self.emit(LoopEvent::SystemReminderDisplay {
                            reminder_type: reminder.attachment_type.name().to_string(),
                            payload: serde_json::to_value(metadata).unwrap_or_default(),
                        })
                        .await;
                    }
                }
            }

            create_injected_messages(reminders)
        };

        // Drain mention_read_records and apply to shared FileTracker
        // This bridges @mention reads back to the canonical tracker
        {
            #[allow(clippy::unwrap_used)]
            let records: Vec<MentionReadRecord> =
                std::mem::take(&mut *mention_read_records.lock().unwrap());
            self.apply_mention_read_records(&records).await;
        }

        // Consume one-shot flags after generating reminders
        if needs_plan_reference {
            self.plan_mode_state.needs_plan_reference = false;
        }
        if needs_exit_attachment {
            self.plan_mode_state.clear_exit_attachment();
        }

        // ── STEP 7: Resolve model (permissions checked externally) ──
        // In this implementation, model selection is handled by ApiClient.

        // ── STEP 8: Check blocking token limit ──
        // Use CompactConfig for blocking limit calculation
        let blocking_limit = self.compact_config.blocking_limit(context_window);
        if status.is_at_blocking_limit {
            warn!(
                estimated_tokens = estimated_with_margin,
                blocking_limit, "Context window exceeded blocking limit"
            );
            self.set_status(AgentStatus::error("Context window exceeded"));
            return Ok(LoopResult::error(
                self.turn_number,
                self.total_input_tokens,
                self.total_output_tokens,
                format!(
                    "Context window exceeded: {estimated_with_margin} tokens >= {blocking_limit} limit"
                ),
            ));
        }

        // Create executor for this turn BEFORE streaming starts.
        // This enables tool execution to begin DURING streaming.

        // Resolve model-level tool output cap from current main model
        let max_tool_output_chars = self
            .selections
            .get_or_main(cocode_protocol::ModelRole::Main)
            .and_then(|sel| {
                self.model_hub
                    .get_model_with_info(&sel.model)
                    .ok()
                    .and_then(|(_, info, _)| info.max_tool_output_chars)
            });

        let executor_config = ExecutorConfig {
            session_id: query_tracking.chain_id.clone(),
            turn_id: turn_id.clone(),
            turn_number: self.turn_number,
            permission_mode: self.config.permission_mode,
            cwd: self.context.environment.cwd.clone(),
            is_plan_mode: self.plan_mode_state.is_active,
            plan_file_path: self.plan_mode_state.plan_file_path.clone(),
            features: self.features.clone(),
            web_search_config: self.web_search_config.clone(),
            web_fetch_config: self.web_fetch_config.clone(),
            max_tool_output_chars,
            ..ExecutorConfig::default()
        };
        let mut executor = StreamingToolExecutor::new(
            self.tool_registry.clone(),
            executor_config,
            Some(self.event_tx.clone()),
        )
        .with_cancel_token(self.cancel_token.clone())
        .with_hooks(self.hooks.clone())
        // Share the file tracker across turns for change detection
        .with_file_tracker(self.shared_tools_file_tracker.clone())
        // Share the approval store across turns for permission persistence
        .with_approval_store(self.shared_approval_store.clone())
        // Share async hook tracker for background hook completion tracking
        .with_async_hook_tracker(self.async_hook_tracker.clone())
        // Share the shell executor for command execution and background tasks
        .with_shell_executor(self.shell_executor.clone())
        // Share OTel manager for tool execution metrics/events
        .with_otel_manager(self.otel_manager.clone());

        // Wire file backup store from snapshot manager for Tier 1 rewind
        if let Some(ref sm) = self.snapshot_manager {
            executor = executor.with_file_backup_store(sm.backup_store().clone());
        }

        // Wire permission rules into executor
        if !self.permission_rules.is_empty() {
            let evaluator =
                cocode_tools::PermissionRuleEvaluator::with_rules(self.permission_rules.clone());
            executor = executor.with_permission_evaluator(evaluator);
        }

        // Add spawn_agent_fn if available for Task tool
        if let Some(ref spawn_fn) = self.spawn_agent_fn {
            executor = executor.with_spawn_agent_fn(spawn_fn.clone());
        }

        // Wire task type restrictions for Task(type) syntax
        if let Some(ref restrictions) = self.task_type_restrictions {
            executor = executor.with_task_type_restrictions(restrictions.clone());
        }

        // Wire model_call_fn for SmartEdit LLM correction (prefer Fast model, fallback to Main)
        {
            let hub = self.model_hub.clone();
            let sels = self.selections.clone();
            let model_call_fn: ModelCallFn = std::sync::Arc::new(move |input: ModelCallInput| {
                let hub = hub.clone();
                let sels = sels.clone();
                Box::pin(async move {
                    let (model, _provider) = hub
                        .get_model_for_role_with_selections(cocode_protocol::ModelRole::Fast, &sels)
                        .map_err(cocode_error::boxed_err)?;
                    let response = model
                        .generate_object(input.request)
                        .await
                        .map_err(|e| cocode_error::boxed(e, cocode_error::StatusCode::External))?;
                    Ok(ModelCallResult { response })
                })
            });
            executor = executor.with_model_call_fn(model_call_fn);
        }

        // Add skill_manager if available for Skill tool
        if let Some(ref sm) = self.skill_manager {
            executor = executor.with_skill_manager(sm.clone());
        }

        // Add LSP server manager if available
        if let Some(ref lm) = self.lsp_manager {
            executor = executor.with_lsp_manager(lm.clone());
        }

        // Wire question responder for AskUserQuestion tool
        executor = executor.with_question_responder(self.question_responder.clone());

        // Share invoked skills tracker with the executor so the driver
        // can read which skills were invoked during tool execution
        executor.set_invoked_skills(self.invoked_skills_tracker.clone());

        // Apply active skill-level tool restrictions if set
        if let Some(ref allowed) = self.active_skill_allowed_tools {
            executor.set_skill_allowed_tools(Some(allowed.clone()));
        }

        // Pass parent selections for subagent isolation
        // Subagents spawned via Task tool will inherit these selections,
        // ensuring they're unaffected by changes to this agent's model settings.
        executor = executor.with_parent_selections(self.selections.clone());

        // ── STEP 9: Main API streaming loop with retry ──
        let mut output_recovery_attempts = 0;
        let collected = loop {
            if self.cancel_token.is_cancelled() {
                executor
                    .abort_all(cocode_protocol::AbortReason::UserInterrupted)
                    .await;
                self.set_status(AgentStatus::Idle);
                return Ok(LoopResult::interrupted(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                ));
            }

            match self
                .stream_with_tools(&turn_id, &executor, &injected_messages, query_tracking)
                .await
            {
                Ok(collected) => break collected,
                Err(e) => {
                    // Check if retriable (output token exhaustion)
                    output_recovery_attempts += 1;
                    if output_recovery_attempts >= MAX_OUTPUT_TOKEN_RECOVERY {
                        return Err(e);
                    }
                    self.emit(LoopEvent::Retry {
                        attempt: output_recovery_attempts,
                        max_attempts: MAX_OUTPUT_TOKEN_RECOVERY,
                        delay_ms: 0,
                    })
                    .await;
                    if let Some(otel) = &self.otel_manager {
                        otel.counter(
                            "cocode.api.retry",
                            1,
                            &[("attempt", &output_recovery_attempts.to_string())],
                        );
                    }
                    continue;
                }
            }
        };

        // ── STEP 10: Record API call info ──
        if let Some(usage) = &collected.usage {
            self.total_input_tokens += usage.input_tokens as i32;
            self.total_output_tokens += usage.output_tokens as i32;

            if let Some(otel) = &self.otel_manager {
                otel.histogram("cocode.api.input_tokens", usage.input_tokens, &[]);
                otel.histogram("cocode.api.output_tokens", usage.output_tokens, &[]);
                if let Some(cached) = usage.cache_read_tokens {
                    otel.histogram("cocode.api.cached_tokens", cached, &[]);
                }
                // Record SSE completion event with token breakdown
                otel.sse_event_completed(
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.cache_read_tokens,
                    usage.reasoning_tokens,
                    0, // tool tokens not tracked separately
                );
            }
        }

        let usage = collected.usage.clone().unwrap_or_default();
        self.emit(LoopEvent::StreamRequestEnd {
            usage: usage.clone(),
        })
        .await;

        // Extract text from response
        let response_text: String = collected
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        // Check for tool calls
        let has_tool_calls = collected
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }));

        // Add assistant message to history
        if let Some(turn) = self.message_history.current_turn_mut() {
            let assistant_msg = TrackedMessage::assistant(&response_text, &turn_id, None);
            turn.set_assistant_message(assistant_msg);
            turn.update_usage(usage.clone());
        }

        // ── STEP 11: Check for tool calls ──
        // ── STEP 12: Execute tool queue ──
        // Tool execution already started DURING streaming for safe tools.
        // Now we execute pending unsafe tools and collect all results.
        if has_tool_calls {
            let tool_calls: Vec<_> = collected
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse {
                        id, name, input, ..
                    } => Some(hyper_sdk::ToolCall::new(id, name, input.clone())),
                    _ => None,
                })
                .collect();

            // Execute pending unsafe tools (safe tools already started during streaming)
            executor.execute_pending_unsafe().await;

            // Drain all results (both from streaming and unsafe execution)
            let results = executor.drain().await;

            // ── STEP 13: Handle abort after tool execution ──
            // Check if cancelled during tool execution
            if self.cancel_token.is_cancelled() {
                executor
                    .abort_all(cocode_protocol::AbortReason::UserInterrupted)
                    .await;
                self.set_status(AgentStatus::Idle);
                return Ok(LoopResult::interrupted(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                ));
            }

            // Add tool results to history and apply context modifiers
            self.add_tool_results_to_history(&results, &tool_calls)
                .await;

            // ── Handle plan mode transitions ──
            // Check if EnterPlanMode or ExitPlanMode was called
            for tc in &tool_calls {
                let tc_name = tc.name.as_str();
                match tc_name {
                    name if name == cocode_protocol::ToolName::EnterPlanMode.as_str() => {
                        // Skip if already in plan mode (prevents pre_plan_mode corruption)
                        if self.plan_mode_state.is_active {
                            tracing::warn!(
                                "EnterPlanMode called while already in plan mode, ignoring"
                            );
                            continue;
                        }
                        // Find the result for this tool call to extract plan file path
                        if let Some(result) = results.iter().find(|r| r.call_id == tc.id)
                            && let Ok(output) = &result.result
                        {
                            if let ToolResultContent::Structured(json) = &output.content {
                                if let (Some(path_str), Some(slug)) = (
                                    json.get("planFilePath").and_then(|v| v.as_str()),
                                    json.get("slug").and_then(|v| v.as_str()),
                                ) {
                                    let path = std::path::PathBuf::from(path_str);
                                    self.plan_mode_state.enter_with_mode(
                                        path,
                                        slug.to_string(),
                                        self.turn_number,
                                        self.config.permission_mode,
                                    );
                                    // Enforce Plan permission mode so the executor
                                    // blocks non-read-only tools (especially Bash).
                                    self.config.permission_mode =
                                        cocode_protocol::PermissionMode::Plan;
                                    info!(turn = self.turn_number, "Entered plan mode");
                                }
                            }
                        }
                    }
                    name if name == cocode_protocol::ToolName::ExitPlanMode.as_str() => {
                        // Skip if not in plan mode (prevents spurious state changes)
                        if !self.plan_mode_state.is_active {
                            tracing::warn!("ExitPlanMode called while not in plan mode, ignoring");
                            continue;
                        }
                        // Update plan mode state and restore pre-plan permission mode
                        let restored_mode = self.plan_mode_state.exit(self.turn_number);
                        if let Some(mode) = restored_mode {
                            self.config.permission_mode = mode;
                            info!(
                                turn = self.turn_number,
                                ?mode,
                                "Restored permission mode after plan exit"
                            );
                        }

                        // Extract allowedPrompts from the tool result's structured JSON
                        let allowed_prompts = results
                            .iter()
                            .find(|r| r.call_id == tc.id)
                            .and_then(|r| r.result.as_ref().ok())
                            .and_then(|output| match &output.content {
                                ToolResultContent::Structured(json) => {
                                    json.get("allowedPrompts")?.as_array().map(|arr| {
                                        arr.iter()
                                            .filter_map(|item| {
                                                Some(cocode_protocol::AllowedPrompt {
                                                    tool: item.get("tool")?.as_str()?.to_string(),
                                                    prompt: item
                                                        .get("prompt")?
                                                        .as_str()?
                                                        .to_string(),
                                                })
                                            })
                                            .collect::<Vec<_>>()
                                    })
                                }
                                _ => None,
                            })
                            .unwrap_or_default();

                        info!(
                            turn = self.turn_number,
                            allowed_prompts_count = allowed_prompts.len(),
                            "Exited plan mode"
                        );

                        // If we reach here, the tool executed — meaning the
                        // user approved via the check_permission dialog.
                        // (If the user denied, the tool would not have
                        // executed and we wouldn't be here.)
                        // The exit_option will be determined by the TUI approval
                        // dialog and passed back through the session layer.
                        return Ok(LoopResult::plan_mode_exit(
                            self.turn_number,
                            self.total_input_tokens,
                            self.total_output_tokens,
                            true, // approved: user approved via permission dialog
                            None, // exit_option: determined by TUI layer
                            allowed_prompts,
                            collected.content,
                        ));
                    }
                    _ => {}
                }
            }

            // Track tool calls for extraction triggering
            for _ in &tool_calls {
                auto_compact_tracking.record_tool_call();
            }
        }

        // ── STEP 14: Check for hook stop ──
        // Hook execution is deferred to a future session.

        // ── STEP 15: Update auto-compact tracking ──
        auto_compact_tracking.turn_counter += 1;

        // ── STEP 15.5: Check session memory extraction trigger ──
        // This runs a background agent to proactively update summary.md
        if let Some(ref extraction_agent) = self.extraction_agent {
            let estimated_tokens = self.message_history.estimate_tokens();
            let is_compacting = false; // We're not currently in a compaction

            if extraction_agent.should_trigger(
                auto_compact_tracking,
                estimated_tokens,
                is_compacting,
            ) {
                // Build conversation text for extraction
                let messages = self.message_history.messages_for_api();
                let conversation_text: String = messages
                    .iter()
                    .map(|m| format!("{m:?}"))
                    .collect::<Vec<_>>()
                    .join("\n");

                let current_tokens = estimated_tokens;
                let tool_calls_since = auto_compact_tracking.tool_calls_since_extraction();
                let last_message_id = turn_id.clone();
                let message_count = messages.len() as i32;

                // Mark extraction as started
                auto_compact_tracking.mark_extraction_started();

                // Clone what we need for the background task
                let agent = Arc::clone(extraction_agent);
                let tracking_current_tokens = current_tokens;

                // Spawn extraction in background (non-blocking)
                tokio::spawn(async move {
                    match agent
                        .run_extraction(
                            &conversation_text,
                            tracking_current_tokens,
                            tool_calls_since,
                            &last_message_id,
                            message_count,
                        )
                        .await
                    {
                        Ok(result) => {
                            debug!(
                                summary_tokens = result.summary_tokens,
                                last_id = %result.last_summarized_id,
                                "Background extraction completed"
                            );
                            // Note: We can't update auto_compact_tracking here since
                            // it's owned by the main loop. The next turn will see
                            // the updated summary.md file.
                        }
                        Err(e) => {
                            warn!(error = %e, "Background extraction failed");
                        }
                    }
                });
            }
        }

        // ── STEP 16: Process queued commands and attachments ──
        // Deferred to future sessions.

        // ── STEP 17: Check max turns limit ──
        if let Some(max) = self.config.max_turns
            && self.turn_number >= max
        {
            self.emit(LoopEvent::MaxTurnsReached).await;
            return Ok(LoopResult::max_turns_reached(
                self.turn_number,
                self.total_input_tokens,
                self.total_output_tokens,
            ));
        }

        // Emit turn completed
        self.emit(LoopEvent::TurnCompleted {
            turn_id: turn_id.clone(),
            usage,
        })
        .await;
        if let Some(otel) = &self.otel_manager {
            otel.record_duration("cocode.turn.duration_ms", turn_start.elapsed(), &[]);
            otel.counter("cocode.turn.completed", 1, &[]);
        }

        // ── STEP 16.5: Finalize turn snapshot (rewind support) ──
        // Collect file backups from this turn and the ghost commit (if created)
        // into a TurnSnapshot entry on the snapshot stack.
        if let Some(ref sm) = self.snapshot_manager {
            sm.finalize_turn_snapshot(&turn_id, self.turn_number, turn_ghost_commit)
                .await;
        }

        // ── STEP 18: Recurse or return ──
        match collected.finish_reason {
            FinishReason::Stop => {
                // Turn completed with stop - set status to Idle
                self.set_status(AgentStatus::Idle);
                Ok(LoopResult::completed(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                    response_text,
                    collected.content,
                ))
            }
            FinishReason::ToolCalls => {
                // Tool call turns don't have fresh user input - only tool results
                self.current_turn_has_user_input = false;
                // Recursive call for next turn (boxed to avoid infinite future size)
                Box::pin(self.core_message_loop(query_tracking, auto_compact_tracking)).await
            }
            FinishReason::MaxTokens => {
                // Output token recovery already handled in step 9
                self.set_status(AgentStatus::Idle);
                Ok(LoopResult::completed(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                    response_text,
                    collected.content,
                ))
            }
            other => {
                warn!(?other, "Unexpected finish reason");
                self.set_status(AgentStatus::Idle);
                Ok(LoopResult::completed(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                    response_text,
                    collected.content,
                ))
            }
        }
    }

    /// Stream an API request and collect the response.
    ///
    /// Uses `ApiClient::stream_request()` with tool definitions from the
    /// registry. Includes stall detection based on `stall_detection` config.
    ///
    /// **Key feature**: Tool execution starts DURING streaming. When a ToolUse
    /// block is received, safe tools begin execution immediately via the
    /// executor. This enables concurrent tool execution while the LLM continues
    /// generating output.
    ///
    /// # Arguments
    ///
    /// * `turn_id` - Unique identifier for this turn
    /// * `executor` - Tool executor for handling tool calls
    /// * `injected_messages` - Injected messages from system reminders
    /// * `query_tracking` - Query tracking info containing the real session_id (chain_id)
    async fn stream_with_tools(
        &mut self,
        turn_id: &str,
        executor: &StreamingToolExecutor,
        injected_messages: &[InjectedMessage],
        query_tracking: &QueryTracking,
    ) -> crate::error::Result<CollectedResponse> {
        debug!(turn_id, "Sending API request");

        // Get model and build request using ModelHub
        // Use the real session_id from query_tracking instead of extracting from turn_id
        let session_id = &query_tracking.chain_id;
        let (ctx, model) = self
            .model_hub
            .prepare_main_with_selections(&self.selections, session_id, self.turn_number)
            .context(agent_loop_error::PrepareMainModelSnafu)?;

        // Build messages and tools using existing logic (model-aware filtering)
        let (messages, tools) = self.build_messages_and_tools(injected_messages, &ctx.model_info);

        // Tell the executor which tool names the model was actually given.
        // Any tool call outside this set is rejected as NotFound, preventing
        // hallucinated calls to apply_patch (when type=None/Shell) or tools
        // outside experimental_supported_tools.
        executor.set_allowed_tool_names(tools.iter().map(|d| d.name.clone()).collect());

        // Use RequestBuilder to assemble the final request with context parameters
        let mut builder = RequestBuilder::new(ctx).messages(messages);
        if !tools.is_empty() {
            builder = builder.tools(tools);
        }
        if let Some(max_tokens) = self.config.max_tokens {
            builder = builder.max_tokens(max_tokens);
        }

        let request = builder.build();

        let api_request_start = Instant::now();
        let stream_result = self
            .api_client
            .stream_request(&*model, request, StreamOptions::streaming())
            .await;
        let api_connect_duration = api_request_start.elapsed();

        // Record API request connection event
        if let Some(otel) = &self.otel_manager {
            let (status, error) = match &stream_result {
                Ok(_) => (Some(200u16), None),
                Err(e) => (None, Some(e.to_string())),
            };
            otel.record_api_request(1, status, error.as_deref(), api_connect_duration);
        }

        let mut stream = stream_result.context(agent_loop_error::ApiStreamSnafu)?;

        let mut all_content: Vec<ContentBlock> = Vec::new();
        let mut final_usage: Option<TokenUsage> = None;
        let mut final_finish_reason = FinishReason::Stop;

        // Stall detection configuration
        let stall_timeout = self.config.stall_detection.stall_timeout;
        let stall_enabled = self.config.stall_detection.enabled;
        let mut last_event_time = Instant::now();

        // Process streaming results with stall detection
        loop {
            let next_event = stream.next();

            // Use tokio::select! for stall detection and cancellation
            let result = if stall_enabled {
                let timeout_at = last_event_time + stall_timeout;
                let remaining = timeout_at.saturating_duration_since(Instant::now());

                tokio::select! {
                    biased;
                    _ = self.cancel_token.cancelled() => {
                        // Cancelled during streaming — break out
                        break;
                    }
                    result = next_event => result,
                    _ = tokio::time::sleep(remaining) => {
                        // Stream stall detected
                        self.emit(LoopEvent::StreamStallDetected {
                            turn_id: turn_id.to_string(),
                            timeout: stall_timeout,
                        }).await;

                        // Handle based on recovery strategy
                        let strategy = self.config.stall_detection.recovery;
                        match strategy {
                            cocode_protocol::StallRecovery::Abort => {}
                            cocode_protocol::StallRecovery::Retry => {
                                warn!(turn_id, timeout = ?stall_timeout, "Stream stalled, retrying");
                            }
                            cocode_protocol::StallRecovery::Fallback => {
                                // Attempt model fallback
                                if self.fallback_state.should_fallback(&self.fallback_config)
                                    && let Some(fallback_model) = self.fallback_state.next_model(&self.fallback_config) {
                                        let from_model = self.fallback_state.current_model.clone();
                                        self.emit(LoopEvent::ModelFallbackStarted {
                                            from: from_model.clone(),
                                            to: fallback_model.clone(),
                                            reason: format!("Stream stalled for {stall_timeout:?}"),
                                        }).await;
                                        self.fire_notification_hook(
                                            "model_fallback",
                                            "Model fallback",
                                            &format!("Falling back from {from_model} to {fallback_model}"),
                                        ).await;
                                        self.fallback_state.record_fallback(
                                            fallback_model,
                                            format!("Stream stalled for {stall_timeout:?}"),
                                        );
                                        if let Some(otel) = &self.otel_manager {
                                            otel.counter("cocode.model.fallback", 1, &[]);
                                        }
                                    }
                            }
                        }

                        return agent_loop_error::StreamStallSnafu {
                            timeout: format!("{stall_timeout:?}"),
                            strategy,
                        }.fail();
                    }
                }
            } else {
                tokio::select! {
                    biased;
                    _ = self.cancel_token.cancelled() => {
                        break;
                    }
                    result = next_event => result,
                }
            };

            // Process the result
            let Some(result) = result else {
                break; // Stream ended
            };

            let result = result.map_err(|e| {
                // Check if this is an overload error for fallback handling
                let err_str = e.to_string();
                if (err_str.contains("overload") || err_str.contains("rate_limit"))
                    && self.fallback_state.should_fallback(&self.fallback_config)
                    && let Some(fallback_model) =
                        self.fallback_state.next_model(&self.fallback_config)
                {
                    // Note: We can't emit async events here, but we record the fallback
                    self.fallback_state
                        .record_fallback(fallback_model, format!("API error: {err_str}"));
                    if let Some(otel) = &self.otel_manager {
                        otel.counter("cocode.model.fallback", 1, &[]);
                    }
                }
                error!("Stream error from provider: {e}");
                agent_loop_error::StreamSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

            // Update stall timer on any event
            last_event_time = Instant::now();

            match result.result_type {
                QueryResultType::Assistant => {
                    // Emit text deltas for UI and process tool uses DURING streaming
                    for block in &result.content {
                        match block {
                            ContentBlock::Text { text } if !text.is_empty() => {
                                self.emit(LoopEvent::TextDelta {
                                    turn_id: turn_id.to_string(),
                                    delta: text.clone(),
                                })
                                .await;
                            }
                            ContentBlock::Thinking { content, .. } if !content.is_empty() => {
                                self.emit(LoopEvent::ThinkingDelta {
                                    turn_id: turn_id.to_string(),
                                    delta: content.clone(),
                                })
                                .await;
                            }
                            ContentBlock::ToolUse {
                                id, name, input, ..
                            } => {
                                // Start tool execution DURING streaming!
                                // Safe tools begin immediately; unsafe tools are queued.
                                let tool_call = ToolCall::new(id, name, input.clone());
                                executor.on_tool_complete(tool_call).await;
                            }
                            _ => {}
                        }
                    }
                    all_content.extend(result.content);

                    // Capture usage from non-streaming responses
                    if result.usage.is_some() {
                        final_usage = result.usage;
                    }
                    if let Some(fr) = result.finish_reason {
                        final_finish_reason = fr;
                    }
                }
                QueryResultType::Done => {
                    final_usage = result.usage;
                    if let Some(fr) = result.finish_reason {
                        final_finish_reason = fr;
                    }
                    break;
                }
                QueryResultType::Error => {
                    let msg = result.error.unwrap_or_else(|| "Unknown error".to_string());

                    // Check for overload errors and handle fallback
                    if (msg.contains("overload") || msg.contains("rate_limit"))
                        && self.fallback_state.should_fallback(&self.fallback_config)
                        && let Some(fallback_model) =
                            self.fallback_state.next_model(&self.fallback_config)
                    {
                        self.emit(LoopEvent::ModelFallbackStarted {
                            from: self.fallback_state.current_model.clone(),
                            to: fallback_model.clone(),
                            reason: msg.clone(),
                        })
                        .await;
                        self.fallback_state
                            .record_fallback(fallback_model, msg.clone());
                        if let Some(otel) = &self.otel_manager {
                            otel.counter("cocode.model.fallback", 1, &[]);
                        }
                    }

                    error!("Stream error from provider: {msg}");
                    return agent_loop_error::StreamSnafu { message: msg }.fail();
                }
                QueryResultType::Retry | QueryResultType::Event => {
                    // Continue
                }
            }
        }

        Ok(CollectedResponse {
            content: all_content,
            usage: final_usage,
            finish_reason: final_finish_reason,
        })
    }

    /// Build messages and tool definitions for the API request.
    ///
    /// This extracts the message/tool building logic for use with `RequestBuilder`.
    /// Tool definitions are filtered per-model based on `ModelInfo` capabilities.
    ///
    /// # Arguments
    ///
    /// * `injected_messages` - Injected messages from system reminders
    /// * `model_info` - Model information for tool filtering
    fn build_messages_and_tools(
        &self,
        injected_messages: &[InjectedMessage],
        model_info: &cocode_protocol::ModelInfo,
    ) -> (Vec<Message>, Vec<ToolDefinition>) {
        // Build system prompt (use custom prompt if set, otherwise generate from builder)
        let system_prompt = if let Some(ref custom) = self.custom_system_prompt {
            custom.clone()
        } else {
            SystemPromptBuilder::build(&self.context)
        };

        // Get conversation messages
        let messages = self.message_history.messages_for_api();

        // Build messages with system, reminders, and conversation
        let mut all_messages = vec![Message::system(&system_prompt)];

        // Inject system reminders as individual messages before the conversation
        // This supports both text reminders and multi-message tool_use/tool_result pairs
        for msg in injected_messages {
            all_messages.push(self.convert_injected_message(msg));
        }

        all_messages.extend(messages);

        // Get tool definitions with model-aware filtering
        let tools = self.select_tools_for_model(model_info);

        (all_messages, tools)
    }

    fn select_tools_for_model(
        &self,
        model_info: &cocode_protocol::ModelInfo,
    ) -> Vec<ToolDefinition> {
        select_tools_for_model(
            self.tool_registry.definitions_filtered(&self.features),
            model_info,
        )
    }

    /// Convert an injected message to an API Message.
    fn convert_injected_message(&self, msg: &InjectedMessage) -> Message {
        match msg {
            InjectedMessage::UserText { content, .. } => {
                // Text reminders become simple user messages
                Message::user(content.as_str())
            }
            InjectedMessage::AssistantBlocks { blocks, .. } => {
                // Assistant blocks (typically tool_use) become assistant messages
                let content_blocks: Vec<ContentBlock> =
                    blocks.iter().map(Self::convert_injected_block).collect();
                Message::new(hyper_sdk::Role::Assistant, content_blocks)
            }
            InjectedMessage::UserBlocks { blocks, .. } => {
                // User blocks (typically tool_result) become user messages
                let content_blocks: Vec<ContentBlock> =
                    blocks.iter().map(Self::convert_injected_block).collect();
                Message::new(hyper_sdk::Role::User, content_blocks)
            }
        }
    }

    /// Convert an injected block to a hyper_sdk ContentBlock.
    fn convert_injected_block(block: &InjectedBlock) -> ContentBlock {
        match block {
            InjectedBlock::Text(text) => ContentBlock::text(text.as_str()),
            InjectedBlock::ToolUse { id, name, input } => {
                ContentBlock::tool_use(id.as_str(), name.as_str(), input.clone())
            }
            InjectedBlock::ToolResult {
                tool_use_id,
                content,
            } => ContentBlock::tool_result(
                tool_use_id.as_str(),
                hyper_sdk::ToolResultContent::text(content.as_str()),
                false,
            ),
        }
    }

    /// Micro-compaction: remove old tool results to save tokens (no LLM call).
    ///
    /// Uses `ThresholdStatus` to determine if micro-compaction is needed based on
    /// current context usage relative to the warning threshold.
    ///
    /// Also cleans up FileTracker entries for compacted Read tool results,
    /// while preserving files from recent turns using `collect_files_to_keep`.
    ///
    /// Returns a tuple of (removed_count, tokens_saved).
    async fn micro_compact(&mut self) -> (i32, i32) {
        // Check if micro-compact is enabled
        if !self.compact_config.is_micro_compact_enabled() {
            return (0, 0);
        }

        let tokens_before = self.message_history.estimate_tokens();
        let context_window = self.context.environment.context_window;

        // Use ThresholdStatus to check if we're above warning threshold
        let status =
            ThresholdStatus::calculate(tokens_before, context_window, &self.compact_config);

        if !status.is_above_warning_threshold {
            debug!(
                tokens_before,
                status = status.status_description(),
                "Below warning threshold, skipping micro-compact"
            );
            return (0, 0);
        }

        // Apply micro-compaction using configured recent_tool_results_to_keep
        // Get paths from ContextModifier::FileRead for FileTracker cleanup
        let keep_count = self.compact_config.recent_tool_results_to_keep;
        let outcome = self.message_history.micro_compact_outcome(keep_count);

        // Clean up FileTracker entries for compacted reads using paths from modifiers
        // This is more accurate than tool_id mapping since it uses actual file paths
        if !outcome.cleared_read_paths.is_empty() {
            // Determine how many recent turns to preserve files from
            // This matches Claude Code's collectFilesToKeep behavior
            let keep_recent_turns = 3; // Keep files from last 3 turns
            let files_to_keep =
                crate::compaction::collect_files_to_keep(&self.message_history, keep_recent_turns);

            let tracker = self.shared_tools_file_tracker.lock().await;

            // Collect paths to remove (excluding preserved files)
            let paths_to_remove: Vec<_> = outcome
                .cleared_read_paths
                .iter()
                .filter(|p| !files_to_keep.contains(*p))
                .cloned()
                .collect();

            if !paths_to_remove.is_empty() {
                tracker.remove_paths(&paths_to_remove);
            }

            debug!(
                cleared_paths = outcome.cleared_read_paths.len(),
                removed_paths = paths_to_remove.len(),
                files_preserved = files_to_keep.len(),
                "Cleaned up FileTracker entries for compacted reads (preserved recent files)"
            );
        }

        // Calculate tokens saved
        let tokens_after = self.message_history.estimate_tokens();
        let tokens_saved = tokens_before - tokens_after;

        debug!(
            removed = outcome.compacted_count,
            tokens_before, tokens_after, tokens_saved, "Micro-compaction complete"
        );

        (outcome.compacted_count, tokens_saved)
    }

    /// Build a per-turn derived FileTracker view from the shared tracker snapshot.
    ///
    /// Creates a temporary FileTracker populated with a read-only snapshot of the
    /// shared tools tracker. This allows system reminder generators to read file
    /// state without holding the shared tracker lock during the entire generation
    /// phase.
    ///
    /// # Claude Code Alignment
    ///
    /// CODEX's per-turn derived tracker view pattern: snapshot → release lock →
    /// pass view to generators → bridge mention reads back afterward.
    async fn build_reminder_tracker_view(&self) -> FileTracker {
        let snapshot = {
            let tools_tracker = self.shared_tools_file_tracker.lock().await;
            tools_tracker.read_files_snapshot()
        };
        // Lock is released here
        let tracker = FileTracker::new();
        tracker.replace_snapshot(snapshot);
        tracker
    }

    /// Apply mention read records from system reminder generation to the shared tracker.
    ///
    /// After `generate_all()` completes, generators may have pushed `MentionReadRecord`
    /// entries into the shared buffer. This method drains those records and applies
    /// them to the canonical shared tools FileTracker.
    async fn apply_mention_read_records(&self, records: &[MentionReadRecord]) {
        if records.is_empty() {
            return;
        }
        let tracker = self.shared_tools_file_tracker.lock().await;
        for record in records {
            let state = match record.read_kind {
                cocode_protocol::FileReadKind::FullContent => FileReadState::complete_with_turn(
                    record.content.clone(),
                    record.last_modified,
                    record.read_turn,
                ),
                cocode_protocol::FileReadKind::PartialContent => FileReadState::partial_with_turn(
                    record.offset.unwrap_or(0),
                    record.limit.unwrap_or(0),
                    record.last_modified,
                    record.read_turn,
                ),
                cocode_protocol::FileReadKind::MetadataOnly => {
                    FileReadState::metadata_only(record.last_modified, record.read_turn)
                }
            };
            tracker.record_read_with_state(record.path.clone(), state);
        }
        debug!(
            count = records.len(),
            "Applied mention read records to FileTracker"
        );
    }

    /// Rebuild FileTracker from restored file context after compaction.
    ///
    /// After compaction restores files, the FileTracker must be rebuilt to match
    /// the restored context. This replaces ALL tracker entries with entries
    /// derived from the restored files.
    ///
    /// # Claude Code Alignment
    ///
    /// Claude Code clears readFileState entirely during compaction and rebuilds
    /// from restored files only.
    async fn rebuild_trackers_from_restored_files(&self, files: &[FileRestoration]) {
        let mut entries = Vec::with_capacity(files.len());
        for file in files {
            let file_mtime = std::fs::metadata(&file.path)
                .ok()
                .and_then(|m| m.modified().ok());
            entries.push((
                file.path.clone(),
                FileReadState::complete_with_turn(
                    file.content.clone(),
                    file_mtime,
                    self.turn_number,
                ),
            ));
        }
        let tracker = self.shared_tools_file_tracker.lock().await;
        tracker.replace_snapshot(entries);
        debug!(
            files_count = files.len(),
            "Rebuilt FileTracker from restored files"
        );
    }

    /// Restore FileTracker state for rewind.
    ///
    /// When a rewind occurs, the FileTracker needs to be restored to match
    /// the state at the target turn. This extracts all file reads from
    /// historical tool calls up to that turn and rebuilds the tracker state.
    ///
    /// # Claude Code Alignment
    ///
    /// This matches Claude Code v2.1.38's rewind file state restoration:
    /// - Extract file reads from ContextModifier::FileRead in tool calls
    /// - Clear current FileTracker state
    /// - Rebuild state from historical reads
    async fn restore_file_tracker_for_rewind(&mut self, to_turn: i32) {
        // Extract file reads from history up to the target turn
        let extractions = self.message_history.extract_file_reads_up_to_turn(to_turn);

        if extractions.is_empty() {
            debug!(to_turn, "No file reads to restore for rewind");
            return;
        }

        // Clear current FileTracker state and rebuild
        let tracker = self.shared_tools_file_tracker.lock().await;
        tracker.clear_reads();

        // Convert mtime from ms if provided
        let convert_mtime = |ms: Option<i64>| -> Option<std::time::SystemTime> {
            ms.and_then(|ms| {
                std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_millis(ms as u64))
            })
        };

        for extraction in extractions {
            let file_mtime = convert_mtime(extraction.file_mtime_ms);

            let state = match extraction.kind {
                cocode_protocol::FileReadKind::FullContent => {
                    if let Some(content) = extraction.content {
                        cocode_tools::FileReadState::complete_with_turn(
                            content,
                            file_mtime,
                            extraction.read_turn,
                        )
                    } else {
                        // Content was compacted, just track metadata
                        cocode_tools::FileReadState::metadata_only(file_mtime, extraction.read_turn)
                    }
                }
                cocode_protocol::FileReadKind::PartialContent => {
                    cocode_tools::FileReadState::partial_with_turn(
                        extraction.offset.unwrap_or(0),
                        extraction.limit.unwrap_or(0),
                        file_mtime,
                        extraction.read_turn,
                    )
                }
                cocode_protocol::FileReadKind::MetadataOnly => {
                    cocode_tools::FileReadState::metadata_only(file_mtime, extraction.read_turn)
                }
            };

            tracker.track_read(extraction.path.clone(), state);
        }

        debug!(
            to_turn,
            restored_count = tracker.read_count(),
            "Restored FileTracker state for rewind"
        );
    }

    /// Run auto-compaction (LLM-based summarization).
    ///
    /// Uses the 9-section compact instructions from `build_compact_instructions()`
    /// to generate a comprehensive conversation summary.
    ///
    /// Before compaction begins, PreCompact hooks are executed. If any hook
    /// returns `Reject`, compaction is skipped and the rejection is logged.
    async fn compact(
        &mut self,
        tracking: &mut AutoCompactTracking,
        turn_id: &str,
        query_tracking: &QueryTracking,
    ) -> crate::error::Result<()> {
        // Execute PreCompact hooks before starting compaction
        let hook_ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::PreCompact,
            turn_id.to_string(),
            self.context.environment.cwd.clone(),
        );

        let outcomes = self.hooks.execute(&hook_ctx).await;

        // Check if any hook rejected compaction
        for outcome in &outcomes {
            // Emit HookExecuted event for each hook
            self.emit(LoopEvent::HookExecuted {
                hook_type: HookEventType::PreCompact,
                hook_name: outcome.hook_name.clone(),
            })
            .await;

            if let cocode_hooks::HookResult::Reject { reason } = &outcome.result {
                info!(
                    hook_name = %outcome.hook_name,
                    reason = %reason,
                    "Compaction skipped by hook"
                );
                self.emit(LoopEvent::CompactionSkippedByHook {
                    hook_name: outcome.hook_name.clone(),
                    reason: reason.clone(),
                })
                .await;
                return Ok(());
            }
        }

        // Update status to compacting
        self.set_status(AgentStatus::Compacting);
        self.emit(LoopEvent::CompactionStarted).await;
        if let Some(otel) = &self.otel_manager {
            otel.counter("cocode.compaction.started", 1, &[]);
        }

        // Estimate tokens before compaction
        let tokens_before = self.message_history.estimate_tokens();

        // Build summarization prompt from conversation text
        let messages = self.message_history.messages_for_api();
        let conversation_text: String = messages
            .iter()
            .map(|m| format!("{m:?}"))
            .collect::<Vec<_>>()
            .join("\n");

        // Use the 9-section compact instructions
        let max_output_tokens = self.compact_config.max_compact_output_tokens;
        let system_prompt = build_compact_instructions(max_output_tokens);

        // Fallback to legacy prompt builder if available
        let (_, user_prompt) = SystemPromptBuilder::build_summarization(&conversation_text, None);

        // Use the API client to get a summary with retry mechanism
        let max_retries = self.compact_config.max_summary_retries;
        let mut attempt = 0;

        let summary_text = loop {
            attempt += 1;
            let last_error: String;

            // Build request for each attempt
            let summary_messages =
                vec![Message::system(&system_prompt), Message::user(&user_prompt)];

            // Get compact model and build request using ModelHub
            // Use the real session_id from query_tracking
            let session_id = &query_tracking.chain_id;
            let (ctx, compact_model) = self
                .model_hub
                .prepare_compact_with_selections(&self.selections, session_id, self.turn_number)
                .context(agent_loop_error::PrepareCompactModelSnafu)?;

            // Use RequestBuilder for the summary request
            let summary_request = RequestBuilder::new(ctx)
                .messages(summary_messages.clone())
                .max_tokens(max_output_tokens)
                .build();

            match self
                .api_client
                .generate(&*compact_model, summary_request)
                .await
            {
                Ok(response) => {
                    // Extract summary text
                    let text: String = response
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect();

                    if text.is_empty() {
                        last_error = "Empty summary produced".to_string();
                        if attempt <= max_retries {
                            // Exponential backoff: 1s, 2s, 4s, ...
                            let delay_ms = 1000 * (1 << (attempt - 1));
                            self.emit(LoopEvent::CompactionRetry {
                                attempt,
                                max_attempts: max_retries + 1,
                                delay_ms,
                                reason: last_error.clone(),
                            })
                            .await;
                            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms as u64))
                                .await;
                            continue;
                        }
                    } else {
                        break text;
                    }
                }
                Err(e) => {
                    last_error = e.to_string();
                    if attempt <= max_retries {
                        // Exponential backoff: 1s, 2s, 4s, ...
                        let delay_ms = 1000 * (1 << (attempt - 1));
                        warn!(
                            attempt,
                            max_retries,
                            error = %last_error,
                            delay_ms,
                            "Compaction API call failed, retrying"
                        );
                        self.emit(LoopEvent::CompactionRetry {
                            attempt,
                            max_attempts: max_retries + 1,
                            delay_ms,
                            reason: last_error.clone(),
                        })
                        .await;
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms as u64))
                            .await;
                        continue;
                    }
                }
            }

            // All retries exhausted
            warn!(
                attempts = attempt,
                error = %last_error,
                "Compaction failed after all retries"
            );
            self.emit(LoopEvent::CompactionFailed {
                attempts: attempt,
                error: last_error,
            })
            .await;
            return Ok(());
        };

        // Extract task status from tool calls before compaction
        let tool_calls: Vec<(String, serde_json::Value)> = self
            .message_history
            .turns()
            .iter()
            .flat_map(|turn| {
                turn.tool_calls
                    .iter()
                    .map(|tc| (tc.name.clone(), tc.input.clone()))
            })
            .collect();

        let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);

        // Extract invoked skills from tool calls with turn numbers
        let tool_calls_with_turns: Vec<(String, serde_json::Value, i32)> = self
            .message_history
            .turns()
            .iter()
            .flat_map(|turn| {
                let turn_num = turn.number;
                turn.tool_calls
                    .iter()
                    .map(move |tc| (tc.name.clone(), tc.input.clone(), turn_num))
            })
            .collect();

        let invoked_skills = InvokedSkillRestoration::from_tool_calls(&tool_calls_with_turns);

        // Build final summary with task status
        let final_summary = if task_status.tasks.is_empty() {
            summary_text
        } else {
            let tasks_section = task_status
                .tasks
                .iter()
                .map(|t| {
                    let owner = t.owner.as_deref().unwrap_or("unassigned");
                    format!("- [{}] {}: {} ({})", t.status, t.id, t.subject, owner)
                })
                .collect::<Vec<_>>()
                .join("\n");

            format!("{summary_text}\n\n<task_status>\n{tasks_section}\n</task_status>")
        };

        // Calculate keep window using token-based algorithm
        let messages_json = self.message_history.messages_for_api_json();
        let keep_result =
            calculate_keep_start_index(&messages_json, &self.compact_config.keep_window);
        let keep_turns = map_message_index_to_keep_turns(
            self.message_history.turn_count(),
            &messages_json,
            keep_result.keep_start_index,
        );
        let tokens_saved = (tokens_before - self.message_history.estimate_tokens()).max(0);

        debug!(
            keep_turns,
            keep_start_index = keep_result.keep_start_index,
            messages_to_keep = keep_result.messages_to_keep,
            keep_tokens = keep_result.keep_tokens,
            text_messages_kept = keep_result.text_messages_kept,
            "Calculated keep window for compaction"
        );

        // Get transcript path from context if available
        let transcript_path = self.context.transcript_path.clone();

        self.message_history.apply_compaction_with_metadata(
            final_summary.clone(),
            keep_turns,
            turn_id,
            tokens_saved,
            cocode_protocol::CompactTrigger::Auto,
            tokens_before,
            transcript_path.clone(),
            true, // Recent messages are preserved
        );

        // Rebuild FileTracker from remaining messages after compaction
        // This ensures file state is consistent with the compacted history
        {
            let cwd = self.context.environment.cwd.clone();
            let tracker = self.shared_tools_file_tracker.lock().await;
            tracker.clear();

            // Rebuild from remaining messages
            let new_tracker = build_file_read_state(&self.message_history, &cwd, LRU_MAX_ENTRIES);

            // Copy state from new tracker
            for (path, state) in new_tracker.read_files_with_state() {
                tracker.record_read_with_state(path, state.clone());
            }

            debug!(
                tracked_files = tracker.len(),
                "FileTracker rebuilt after compaction"
            );
        }

        // Update tracking
        tracking.mark_compacted(turn_id, self.turn_number);

        // Calculate post-compaction tokens and update boundary
        let post_tokens = self.message_history.estimate_tokens();
        self.message_history
            .update_boundary_post_tokens(post_tokens);

        // Set compaction boundary on the snapshot manager so rewinding
        // cannot go past compacted turns (messages are gone, files would be
        // inconsistent).
        if let Some(ref sm) = self.snapshot_manager {
            sm.set_compaction_boundary(self.turn_number).await;
        }

        // Compaction complete - restore status to Idle
        self.set_status(AgentStatus::Idle);
        if let Some(otel) = &self.otel_manager {
            otel.counter("cocode.compaction.completed", 1, &[]);
        }
        self.emit(LoopEvent::CompactionCompleted {
            removed_messages: 0, // Tracked by MessageHistory
            summary_tokens: post_tokens,
        })
        .await;

        // Emit compact boundary inserted event
        self.emit(LoopEvent::CompactBoundaryInserted {
            trigger: cocode_protocol::CompactTrigger::Auto,
            pre_tokens: tokens_before,
            post_tokens,
        })
        .await;

        // Emit invoked skills restored event if any skills were found
        if !invoked_skills.is_empty() {
            let skill_names: Vec<String> = invoked_skills.iter().map(|s| s.name.clone()).collect();
            self.emit(LoopEvent::InvokedSkillsRestored {
                skills: skill_names,
            })
            .await;
        }

        // Context restoration: restore important files that were read before compaction
        self.restore_context_after_compaction(&invoked_skills, &task_status)
            .await;

        // Save to session memory for future Tier 1 compaction
        if self.compaction_config.session_memory.enabled
            && let Some(ref path) = self.compaction_config.session_memory.summary_path
        {
            let summary_content = final_summary;
            let turn_id_owned = turn_id.to_string();
            let path_owned = path.clone();

            // Spawn background task to write session memory
            tokio::spawn(async move {
                if let Err(e) =
                    write_session_memory(&path_owned, &summary_content, &turn_id_owned).await
                {
                    tracing::warn!(
                        error = %e,
                        path = ?path_owned,
                        "Failed to write session memory"
                    );
                } else {
                    tracing::debug!(
                        path = ?path_owned,
                        "Session memory saved for future Tier 1 compaction"
                    );
                }
            });
        }

        // Execute SessionStart hooks after compaction (with source: 'compact')
        // This allows hooks to provide additional context after compaction
        self.execute_post_compact_hooks(turn_id).await;

        Ok(())
    }

    /// Execute SessionStart hooks after compaction.
    ///
    /// Runs SessionStart hooks with source='compact' to allow them to provide
    /// additional context for the resumed conversation.
    async fn execute_post_compact_hooks(&mut self, turn_id: &str) {
        let hook_ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::SessionStart,
            turn_id.to_string(),
            self.context.environment.cwd.clone(),
        )
        .with_source("compact");

        let outcomes = self.hooks.execute(&hook_ctx).await;

        let mut hooks_executed = 0;
        let mut additional_context_count = 0;

        for outcome in &outcomes {
            // Emit HookExecuted event for each hook
            self.emit(LoopEvent::HookExecuted {
                hook_type: HookEventType::SessionStart,
                hook_name: outcome.hook_name.clone(),
            })
            .await;

            hooks_executed += 1;

            // Check for additional context from hooks
            if let cocode_hooks::HookResult::ContinueWithContext { additional_context } =
                &outcome.result
                && let Some(ctx) = additional_context
                && !ctx.is_empty()
            {
                additional_context_count += 1;
                debug!(
                    hook_name = %outcome.hook_name,
                    context_len = ctx.len(),
                    "Hook provided additional context"
                );
            }
        }

        if hooks_executed > 0 {
            self.emit(LoopEvent::PostCompactHooksExecuted {
                hooks_executed,
                additional_context_count,
            })
            .await;
        }
    }

    /// Execute lifecycle hooks (non-tool events) and emit HookExecuted for each.
    ///
    /// Used for SessionStart, UserPromptSubmit, Stop, SessionEnd, etc.
    /// Returns `true` if any hook rejected (for events that support rejection).
    async fn execute_lifecycle_hooks(&self, ctx: cocode_hooks::HookContext) -> bool {
        let outcomes = self.hooks.execute(&ctx).await;
        let mut rejected = false;

        for outcome in &outcomes {
            self.emit(LoopEvent::HookExecuted {
                hook_type: ctx.event_type.clone(),
                hook_name: outcome.hook_name.clone(),
            })
            .await;

            match &outcome.result {
                cocode_hooks::HookResult::Reject { reason } => {
                    info!(
                        hook_name = %outcome.hook_name,
                        reason = %reason,
                        event = %ctx.event_type,
                        "Lifecycle hook rejected"
                    );
                    rejected = true;
                }
                cocode_hooks::HookResult::Async { task_id, hook_name } => {
                    self.async_hook_tracker
                        .register(task_id.clone(), hook_name.clone());
                }
                cocode_hooks::HookResult::ContinueWithContext {
                    additional_context, ..
                } => {
                    if let Some(ctx_str) = additional_context {
                        info!(
                            hook_name = %outcome.hook_name,
                            event = %ctx.event_type,
                            "Lifecycle hook provided additional context: {ctx_str}"
                        );
                    }
                }
                cocode_hooks::HookResult::SystemMessage { message } => {
                    info!(
                        hook_name = %outcome.hook_name,
                        event = %ctx.event_type,
                        "Lifecycle hook system message: {message}"
                    );
                }
                _ => {}
            }
        }

        rejected
    }

    /// Fire a Notification hook (informational, non-blocking).
    async fn fire_notification_hook(&self, notification_type: &str, title: &str, message: &str) {
        let ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::Notification,
            uuid::Uuid::new_v4().to_string(),
            self.context.environment.cwd.clone(),
        )
        .with_notification_type(notification_type)
        .with_title(title)
        .with_message(message);

        let outcomes = self.hooks.execute(&ctx).await;
        for outcome in &outcomes {
            self.emit(LoopEvent::HookExecuted {
                hook_type: ctx.event_type.clone(),
                hook_name: outcome.hook_name.clone(),
            })
            .await;
        }
    }

    /// Restore context after compaction.
    ///
    /// This method restores important files, skills, and task status that were
    /// tracked before compaction. Files are prioritized by recency and importance.
    ///
    /// # Arguments
    /// * `invoked_skills` - Skills that were invoked before compaction
    /// * `task_status` - Task status restoration data
    /// Collect tracked files suitable for context restoration after compaction.
    ///
    /// Reads current content from disk, applies exclusion patterns, skips internal
    /// files, and limits to the configured max_files count.
    async fn collect_restorable_tracked_files(
        &self,
        file_config: &FileRestorationConfig,
    ) -> Vec<FileRestoration> {
        // Collect files from the file tracker (Layer 2)
        let tracker = self.shared_tools_file_tracker.lock().await;
        let tracked_files = tracker.tracked_files();
        drop(tracker); // Release lock before async operations

        let mut files_for_restoration: Vec<FileRestoration> = Vec::new();

        for path in tracked_files {
            // Skip excluded patterns
            let path_str = path.to_string_lossy();
            if file_config.should_exclude(&path_str) {
                continue;
            }

            // Skip internal files (session memory, plan files, auto memory)
            if is_internal_file(&path, "") {
                debug!(path = %path.display(), "Skipping internal file for restoration");
                continue;
            }

            // Try to read the file content (re-read at compact time for current content)
            // Truncate to max_tokens_per_file limit to avoid large file overhead
            let max_chars = (file_config.max_tokens_per_file * 4) as usize;
            match tokio::fs::read_to_string(&path).await {
                Ok(content) => {
                    // Truncate content if it exceeds per-file limit
                    let (content, truncated) = if content.len() > max_chars {
                        (content[..max_chars].to_string(), true)
                    } else {
                        (content, false)
                    };
                    let tokens = (content.len() / 4) as i32; // Rough estimate

                    // Get last access time from file tracker
                    let tracker = self.shared_tools_file_tracker.lock().await;
                    let last_accessed = if let Some(state) = tracker.read_state(&path) {
                        // Use read_turn as a proxy for access time
                        state.read_turn as i64
                    } else {
                        0
                    };
                    drop(tracker); // Release lock

                    if truncated {
                        debug!(
                            path = %path.display(),
                            tokens = tokens,
                            max_tokens = file_config.max_tokens_per_file,
                            "File truncated to per-file token limit"
                        );
                    }

                    files_for_restoration.push(FileRestoration {
                        path,
                        content,
                        priority: 1, // Default priority
                        tokens,
                        last_accessed,
                    });
                }
                Err(e) => {
                    debug!(path = %path.display(), error = %e, "Failed to read file for restoration");
                }
            }
        }

        // Limit to configured max files
        if files_for_restoration.len() > file_config.max_files as usize {
            // Sort by last_accessed descending (most recent first)
            files_for_restoration.sort_by(|a, b| b.last_accessed.cmp(&a.last_accessed));
            files_for_restoration.truncate(file_config.max_files as usize);
        }

        files_for_restoration
    }

    async fn restore_context_after_compaction(
        &mut self,
        invoked_skills: &[InvokedSkillRestoration],
        task_status: &TaskStatusRestoration,
    ) {
        // Get file restoration config
        let file_config = &self.compact_config.file_restoration;

        let files_for_restoration = self.collect_restorable_tracked_files(file_config).await;

        // Build todo list from task status
        let todos = if !task_status.tasks.is_empty() {
            let todo_text = task_status
                .tasks
                .iter()
                .map(|t| format!("- [{}] {}: {}", t.status, t.id, t.subject))
                .collect::<Vec<_>>()
                .join("\n");
            Some(todo_text)
        } else {
            None
        };

        // Build skills list from invoked skills
        let skills: Vec<String> = invoked_skills.iter().map(|s| s.name.clone()).collect();

        // Get plan content if plan file exists (even if plan mode is not active,
        // the plan may have been approved and should survive compaction)
        let plan = if let Some(plan_path) = &self.plan_mode_state.plan_file_path {
            tokio::fs::read_to_string(plan_path).await.ok()
        } else {
            None
        };

        // Mark that a plan file reference should be injected on the next turn
        // so the model knows the plan still exists after compaction
        if plan.is_some() {
            self.plan_mode_state.needs_plan_reference = true;
        }

        // Build context restoration
        let restoration = build_context_restoration_with_config(
            files_for_restoration,
            todos,
            plan,
            skills,
            file_config,
        );

        // Transfer compacted large file references so CompactFileReferenceGenerator
        // can notify the model on the next turn (one-shot drain pattern)
        self.pending_compacted_large_files = restoration.compacted_large_files.clone();

        // Format and inject restoration message if there's content to restore
        let restoration_message = format_restoration_message(&restoration);
        if !restoration_message.is_empty() {
            let files_count = restoration.files.len();
            debug!(
                files_restored = files_count,
                has_todos = restoration.todos.is_some(),
                has_plan = restoration.plan.is_some(),
                skills_count = restoration.skills.len(),
                "Context restoration completed"
            );

            // Rebuild FileTracker from restored files (Claude Code alignment: C4)
            // After compaction, the tracker must reflect the restored context only
            if !restoration.files.is_empty() {
                self.rebuild_trackers_from_restored_files(&restoration.files)
                    .await;
            }

            // Emit context restoration event
            self.emit(LoopEvent::ContextRestored {
                files_count: files_count as i32,
                has_todos: restoration.todos.is_some(),
                has_plan: restoration.plan.is_some(),
            })
            .await;
        }
    }

    /// Apply a cached session memory summary (Tier 1 compaction).
    ///
    /// This is the zero-cost compaction path that uses a previously saved summary
    /// instead of making an LLM API call. The summary is stored in the session memory
    /// file and can be reused across conversation continuations.
    ///
    /// # Arguments
    /// * `summary` - The cached session memory summary
    /// * `turn_id` - ID of the current turn
    /// * `tracking` - Auto-compact tracking state
    async fn apply_session_memory_summary(
        &mut self,
        summary: SessionMemorySummary,
        turn_id: &str,
        tracking: &mut AutoCompactTracking,
    ) -> crate::error::Result<()> {
        let tokens_before = self.message_history.estimate_tokens();

        info!(
            summary_tokens = summary.token_estimate,
            last_id = ?summary.last_summarized_id,
            "Applying session memory summary (Tier 1)"
        );

        // Get transcript path from context if available
        let transcript_path = self.context.transcript_path.clone();

        // Calculate keep window using token-based algorithm
        let messages_json = self.message_history.messages_for_api_json();
        let keep_result =
            calculate_keep_start_index(&messages_json, &self.compact_config.keep_window);
        let keep_turns = map_message_index_to_keep_turns(
            self.message_history.turn_count(),
            &messages_json,
            keep_result.keep_start_index,
        );
        let tokens_saved = (tokens_before - summary.token_estimate).max(0);

        debug!(
            keep_turns,
            keep_start_index = keep_result.keep_start_index,
            messages_to_keep = keep_result.messages_to_keep,
            keep_tokens = keep_result.keep_tokens,
            "Calculated keep window for session memory compact"
        );

        self.message_history.apply_compaction_with_metadata(
            summary.summary.clone(),
            keep_turns,
            turn_id,
            tokens_saved,
            cocode_protocol::CompactTrigger::Auto,
            tokens_before,
            transcript_path,
            true, // Recent messages preserved
        );

        // Update tracking
        tracking.mark_compacted(turn_id, self.turn_number);

        // Calculate post-compaction tokens and update boundary
        let post_tokens = self.message_history.estimate_tokens();
        self.message_history
            .update_boundary_post_tokens(post_tokens);

        // Emit events
        self.emit(LoopEvent::SessionMemoryCompactApplied {
            saved_tokens: tokens_saved,
            summary_tokens: summary.token_estimate,
        })
        .await;

        // Emit compact boundary inserted event
        self.emit(LoopEvent::CompactBoundaryInserted {
            trigger: cocode_protocol::CompactTrigger::Auto,
            pre_tokens: tokens_before,
            post_tokens,
        })
        .await;

        // Keep tracker consistent after session memory compact (Claude Code alignment)
        // Without this, tracker state becomes stale and large compacted files are never
        // notified to the model after Tier 1 compaction.
        let file_config = &self.compact_config.file_restoration;
        let files_for_restoration = self.collect_restorable_tracked_files(file_config).await;
        let restoration = build_context_restoration_with_config(
            files_for_restoration,
            None,
            None,
            Vec::new(),
            file_config,
        );
        self.pending_compacted_large_files = restoration.compacted_large_files;
        if !restoration.files.is_empty() {
            self.rebuild_trackers_from_restored_files(&restoration.files)
                .await;
        }

        Ok(())
    }

    /// Add tool results to the message history and apply context modifiers.
    ///
    /// This creates proper tool_result messages that link back to the tool_use
    /// blocks via their call_id. The results are added to the current turn
    /// for tracking, and a new turn with tool result messages is created
    /// for the next API call.
    ///
    /// Context modifiers from tool outputs are applied to update:
    /// - `FileTracker`: Records file reads with content and timestamps
    /// - `ApprovalStore`: Records permission grants for future operations
    /// - Queued commands (logged but not yet executed)
    async fn add_tool_results_to_history(
        &mut self,
        results: &[ToolExecutionResult],
        _tool_calls: &[ToolCall],
    ) {
        if results.is_empty() {
            return;
        }

        // Collect all modifiers from successful tool executions
        let mut all_modifiers: Vec<ContextModifier> = Vec::new();

        // Add tool results to current turn for tracking
        for result in results {
            let (output, is_error) = match &result.result {
                Ok(output) => {
                    // Collect modifiers from successful executions
                    all_modifiers.extend(output.modifiers.clone());
                    (output.content.clone(), output.is_error)
                }
                Err(e) => (ToolResultContent::Text(e.to_string()), true),
            };
            self.message_history
                .add_tool_result(&result.call_id, &result.name, output, is_error);
        }

        // Apply context modifiers
        if !all_modifiers.is_empty() {
            self.apply_modifiers(&all_modifiers).await;
        }

        // Create a new turn with tool result messages for the next API call
        // Using TrackedMessage::tool_result for proper role assignment
        let next_turn_id = uuid::Uuid::new_v4().to_string();

        // Build tool result content blocks for the user message
        // (Some providers expect tool results as user messages with special content)
        let tool_results_text: String = results
            .iter()
            .map(|r| {
                let output_text = match &r.result {
                    Ok(output) => match &output.content {
                        ToolResultContent::Text(t) => t.clone(),
                        ToolResultContent::Structured(v) => v.to_string(),
                    },
                    Err(e) => format!("Tool error: {e}"),
                };
                format!(
                    "<tool_result tool_use_id=\"{}\" name=\"{}\">\n{}\n</tool_result>",
                    r.call_id, r.name, output_text
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // Collect images from tool results
        let all_images: Vec<&cocode_protocol::ImageData> = results
            .iter()
            .filter_map(|r| r.result.as_ref().ok())
            .flat_map(|output| &output.images)
            .collect();

        // Create a user message containing the tool results (and images if any)
        // This will be normalized by MessageHistory::messages_for_api() to the correct format
        let user_msg = if all_images.is_empty() {
            TrackedMessage::user(&tool_results_text, &next_turn_id)
        } else {
            let mut content_blocks = vec![ContentBlock::text(&tool_results_text)];
            for img in &all_images {
                content_blocks.push(ContentBlock::Image {
                    source: hyper_sdk::ImageSource::Base64 {
                        data: img.data.clone(),
                        media_type: img.media_type.clone(),
                    },
                    detail: None,
                });
            }
            let message = Message::new(hyper_sdk::Role::User, content_blocks);
            TrackedMessage::new(message, &next_turn_id, cocode_message::MessageSource::User)
        };
        let turn = Turn::new(self.turn_number + 1, user_msg);
        self.message_history.add_turn(turn);
    }

    /// Apply context modifiers from tool execution results.
    ///
    /// This processes modifiers collected from tool outputs and updates the
    /// appropriate stores:
    /// - `FileRead`: Updates the FileTracker with file content and timestamps
    /// - `PermissionGranted`: Updates the ApprovalStore with granted permissions
    async fn apply_modifiers(&mut self, modifiers: &[ContextModifier]) {
        for modifier in modifiers {
            match modifier {
                ContextModifier::FileRead {
                    path,
                    content,
                    file_mtime_ms,
                    offset,
                    limit,
                    read_kind,
                } => {
                    // Update the shared file tracker with the file read state
                    let tracker = self.shared_tools_file_tracker.lock().await;
                    // Convert mtime from ms if provided, otherwise get from filesystem
                    let file_mtime = if let Some(ms) = file_mtime_ms {
                        std::time::UNIX_EPOCH
                            .checked_add(std::time::Duration::from_millis(*ms as u64))
                    } else {
                        tokio::fs::metadata(path)
                            .await
                            .ok()
                            .and_then(|m| m.modified().ok())
                    };
                    let state = match read_kind {
                        cocode_protocol::FileReadKind::FullContent => {
                            FileReadState::complete_with_turn(
                                content.clone(),
                                file_mtime,
                                self.turn_number,
                            )
                        }
                        cocode_protocol::FileReadKind::PartialContent => {
                            FileReadState::partial_with_turn(
                                offset.unwrap_or(0),
                                limit.unwrap_or(0),
                                file_mtime,
                                self.turn_number,
                            )
                        }
                        cocode_protocol::FileReadKind::MetadataOnly => {
                            // For metadata-only, we just record that the file was touched
                            FileReadState {
                                content: None,
                                timestamp: std::time::SystemTime::now(),
                                file_mtime,
                                content_hash: None,
                                offset: None,
                                limit: None,
                                kind: cocode_protocol::FileReadKind::MetadataOnly,
                                access_count: 1,
                                read_turn: self.turn_number,
                            }
                        }
                    };
                    tracker.track_read(path.clone(), state);
                    debug!(
                        path = %path.display(),
                        content_len = content.len(),
                        read_kind = ?read_kind,
                        "Applied FileRead modifier"
                    );
                }
                ContextModifier::PermissionGranted { tool, pattern } => {
                    // Update the shared approval store with the granted permission
                    let mut store = self.shared_approval_store.lock().await;
                    store.approve_pattern(tool, pattern);
                    debug!(
                        tool = %tool,
                        pattern = %pattern,
                        "Applied PermissionGranted modifier"
                    );
                }
                ContextModifier::SkillAllowedTools {
                    skill_name,
                    allowed_tools,
                } => {
                    // Set skill-level tool restrictions for subsequent tool execution.
                    // Always include "Skill" itself so nested skill invocations work.
                    let mut allowed: std::collections::HashSet<String> =
                        allowed_tools.iter().cloned().collect();
                    allowed.insert(cocode_protocol::ToolName::Skill.as_str().to_string());
                    self.active_skill_allowed_tools = Some(allowed);
                    debug!(
                        skill = %skill_name,
                        tools = ?allowed_tools,
                        "Applied SkillAllowedTools modifier"
                    );
                }
                ContextModifier::TodosUpdated { todos } => {
                    self.current_todos = Some(todos.clone());
                    debug!(
                        count = todos.as_array().map_or(0, std::vec::Vec::len),
                        "Applied TodosUpdated modifier"
                    );
                }
            }
        }
    }

    /// Emit a loop event to the event channel.
    async fn emit(&self, event: LoopEvent) {
        if let Err(e) = self.event_tx.send(event).await {
            debug!("Failed to send loop event: {e}");
        }
    }

    /// Returns the current turn number.
    pub fn turn_number(&self) -> i32 {
        self.turn_number
    }

    /// Returns the total input tokens consumed.
    pub fn total_input_tokens(&self) -> i32 {
        self.total_input_tokens
    }

    /// Returns the total output tokens generated.
    pub fn total_output_tokens(&self) -> i32 {
        self.total_output_tokens
    }

    /// Returns a reference to the message history.
    pub fn message_history(&self) -> &MessageHistory {
        &self.message_history
    }

    /// Returns a mutable reference to the message history.
    pub fn message_history_mut(&mut self) -> &mut MessageHistory {
        &mut self.message_history
    }

    /// Returns the snapshot manager (if configured).
    pub fn snapshot_manager(&self) -> Option<&Arc<cocode_file_backup::SnapshotManager>> {
        self.snapshot_manager.as_ref()
    }

    /// Returns a reference to the loop configuration.
    pub fn config(&self) -> &LoopConfig {
        &self.config
    }

    /// Returns the cancellation token.
    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel_token
    }
}

/// Filter tool definitions based on model capabilities.
///
/// This ensures each model only sees tools it supports:
/// - `shell_type`: `Disabled` removes shell-related tools (`Bash`, `shell`, `TaskOutput`, `TaskStop`)
/// - `apply_patch`: controlled by `ModelInfo.apply_patch_tool_type`
/// - `excluded_tools`: blacklist filter removing named tools
/// - experimental tools: controlled by `ModelInfo.experimental_supported_tools`
///
/// Feature-gated tools are already filtered by `ToolRegistry::definitions_filtered()`.
fn select_tools_for_model(
    mut defs: Vec<ToolDefinition>,
    model_info: &cocode_protocol::ModelInfo,
) -> Vec<ToolDefinition> {
    use cocode_protocol::ApplyPatchToolType;
    use cocode_protocol::ConfigShellToolType;
    use cocode_tools::builtin::ApplyPatchTool;

    // 1. Handle shell_type
    match model_info.shell_type {
        Some(ConfigShellToolType::Disabled) => {
            use cocode_protocol::ToolName;
            defs.retain(|d| {
                let name = d.name.as_str();
                name != ToolName::Bash.as_str()
                    && name != ToolName::Shell.as_str()
                    && name != ToolName::TaskOutput.as_str()
                    && name != ToolName::TaskStop.as_str()
            });
        }
        Some(ConfigShellToolType::Shell) => {
            // Shell mode: remove Bash, keep shell tool
            defs.retain(|d| d.name != cocode_protocol::ToolName::Bash.as_str());
        }
        Some(ConfigShellToolType::ShellCommand) | None => {
            // ShellCommand (default): remove shell tool, keep Bash
            defs.retain(|d| d.name != cocode_protocol::ToolName::Shell.as_str());
        }
    }

    // 2. Handle apply_patch: remove registry default, add model-specific variant
    defs.retain(|d| d.name != cocode_protocol::ToolName::ApplyPatch.as_str());
    match model_info.apply_patch_tool_type {
        Some(ApplyPatchToolType::Function) => {
            defs.push(ApplyPatchTool::function_definition());
        }
        Some(ApplyPatchToolType::Freeform) => {
            defs.push(ApplyPatchTool::freeform_definition());
        }
        Some(ApplyPatchToolType::Shell) | None => {
            // Shell: prompt handles it; None: no apply_patch at all
        }
    }

    // 3. Handle excluded_tools (blacklist filter)
    if let Some(ref excluded) = model_info.excluded_tools
        && !excluded.is_empty()
    {
        defs.retain(|d| !excluded.contains(&d.name));
    }

    // 4. Handle experimental_supported_tools (whitelist filter)
    if let Some(ref supported) = model_info.experimental_supported_tools
        && !supported.is_empty()
    {
        defs.retain(|d| supported.contains(&d.name));
    }

    defs
}

#[cfg(test)]
#[path = "driver.test.rs"]
mod tests;
