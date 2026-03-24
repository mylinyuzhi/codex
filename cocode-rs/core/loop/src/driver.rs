//! Agent loop driver - the core 18-step conversation loop.

mod builder;
mod compact_micro;
mod compact_orchestrator;
mod compact_restore;
mod file_tracking;
mod hooks_bridge;
mod streaming;
mod tool_results;

pub use builder::AgentLoopBuilder;
use streaming::format_language_model_message;

use std::sync::Arc;
use std::time::Instant;

use cocode_api::ApiClient;
use cocode_api::AssistantContentPart;
use cocode_api::ModelHub;
use cocode_api::TextPart;
use cocode_api::ToolCall;
use cocode_api::ToolCallPart;
use cocode_api::UnifiedFinishReason;
use cocode_context::ConversationContext;
use cocode_hooks::AsyncHookTracker;
use cocode_hooks::HookRegistry;
use cocode_message::MessageHistory;
use cocode_message::TrackedMessage;
use cocode_message::Turn;
use cocode_policy::ApprovalStore;
use cocode_protocol::AgentStatus;
use cocode_protocol::AutoCompactTracking;
use cocode_protocol::CompactConfig;
use cocode_protocol::LoopConfig;
use cocode_protocol::LoopEvent;
use cocode_protocol::QueryTracking;
use cocode_protocol::RoleSelections;
use cocode_protocol::ToolResultContent;
use cocode_skill::SkillManager;
use cocode_system_reminder::ApprovedPlanInfo;
use cocode_system_reminder::AsyncHookResponseInfo;
use cocode_system_reminder::BackgroundTaskInfo;
use cocode_system_reminder::BackgroundTaskStatus;
use cocode_system_reminder::BackgroundTaskType;
use cocode_system_reminder::GeneratorContext;
use cocode_system_reminder::HookBlockingInfo;
use cocode_system_reminder::HookContextInfo;
use cocode_system_reminder::HookState;
use cocode_system_reminder::InjectedMessage;
use cocode_system_reminder::InvokedSkillInfo;
use cocode_system_reminder::MentionReadRecord;
use cocode_system_reminder::QueuedCommandInfo;
use cocode_system_reminder::SkillInfo;
use cocode_system_reminder::StructuredTaskInfo;
use cocode_system_reminder::SystemReminderOrchestrator;
use cocode_system_reminder::create_injected_messages;
use cocode_system_reminder::generator::DiagnosticInfo;
use cocode_system_reminder::generator::TeamContextData;
use cocode_system_reminder::generator::TeamMemberInfo;
use cocode_system_reminder::generator::UnreadMessage;
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
use std::sync::Mutex;

use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::compaction::ThresholdStatus;
use crate::compaction::try_session_memory_compact;
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
    /// Compact configuration with all threshold constants and session memory settings.
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
    /// Channel for receiving extraction outcomes from background tasks.
    extraction_result_rx: mpsc::Receiver<crate::session_memory_agent::ExtractionOutcome>,
    /// Sender cloned into each background extraction task.
    extraction_result_tx: mpsc::Sender<crate::session_memory_agent::ExtractionOutcome>,

    // Circuit breaker for auto-compaction
    /// Consecutive compaction failure count. Reset to 0 on success.
    compact_failure_count: i32,
    /// When true, auto-compaction is disabled (manual still works).
    /// Trips after 3 consecutive failures.
    circuit_breaker_open: bool,

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
    /// Optional suffix appended to the generated system prompt.
    ///
    /// Used for `critical_reminder` enforcement at system prompt level
    /// (highest authority). Ignored when `custom_system_prompt` is set.
    system_prompt_suffix: Option<String>,
    /// Whether the current turn has user input.
    /// When false, UserPrompt tier reminders are skipped.
    current_turn_has_user_input: bool,

    // Plan mode tracking
    /// Plan mode state for the session.
    plan_mode_state: PlanModeState,

    // Auto memory
    /// Auto memory state for the session.
    auto_memory_state: Option<Arc<cocode_auto_memory::AutoMemoryState>>,

    // Team collaboration
    /// Team store for querying team membership each turn.
    team_store: Option<Arc<cocode_team::TeamStore>>,
    /// Team mailbox for querying unread messages each turn.
    team_mailbox: Option<Arc<cocode_team::Mailbox>>,

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
    ///
    /// **Lifecycle invariant**: set via `SkillAllowedTools` context modifier
    /// (in `apply_modifiers`), persists across `ToolCalls` recursion, and
    /// cleared on `Stop`, `Length`, or any other finish reason. A new skill
    /// invocation replaces the previous restriction.
    active_skill_allowed_tools: Option<std::collections::HashSet<String>>,
    /// Model override requested by a skill via `ContextModifier::ModelOverride`.
    /// Applied before the next API call and then cleared.
    model_override: Option<String>,

    // Task list state (updated by TodoWrite tool via ContextModifier)
    /// Latest task list from the most recent TodoWrite tool call.
    current_todos: Option<serde_json::Value>,

    // Structured task state (updated by TaskCreate/TaskUpdate via ContextModifier)
    /// Latest structured tasks snapshot.
    current_structured_tasks: Option<serde_json::Value>,

    // Cron job state (updated by CronCreate/CronDelete via ContextModifier)
    /// Latest cron jobs snapshot.
    current_cron_jobs: Option<serde_json::Value>,

    // Delegate mode (updated by ContextModifier::DelegateModeChanged)
    /// Whether the main agent is in delegate mode (coordination-only tools).
    delegate_mode: bool,

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
    permission_rules: Vec<cocode_policy::PermissionRule>,

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

    /// Path to the cocode home directory for durable cron persistence.
    cocode_home: Option<std::path::PathBuf>,

    /// Shared set of agent IDs killed via TaskStop (persists across turns).
    killed_agents: cocode_tools::context::KilledAgents,

    /// Per-turn background agent task info pushed by the session layer.
    ///
    /// Set via [`set_background_agent_tasks`] before each turn so that
    /// `generate_system_reminders()` can populate the `background_tasks`
    /// field in `GeneratorContext`. Combined with shell background tasks
    /// collected directly from the registry, this closes the feedback loop
    /// between the subagent manager and the unified tasks system reminder.
    background_agent_tasks: Vec<cocode_system_reminder::BackgroundTaskInfo>,
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
        self.queued_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(cmd);
    }

    /// Drain all queued commands.
    pub fn take_queued_commands(&self) -> Vec<QueuedCommandInfo> {
        std::mem::take(
            &mut *self
                .queued_commands
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Get the number of queued commands.
    pub fn queued_count(&self) -> usize {
        self.queued_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
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

    /// Take the current structured tasks (if any) set by TaskCreate/TaskUpdate.
    pub fn take_structured_tasks(&mut self) -> Option<serde_json::Value> {
        self.current_structured_tasks.take()
    }

    /// Take the current cron jobs (if any) set by CronCreate/CronDelete.
    pub fn take_cron_jobs(&mut self) -> Option<serde_json::Value> {
        self.current_cron_jobs.take()
    }

    /// Set background agent task info for the next turn's system reminders.
    ///
    /// Called by the session/executor layer before each turn with the latest
    /// agent snapshot from `SubagentManager::agent_infos()`. These are
    /// combined with shell background tasks from the registry to populate
    /// the `background_tasks` field in `GeneratorContext`.
    pub fn set_background_agent_tasks(
        &mut self,
        tasks: Vec<cocode_system_reminder::BackgroundTaskInfo>,
    ) {
        self.background_agent_tasks = tasks;
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
            if let Ok(ref loop_result) = result
                && !loop_result.final_text.is_empty()
            {
                ctx = ctx.with_last_assistant_message(&loop_result.final_text);
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
                if let Ok(ref lr) = re_result
                    && !lr.final_text.is_empty()
                {
                    ctx2 = ctx2.with_last_assistant_message(&lr.final_text);
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

    /// The 18-step core message loop.
    ///
    /// This implements the algorithm from `docs/arch/core-loop.md`:
    ///
    /// - SETUP (1-6): emit events, query tracking, normalize, micro-compact,
    ///   auto-compact, init state.
    /// - EXECUTION (7-10): resolve model, check token limit, stream with tools
    ///   + retry, record telemetry.
    /// - POST-PROCESSING (11-18): check tool calls, execute queue, abort handling,
    ///   hooks, tracking, queued commands, max turns, recurse.
    async fn core_message_loop(
        &mut self,
        query_tracking: &mut QueryTracking,
        auto_compact_tracking: &mut AutoCompactTracking,
    ) -> crate::error::Result<LoopResult> {
        // ── STEP 0.5: Refresh auto memory from disk (always fresh) ──
        if let Some(ref state) = self.auto_memory_state {
            state.refresh().await;
        }

        // ── STEP 1: Signal stream_request_start ──
        self.emit(LoopEvent::StreamRequestStart).await;

        // ── STEP 2: Setup query tracking ──
        query_tracking.depth += 1;
        let turn_id = uuid::Uuid::new_v4().to_string();

        // ── STEP 3: Normalize messages ──
        // Messages are already normalized through MessageHistory::messages_for_api().

        // ── STEP 3.5: Drain extraction outcomes from background tasks ──
        // Fixes bug where extraction_in_progress stayed true forever because
        // the background tokio::spawn couldn't update auto_compact_tracking.
        while let Ok(outcome) = self.extraction_result_rx.try_recv() {
            match outcome {
                crate::session_memory_agent::ExtractionOutcome::Completed {
                    summary_tokens,
                    last_summarized_id,
                } => {
                    auto_compact_tracking
                        .mark_extraction_completed(summary_tokens, &last_summarized_id);
                    debug!(
                        summary_tokens,
                        last_summarized_id, "Extraction outcome received: completed"
                    );
                }
                crate::session_memory_agent::ExtractionOutcome::Failed => {
                    auto_compact_tracking.mark_extraction_failed();
                    debug!("Extraction outcome received: failed");
                }
            }
        }

        // ── STEP 4: Micro-compaction (PRE-API) ──
        // NOTE: Deliberate divergence from Claude Code v2.1.76, where `performMicrocompaction`
        // is a no-op (returns messages unchanged). cocode-rs has micro-compact active by
        // default because it operates at the MessageHistory level rather than raw JSON,
        // making tool-result trimming safe and effective.
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
        // Skip if circuit breaker is open (3+ consecutive failures)
        if status.is_above_auto_compact_threshold
            && self.compact_config.is_auto_compact_enabled()
            && !self.circuit_breaker_open
        {
            // Tier 1: Try session memory first (zero API cost)
            // Only if session memory compact is enabled
            let mut needs_llm_compact = true;
            if self.compact_config.enable_sm_compact
                && let Some(summary) = try_session_memory_compact(&self.compact_config)
            {
                self.apply_session_memory_summary(summary, &turn_id, auto_compact_tracking)
                    .await?;

                // Post-compact validation: check if session memory compact was sufficient.
                // If post-compact tokens still exceed auto-compact target, fall through
                // to Tier 2 (LLM-based compaction).
                let post_tokens = self.message_history.estimate_tokens();
                let post_estimated = self.compact_config.estimate_tokens_with_margin(post_tokens);
                let target = self.compact_config.auto_compact_target(context_window);
                if post_estimated < target {
                    needs_llm_compact = false;
                } else {
                    warn!(
                        post_tokens = post_estimated,
                        target,
                        "Session memory compact insufficient, falling through to LLM compact"
                    );
                }
            }
            if needs_llm_compact {
                self.compact(auto_compact_tracking, &turn_id, query_tracking)
                    .await?;
            }
        }

        // Recalculate threshold status after auto-compact.
        // The status from Step 5 is stale if Tier 1 or Tier 2 compact ran,
        // which could cause a false-positive blocking limit error at Step 8.
        let estimated_tokens = self.message_history.estimate_tokens();
        let estimated_with_margin = self
            .compact_config
            .estimate_tokens_with_margin(estimated_tokens);
        let status =
            ThresholdStatus::calculate(estimated_with_margin, context_window, &self.compact_config);

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
        let injected_messages = self.generate_system_reminders().await;

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
        let executor = self.build_turn_executor(&turn_id, query_tracking);

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
                AssistantContentPart::Text(TextPart { text, .. }) => Some(text.as_str()),
                _ => None,
            })
            .collect();

        // Check for tool calls
        let has_tool_calls = collected
            .content
            .iter()
            .any(|b| matches!(b, AssistantContentPart::ToolCall(_)));

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
                    AssistantContentPart::ToolCall(ToolCallPart {
                        tool_call_id,
                        tool_name,
                        input,
                        ..
                    }) => Some(ToolCall::new(tool_call_id, tool_name, input.clone())),
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
            if let Some(result) =
                self.handle_plan_mode_transitions(&tool_calls, &results, &collected.content)
            {
                return Ok(result);
            }

            // Track tool calls for extraction triggering
            for _ in &tool_calls {
                auto_compact_tracking.record_tool_call();
            }

            // ── STEP 14: Check for hook stop ──
            // If any PostToolUse hook returned `preventContinuation`, halt the loop
            // after processing this turn's tool results. The tool output itself is
            // preserved — only the loop continuation is suppressed.
            if let Some(reason) = results.iter().find_map(|r| r.stop_continuation.as_deref()) {
                info!(
                    turn = self.turn_number,
                    reason = %reason,
                    "PostToolUse hook requested loop stop (preventContinuation)"
                );
                self.set_status(AgentStatus::Idle);
                return Ok(LoopResult::hook_stopped(
                    self.turn_number,
                    self.total_input_tokens,
                    self.total_output_tokens,
                ));
            }
        }

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
                    .map(format_language_model_message)
                    .collect::<Vec<_>>()
                    .join("\n\n");

                let current_tokens = estimated_tokens;
                let tool_calls_since = auto_compact_tracking.tool_calls_since_extraction();
                let last_message_id = turn_id.clone();
                let message_count = messages.len() as i32;

                // Mark extraction as started
                auto_compact_tracking.mark_extraction_started();

                // Clone what we need for the background task
                let agent = Arc::clone(extraction_agent);
                let tracking_current_tokens = current_tokens;
                let outcome_tx = self.extraction_result_tx.clone();

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
                            let _ = outcome_tx
                                .send(crate::session_memory_agent::ExtractionOutcome::Completed {
                                    summary_tokens: result.summary_tokens,
                                    last_summarized_id: result.last_summarized_id,
                                })
                                .await;
                        }
                        Err(e) => {
                            warn!(error = %e, "Background extraction failed");
                            let _ = outcome_tx
                                .send(crate::session_memory_agent::ExtractionOutcome::Failed)
                                .await;
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
        match collected.finish_reason.unified {
            UnifiedFinishReason::Stop => {
                // Clear skill-level tool restrictions — the model has finished
                // processing the skill's instructions (no more tool calls).
                // Matches CC: contextModifier scope ends when the model stops.
                self.active_skill_allowed_tools = None;

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
            UnifiedFinishReason::ToolCalls => {
                // Tool call turns don't have fresh user input - only tool results
                self.current_turn_has_user_input = false;
                // Recursive call for next turn (boxed to avoid infinite future size)
                Box::pin(self.core_message_loop(query_tracking, auto_compact_tracking)).await
            }
            UnifiedFinishReason::Length => {
                // Clear skill-level tool restrictions — the model hit a token
                // limit mid-skill, so the skill context is effectively lost.
                self.active_skill_allowed_tools = None;
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
                self.active_skill_allowed_tools = None;
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

    /// Generate system reminders for the current turn (Step 6.5).
    ///
    /// Collects async hook results, builds the generator context, generates
    /// all system reminders, and applies side effects (mention reads,
    /// rewind restoration, one-shot flag consumption).
    async fn generate_system_reminders(&mut self) -> Vec<InjectedMessage> {
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
            .map(|turn| turn.user_message.text());

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

        // Collect background tasks for system reminder feedback loop:
        // 1. Shell background tasks from the registry
        // 2. Agent background tasks pushed by the session layer
        let background_tasks = {
            let mut tasks: Vec<BackgroundTaskInfo> = Vec::new();

            // Shell background tasks
            for snapshot in self.shell_executor.background_registry.list_tasks().await {
                let status = if snapshot.is_running {
                    BackgroundTaskStatus::Running
                } else {
                    BackgroundTaskStatus::Completed
                };
                tasks.push(BackgroundTaskInfo {
                    task_id: snapshot.id,
                    task_type: BackgroundTaskType::Shell,
                    command: snapshot.command,
                    status,
                    exit_code: None,
                    has_new_output: false,
                    progress_message: None,
                    is_completion_notification: false,
                    delta_summary: None,
                    description: None,
                });
            }

            // Agent background tasks (pushed by session layer)
            tasks.append(&mut self.background_agent_tasks);

            tasks
        };

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
                })
                .is_auto_compact_enabled(self.compact_config.is_auto_compact_enabled());

            // Wire auto memory state into generator context
            if let Some(ref state) = self.auto_memory_state {
                builder = builder.auto_memory_state(Arc::clone(state));
            }

            // Wire team context and unread messages into generator context
            if let (Some(store), Some(mbox)) = (&self.team_store, &self.team_mailbox)
                && let Some(identity) = cocode_subagent::current_agent()
                && let Some(ref team_name) = identity.team_name
            {
                // Query team membership
                if let Some(team) = store.get_team(team_name).await {
                    builder = builder.team_context(TeamContextData {
                        agent_id: identity.agent_id.clone(),
                        agent_name: identity.name.clone(),
                        team_name: team_name.clone(),
                        agent_type: identity.agent_type.clone(),
                        members: team
                            .members
                            .iter()
                            .map(|m| TeamMemberInfo {
                                agent_id: m.agent_id.clone(),
                                name: m.name.clone(),
                                agent_type: m.agent_type.clone(),
                                status: m.status.as_str().to_string(),
                            })
                            .collect(),
                    });
                }

                // Query unread messages and mark as read in a single file operation
                if let Ok(messages) = mbox.take_unread(team_name, &identity.agent_id).await
                    && !messages.is_empty()
                {
                    builder = builder.unread_messages(
                        messages
                            .into_iter()
                            .map(|m| UnreadMessage {
                                id: m.id,
                                from: m.from,
                                content: m.content,
                                message_type: m.message_type.as_str().to_string(),
                                timestamp: m.timestamp,
                            })
                            .collect(),
                    );
                }
            }

            // Wire background tasks into the generator context
            if !background_tasks.is_empty() {
                builder = builder.background_tasks(background_tasks);
            }

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
            if needs_plan_reference
                && let Some(ref plan_path) = plan_file_path
                && let Ok(content) = std::fs::read_to_string(plan_path)
            {
                builder = builder.restored_plan(cocode_system_reminder::RestoredPlanInfo {
                    content,
                    file_path: plan_path.clone(),
                });
            }

            // Inject approved plan content after ExitPlanMode (one-shot)
            if needs_exit_attachment {
                builder = builder.plan_mode_exit_pending(true);
                if let Some(ref plan_path) = plan_file_path
                    && let Ok(content) = std::fs::read_to_string(plan_path)
                {
                    builder = builder.approved_plan(ApprovedPlanInfo {
                        content,
                        approved_turn: exited_at_turn.unwrap_or(turn_number),
                    });
                }
            }

            // Add available skills to generator context
            if let Some(ref sm) = self.skill_manager {
                let mut skill_infos: Vec<SkillInfo> = sm
                    .llm_invocable_skills()
                    .into_iter()
                    .map(|skill| {
                        let plugin_name = match &skill.source {
                            cocode_skill::SkillSource::Plugin { plugin_name } => {
                                Some(plugin_name.clone())
                            }
                            _ => None,
                        };
                        SkillInfo {
                            name: skill.name.clone(),
                            description: skill.description.clone(),
                            when_to_use: skill.when_to_use.clone(),
                            is_bundled: skill.loaded_from == cocode_skill::LoadedFrom::Bundled,
                            plugin_name,
                        }
                    })
                    .collect();

                // Activate conditional skills matching files touched this session
                let touched_paths = reminder_tracker_view.tracked_files();
                if !touched_paths.is_empty() {
                    for skill in sm.activate_for_paths(&touched_paths) {
                        let plugin_name = match &skill.source {
                            cocode_skill::SkillSource::Plugin { plugin_name } => {
                                Some(plugin_name.clone())
                            }
                            _ => None,
                        };
                        skill_infos.push(SkillInfo {
                            name: skill.name.clone(),
                            description: skill.description.clone(),
                            when_to_use: skill.when_to_use.clone(),
                            is_bundled: false,
                            plugin_name,
                        });
                    }
                }

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
                            prompt_content: skill.prompt_content.clone(),
                        })
                        .collect();
                    builder = builder.invoked_skills(skill_infos);
                }
            }

            // Consume queued commands for steering injection
            {
                let drained = std::mem::take(
                    &mut *self
                        .queued_commands
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                );
                if !drained.is_empty() {
                    builder = builder.queued_commands(drained);
                }
            }

            // Get rewind info if available (already extracted earlier)
            if let Some(rewind_info) = rewind_context_for_builder.clone() {
                builder = builder.rewind_info(rewind_info);
            }

            // Convert structured tasks to StructuredTaskInfo for rich reminders.
            // Falls back to plain TodoItems from TodoWrite for backwards compatibility.
            {
                let mut has_structured = false;

                if let Some(ref tasks_val) = self.current_structured_tasks
                    && let Some(tasks_map) = tasks_val.as_object()
                {
                    let mut structured_infos = Vec::new();
                    for (_id, task) in tasks_map {
                        let status_str = task["status"].as_str().unwrap_or("pending").to_string();
                        // Skip deleted tasks
                        if status_str == "deleted" {
                            continue;
                        }
                        let blocked_by: Vec<String> = task["blocked_by"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|b| b.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let is_blocked = blocked_by.iter().any(|bid| {
                            tasks_map
                                .get(bid)
                                .is_some_and(|bt| bt["status"].as_str() != Some("completed"))
                        });
                        let blocks: Vec<String> = task["blocks"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|b| b.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        structured_infos.push(StructuredTaskInfo {
                            id: task["id"].as_str().unwrap_or("?").to_string(),
                            subject: task["subject"].as_str().unwrap_or("?").to_string(),
                            description: task["description"].as_str().map(String::from),
                            status: status_str,
                            active_form: task["active_form"].as_str().map(String::from),
                            owner: task["owner"].as_str().map(String::from),
                            blocks,
                            blocked_by,
                            is_blocked,
                        });
                    }
                    if !structured_infos.is_empty() {
                        builder = builder.structured_tasks(structured_infos);
                        has_structured = true;
                    }
                }

                // Plain todos (from TodoWrite) — only when structured tasks are absent
                if !has_structured
                    && let Some(ref todos_val) = self.current_todos
                    && let Some(arr) = todos_val.as_array()
                {
                    let mut todo_items = Vec::new();
                    for todo in arr {
                        let status_str = todo["status"].as_str().unwrap_or("pending");
                        let status = match status_str {
                            "in_progress" => {
                                cocode_system_reminder::generator::TodoStatus::InProgress
                            }
                            "completed" => cocode_system_reminder::generator::TodoStatus::Completed,
                            _ => cocode_system_reminder::generator::TodoStatus::Pending,
                        };
                        todo_items.push(cocode_system_reminder::generator::TodoItem {
                            id: todo["id"].as_str().unwrap_or("?").to_string(),
                            subject: todo["subject"]
                                .as_str()
                                .or_else(|| todo["content"].as_str())
                                .unwrap_or("?")
                                .to_string(),
                            status,
                            is_blocked: false,
                        });
                    }
                    if !todo_items.is_empty() {
                        builder = builder.todos(todo_items);
                    }
                }
            }

            // Convert cron jobs to CronJobInfo for the reminder system.
            {
                if let Some(ref jobs_val) = self.current_cron_jobs
                    && let Some(jobs_map) = jobs_val.as_object()
                {
                    let cron_infos: Vec<cocode_system_reminder::CronJobInfo> = jobs_map
                        .values()
                        .map(|job| cocode_system_reminder::CronJobInfo {
                            id: job["id"].as_str().unwrap_or("?").to_string(),
                            cron: job["cron"].as_str().unwrap_or("?").to_string(),
                            description: job["description"]
                                .as_str()
                                .unwrap_or_else(|| job["prompt"].as_str().unwrap_or("?"))
                                .chars()
                                .take(80)
                                .collect(),
                            one_shot: !job["recurring"].as_bool().unwrap_or(true),
                            execution_count: job["execution_count"].as_i64().unwrap_or(0) as u32,
                        })
                        .collect();
                    if !cron_infos.is_empty() {
                        builder = builder.cron_jobs(cron_infos);
                    }
                }
            }

            // Collect dirty LSP diagnostics for system reminder injection
            if let Some(ref lsp) = self.lsp_manager {
                let dirty = lsp.diagnostics().take_dirty().await;
                if !dirty.is_empty() {
                    let diag_infos: Vec<DiagnosticInfo> = dirty
                        .into_iter()
                        .map(|d| DiagnosticInfo {
                            file_path: d.file,
                            line: d.line,
                            column: d.character,
                            severity: d.severity.as_str().to_string(),
                            message: d.message,
                            code: d.code,
                            source: d.source,
                        })
                        .collect();
                    builder = builder.diagnostics(diag_infos);
                }
            }

            let gen_ctx = builder.build();
            let reminders = self.reminder_orchestrator.generate_all(gen_ctx).await;

            // Emit SystemReminderDisplay for silent reminders (UI notification only)
            for reminder in &reminders {
                if reminder.is_silent
                    && let Some(ref metadata) = reminder.metadata
                {
                    self.emit(LoopEvent::SystemReminderDisplay {
                        reminder_type: reminder.attachment_type.name().to_string(),
                        payload: serde_json::to_value(metadata).unwrap_or_default(),
                    })
                    .await;
                }
            }

            create_injected_messages(reminders)
        };

        // Drain mention_read_records and apply to shared FileTracker
        // This bridges @mention reads back to the canonical tracker
        {
            let records: Vec<MentionReadRecord> = std::mem::take(
                &mut *mention_read_records
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            self.apply_mention_read_records(&records).await;
        }

        // Consume one-shot flags after generating reminders
        if needs_plan_reference {
            self.plan_mode_state.needs_plan_reference = false;
        }
        if needs_exit_attachment {
            self.plan_mode_state.clear_exit_attachment();
        }

        injected_messages
    }

    /// Build the `StreamingToolExecutor` for the current turn.
    ///
    /// Configures the executor with all tool system components: permissions,
    /// hooks, file tracking, shell executor, model call function, skills,
    /// LSP, and subagent spawning.
    fn build_turn_executor(
        &self,
        turn_id: &str,
        query_tracking: &QueryTracking,
    ) -> StreamingToolExecutor {
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
            turn_id: turn_id.to_string(),
            turn_number: self.turn_number,
            permission_mode: self.config.permission_mode,
            cwd: self.context.environment.cwd.clone(),
            is_plan_mode: self.plan_mode_state.is_active,
            plan_file_path: self.plan_mode_state.plan_file_path.clone(),
            auto_memory_dir: self
                .auto_memory_state
                .as_ref()
                .map(|s| s.config.directory.clone()),
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
        .with_file_tracker(self.shared_tools_file_tracker.clone())
        .with_approval_store(self.shared_approval_store.clone())
        .with_async_hook_tracker(self.async_hook_tracker.clone())
        .with_shell_executor(self.shell_executor.clone())
        .with_otel_manager(self.otel_manager.clone());

        // Share killed agents registry (persists across turns)
        executor = executor.with_killed_agents(self.killed_agents.clone());

        // Wire file backup store from snapshot manager for Tier 1 rewind
        if let Some(ref sm) = self.snapshot_manager {
            executor = executor.with_file_backup_store(sm.backup_store().clone());
        }

        // Wire permission rules into executor
        if !self.permission_rules.is_empty() {
            let evaluator =
                cocode_policy::PermissionRuleEvaluator::with_rules(self.permission_rules.clone());
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
                        .do_generate(input.request)
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

        // Wire cocode_home for durable cron persistence
        if let Some(ref home) = self.cocode_home {
            executor = executor.with_cocode_home(home.clone());
        }

        // Share invoked skills tracker with the executor
        executor.set_invoked_skills(self.invoked_skills_tracker.clone());

        // Apply active skill-level tool restrictions if set
        if let Some(ref allowed) = self.active_skill_allowed_tools {
            executor.set_skill_allowed_tools(Some(allowed.clone()));
        }

        // Pass parent selections for subagent isolation
        executor = executor.with_parent_selections(self.selections.clone());

        executor
    }

    /// Handle plan mode transitions from EnterPlanMode and ExitPlanMode tool calls.
    ///
    /// Returns `Some(LoopResult)` if ExitPlanMode was called (early loop exit),
    /// or `None` if no plan mode exit occurred.
    fn handle_plan_mode_transitions(
        &mut self,
        tool_calls: &[ToolCall],
        results: &[ToolExecutionResult],
        collected_content: &[AssistantContentPart],
    ) -> Option<LoopResult> {
        for tc in tool_calls {
            let tc_name = tc.tool_name.as_str();
            match tc_name {
                name if name == cocode_protocol::ToolName::EnterPlanMode.as_str() => {
                    // Skip if already in plan mode (prevents pre_plan_mode corruption)
                    if self.plan_mode_state.is_active {
                        tracing::warn!("EnterPlanMode called while already in plan mode, ignoring");
                        continue;
                    }
                    // Find the result for this tool call to extract plan file path
                    if let Some(result) = results.iter().find(|r| r.call_id == tc.tool_call_id)
                        && let Ok(output) = &result.result
                        && let ToolResultContent::Structured(json) = &output.content
                        && let (Some(path_str), Some(slug)) = (
                            json.get("planFilePath").and_then(|v| v.as_str()),
                            json.get("slug").and_then(|v| v.as_str()),
                        )
                    {
                        let path = std::path::PathBuf::from(path_str);
                        self.plan_mode_state.enter_with_mode(
                            path,
                            slug.to_string(),
                            self.turn_number,
                            self.config.permission_mode,
                        );
                        // Enforce Plan permission mode so the executor
                        // blocks non-read-only tools (especially Bash).
                        self.config.permission_mode = cocode_protocol::PermissionMode::Plan;
                        info!(turn = self.turn_number, "Entered plan mode");
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
                        .find(|r| r.call_id == tc.tool_call_id)
                        .and_then(|r| r.result.as_ref().ok())
                        .and_then(|output| match &output.content {
                            ToolResultContent::Structured(json) => {
                                json.get("allowedPrompts")?.as_array().map(|arr| {
                                    arr.iter()
                                        .filter_map(|item| {
                                            Some(cocode_protocol::AllowedPrompt {
                                                tool: item.get("tool")?.as_str()?.to_string(),
                                                prompt: item.get("prompt")?.as_str()?.to_string(),
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

                    return Some(LoopResult::plan_mode_exit(
                        self.turn_number,
                        self.total_input_tokens,
                        self.total_output_tokens,
                        true, // approved: user approved via permission dialog
                        None, // exit_option: determined by TUI layer
                        allowed_prompts,
                        collected_content.to_vec(),
                    ));
                }
                _ => {}
            }
        }
        None
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

#[cfg(test)]
#[path = "driver.test.rs"]
mod tests;
