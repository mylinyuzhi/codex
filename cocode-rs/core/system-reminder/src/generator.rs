//! Generator trait and context for system reminders.
//!
//! This module defines the [`AttachmentGenerator`] trait that all reminder
//! generators must implement, and the [`GeneratorContext`] that provides
//! the runtime state needed for generation.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use cocode_tools::FileTracker;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// IDE selection context from connected IDE via MCP.
#[derive(Debug, Clone)]
pub struct IdeSelection {
    pub ide_name: String,
    pub filename: String,
    pub line_start: i32,
    pub line_end: i32,
    pub content: String,
}

/// Trait for attachment generators.
///
/// Each generator is responsible for producing a specific type of system
/// reminder based on the current context. Generators are run in parallel
/// with timeout protection.
#[async_trait]
pub trait AttachmentGenerator: Send + Sync + Debug {
    /// Unique name for this generator.
    fn name(&self) -> &str;

    /// The type of attachment this generator produces.
    fn attachment_type(&self) -> AttachmentType;

    /// The tier this generator belongs to.
    fn tier(&self) -> ReminderTier {
        self.attachment_type().tier()
    }

    /// Generate the reminder content.
    ///
    /// Returns `Ok(Some(reminder))` if content was generated,
    /// `Ok(None)` if there's nothing to generate this turn,
    /// or `Err` if generation failed.
    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>>;

    /// Check if this generator is enabled in the config.
    fn is_enabled(&self, config: &SystemReminderConfig) -> bool;

    /// Get the throttle configuration for this generator.
    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::default()
    }

    /// Get context-aware throttle configuration.
    ///
    /// Override this when the throttle parameters are user-configurable and
    /// stored in the `GeneratorContext` (e.g., `auto_memory_state.config`).
    /// The default delegates to [`Self::throttle_config()`].
    fn throttle_config_for_context(&self, _ctx: &GeneratorContext<'_>) -> ThrottleConfig {
        self.throttle_config()
    }
}

/// Background task information.
#[derive(Debug, Clone)]
pub struct BackgroundTaskInfo {
    /// Unique task identifier.
    pub task_id: String,
    /// Type of background task.
    pub task_type: BackgroundTaskType,
    /// Command or description.
    pub command: String,
    /// Current status.
    pub status: BackgroundTaskStatus,
    /// Exit code if completed.
    pub exit_code: Option<i32>,
    /// Whether there's new output since last check.
    pub has_new_output: bool,
    /// Latest progress message from the agent (if running).
    pub progress_message: Option<String>,
    /// Whether this is a completion notification (bypasses throttle).
    pub is_completion_notification: bool,
    /// Summary of new output since last report (for completed agents).
    pub delta_summary: Option<String>,
    /// Human-readable description of the task.
    pub description: Option<String>,
}

/// Type of background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundTaskType {
    /// Shell command running in background.
    Shell,
    /// Async agent task.
    AsyncAgent,
    /// Remote session.
    RemoteSession,
}

/// Status of a background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundTaskStatus {
    /// Task is still running.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed.
    Failed,
}

/// Approved plan information (one-time injection after ExitPlanMode).
#[derive(Debug, Clone)]
pub struct ApprovedPlanInfo {
    /// The approved plan content.
    pub content: String,
    /// Turn when the plan was approved.
    pub approved_turn: i32,
}

/// Restored plan information (after compaction recovery).
#[derive(Debug, Clone)]
pub struct RestoredPlanInfo {
    /// The plan file content.
    pub content: String,
    /// Path to the plan file.
    pub file_path: PathBuf,
}

/// LSP diagnostic information.
#[derive(Debug, Clone)]
pub struct DiagnosticInfo {
    /// File path.
    pub file_path: PathBuf,
    /// Line number (1-based).
    pub line: i32,
    /// Column number (1-based).
    pub column: i32,
    /// Severity (error, warning, info, hint).
    pub severity: String,
    /// Diagnostic message.
    pub message: String,
    /// Diagnostic code.
    pub code: Option<String>,
    /// Source server name (e.g. "rust-analyzer").
    pub source: Option<String>,
}

/// Todo/task item information (plain TodoWrite items).
#[derive(Debug, Clone)]
pub struct TodoItem {
    /// Task ID.
    pub id: String,
    /// Task subject/title.
    pub subject: String,
    /// Task status.
    pub status: TodoStatus,
    /// Whether this task is blocked.
    pub is_blocked: bool,
}

/// Rich structured task information (from TaskCreate/TaskUpdate tools).
#[derive(Debug, Clone)]
pub struct StructuredTaskInfo {
    /// Task ID.
    pub id: String,
    /// Task subject/title.
    pub subject: String,
    /// Detailed description.
    pub description: Option<String>,
    /// Status string (pending, in_progress, completed).
    pub status: String,
    /// Present-continuous form of the task (e.g., "Fixing auth bug").
    pub active_form: Option<String>,
    /// Task owner (agent/team name).
    pub owner: Option<String>,
    /// IDs of tasks this task blocks.
    pub blocks: Vec<String>,
    /// IDs of tasks that block this task.
    pub blocked_by: Vec<String>,
    /// Whether this task is blocked by any incomplete task.
    pub is_blocked: bool,
}

/// Status of a todo item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    /// Task is pending.
    Pending,
    /// Task is in progress.
    InProgress,
    /// Task is completed.
    Completed,
}

impl std::fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TodoStatus::Pending => write!(f, "pending"),
            TodoStatus::InProgress => write!(f, "in_progress"),
            TodoStatus::Completed => write!(f, "completed"),
        }
    }
}

/// Information about a delegated agent.
#[derive(Debug, Clone)]
pub struct DelegatedAgentInfo {
    /// Agent identifier.
    pub agent_id: String,
    /// Agent type (e.g., "Explore", "Plan").
    pub agent_type: String,
    /// Current status.
    pub status: String,
    /// Brief description of what the agent is doing.
    pub description: String,
}

/// Token usage statistics.
#[derive(Debug, Clone, Default)]
pub struct TokenUsageStats {
    /// Input tokens consumed.
    pub input_tokens: i64,
    /// Output tokens generated.
    pub output_tokens: i64,
    /// Cache read tokens (if applicable).
    pub cache_read_tokens: i64,
    /// Cache write tokens (if applicable).
    pub cache_write_tokens: i64,
    /// Total tokens used in session.
    pub total_session_tokens: i64,
    /// Context window capacity.
    pub context_capacity: i64,
    /// Percentage of context used.
    pub context_usage_percent: f64,
}

/// Budget information.
#[derive(Debug, Clone)]
pub struct BudgetInfo {
    /// Total budget in USD.
    pub total_usd: f64,
    /// Used budget in USD.
    pub used_usd: f64,
    /// Remaining budget in USD.
    pub remaining_usd: f64,
    /// Whether budget is low (< 10% remaining).
    pub is_low: bool,
}

/// Information about a skill for the system reminder.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    /// Skill name (slash command identifier).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Guidance for the LLM on when to invoke this skill.
    pub when_to_use: Option<String>,
    /// Whether this is a bundled (built-in) skill. Bundled skills get priority
    /// in budget-aware formatting and are never truncated.
    pub is_bundled: bool,
    /// Plugin name if this skill was contributed by a plugin.
    /// Shown as `"(from plugin-name)"` in the system reminder.
    pub plugin_name: Option<String>,
}

/// Information about an invoked skill.
#[derive(Debug, Clone)]
pub struct InvokedSkillInfo {
    /// Skill name (slash command identifier, e.g., "commit", "review-pr").
    pub name: String,
    /// The skill's prompt content (typically from SKILL.md or similar).
    pub prompt_content: String,
}

/// Information about a completed async hook response.
#[derive(Debug, Clone)]
pub struct AsyncHookResponseInfo {
    /// Name of the hook that completed.
    pub hook_name: String,
    /// The additional context returned by the hook.
    pub additional_context: Option<String>,
    /// Whether the hook blocked execution.
    pub was_blocking: bool,
    /// Reason for blocking (if was_blocking is true).
    pub blocking_reason: Option<String>,
    /// Execution duration in milliseconds.
    pub duration_ms: i64,
}

/// Information about hook context to inject.
#[derive(Debug, Clone)]
pub struct HookContextInfo {
    /// Name of the hook.
    pub hook_name: String,
    /// Event type (e.g., "pre_tool_use").
    pub event_type: String,
    /// Tool name if applicable.
    pub tool_name: Option<String>,
    /// Additional context from the hook.
    pub additional_context: String,
}

/// Information about a hook that blocked execution.
#[derive(Debug, Clone)]
pub struct HookBlockingInfo {
    /// Name of the hook that blocked.
    pub hook_name: String,
    /// Event type (e.g., "pre_tool_use").
    pub event_type: String,
    /// Tool name that was blocked.
    pub tool_name: Option<String>,
    /// Reason for blocking.
    pub reason: String,
}

/// Grouped hook state for the generator context.
#[derive(Debug, Clone, Default)]
pub struct HookState {
    /// Completed async hook responses.
    pub async_responses: Vec<AsyncHookResponseInfo>,
    /// Hook contexts to inject.
    pub contexts: Vec<HookContextInfo>,
    /// Hooks that blocked execution.
    pub blocking: Vec<HookBlockingInfo>,
}

/// Information about a large file that was compacted.
///
/// Used to track files that were read before compaction but are too large
/// to include in the restored context. The CompactFileReferenceGenerator
/// uses this to inform the model about these files.
#[derive(Debug, Clone)]
pub struct CompactedLargeFile {
    /// Path to the file.
    pub path: PathBuf,
    /// Number of lines in the file.
    pub line_count: usize,
    /// File size in bytes.
    pub byte_size: usize,
}

/// Record of a file read via @mention syntax.
///
/// Captures the details of a file that was read because it was mentioned
/// in the user's prompt. These records are synced back to the tools
/// FileTracker after reminder generation to ensure proper state tracking.
#[derive(Debug, Clone)]
pub struct MentionReadRecord {
    /// Path to the file that was read.
    pub path: PathBuf,
    /// Content of the file at read time.
    pub content: String,
    /// Last modification time of the file.
    pub last_modified: Option<std::time::SystemTime>,
    /// Line offset if partial read (None for full reads).
    pub offset: Option<i64>,
    /// Line limit if partial read (None for full reads).
    pub limit: Option<i64>,
    /// Kind of read operation.
    pub read_kind: cocode_protocol::FileReadKind,
    /// Turn number when this read occurred.
    pub read_turn: i32,
}

/// Cron job information for system reminders.
#[derive(Debug, Clone)]
pub struct CronJobInfo {
    /// Job ID.
    pub id: String,
    /// Cron schedule expression.
    pub cron: String,
    /// Job description or prompt snippet.
    pub description: String,
    /// Whether this is a one-shot (non-recurring) job.
    pub one_shot: bool,
    /// Number of executions so far.
    pub execution_count: u32,
}

/// Collaboration notification from another agent.
#[derive(Debug, Clone)]
pub struct CollabNotification {
    /// Source agent identifier.
    pub from_agent: String,
    /// Notification type (e.g., "completed", "needs_input", "error").
    pub notification_type: String,
    /// Notification message.
    pub message: String,
    /// Turn when notification was received.
    pub received_turn: i32,
}

/// Information about a queued command (real-time steering).
///
/// Queued commands are entered by the user via Enter during streaming.
/// They are consumed once and injected as steering system-reminders that
/// ask the model to address the message and continue with its tasks.
///
/// The optional `target_turn` field allows stale-command rejection:
/// if set, the command is only injected when `target_turn` matches the
/// current turn number (inspired by codex-rs `expected_turn_id`).
#[derive(Debug, Clone)]
pub struct QueuedCommandInfo {
    /// Unique identifier for this command.
    pub id: String,
    /// The user's prompt/message.
    pub prompt: String,
    /// When the command was queued (Unix millis).
    pub queued_at: i64,
    /// Turn number when this command was queued.
    /// Used for stale-command rejection: if set, the command is only
    /// injected when the current turn matches.
    pub target_turn: Option<i32>,
}

/// Team context data for the current agent.
#[derive(Debug, Clone)]
pub struct TeamContextData {
    /// Agent ID.
    pub agent_id: String,
    /// Display name.
    pub agent_name: Option<String>,
    /// Team name.
    pub team_name: String,
    /// Agent type (e.g., "Explore", "general-purpose").
    pub agent_type: String,
    /// Team members.
    pub members: Vec<TeamMemberInfo>,
}

/// Information about a team member.
#[derive(Debug, Clone)]
pub struct TeamMemberInfo {
    /// Agent ID.
    pub agent_id: String,
    /// Display name.
    pub name: Option<String>,
    /// Agent type.
    pub agent_type: Option<String>,
    /// Current status (e.g., "active", "idle", "stopped").
    pub status: String,
}

/// Well-known message type identifiers.
///
/// These match `cocode_team::MessageType::as_str()` values. Defined here
/// to avoid a dependency on `cocode-team` from the system-reminder crate.
pub mod message_types {
    pub const SHUTDOWN_REQUEST: &str = "shutdown_request";
    pub const SHUTDOWN_RESPONSE: &str = "shutdown_response";
}

/// An unread message from the agent's mailbox.
#[derive(Debug, Clone)]
pub struct UnreadMessage {
    /// Message ID.
    pub id: String,
    /// Sender ID or name.
    pub from: String,
    /// Message content.
    pub content: String,
    /// Message type (e.g., "message", "shutdown_request").
    pub message_type: String,
    /// Unix timestamp.
    pub timestamp: i64,
}

/// Context passed to generators during execution.
///
/// This provides all the runtime state needed for generators to
/// determine what content to produce.
#[derive(Debug)]
pub struct GeneratorContext<'a> {
    /// Current configuration.
    pub config: &'a SystemReminderConfig,

    // === Turn tracking ===
    /// Current turn number.
    pub turn_number: i32,
    /// Whether this is the main agent (not a subagent).
    pub is_main_agent: bool,
    /// Whether there's user input this turn.
    pub has_user_input: bool,
    /// Context window size in tokens.
    /// Used for token-aware decisions in generators.
    pub context_window: i32,

    // === User input ===
    /// The user's prompt text (if any).
    pub user_prompt: Option<&'a str>,
    /// Files mentioned via @file syntax.
    pub user_mentioned_files: Vec<PathBuf>,
    /// Agents mentioned via @agent-type syntax.
    pub user_mentioned_agents: Vec<String>,

    // === File state ===
    /// File tracker for change detection.
    pub file_tracker: Option<&'a FileTracker>,
    /// Current working directory.
    pub cwd: PathBuf,
    /// Plan file path (if in plan mode).
    pub plan_file_path: Option<PathBuf>,

    // === Plan state ===
    /// Whether plan mode is active.
    pub is_plan_mode: bool,
    /// Whether this is a re-entry into plan mode.
    pub is_plan_reentry: bool,
    /// Whether interview-style plan mode is active.
    pub is_plan_interview_phase: bool,
    /// Whether this is an ultraplan session (plan pre-written by remote session).
    pub is_ultraplan: bool,
    /// Phase 4 instruction variant for plan mode (Gap 5).
    pub phase4_variant: crate::generators::plan_mode::Phase4Variant,
    /// Maximum Explore agent count for plan mode Phase 1 (Gap 6).
    pub explore_agent_count: i32,
    /// Minimum Plan agent count for plan mode Phase 2 (Gap 6).
    pub plan_agent_count: i32,
    /// Approved plan (one-time, after ExitPlanMode).
    pub approved_plan: Option<ApprovedPlanInfo>,
    /// Restored plan (after compaction).
    pub restored_plan: Option<RestoredPlanInfo>,

    // === Background tasks ===
    /// Currently running background tasks.
    pub background_tasks: Vec<BackgroundTaskInfo>,

    // === Diagnostics ===
    /// LSP diagnostics.
    pub diagnostics: Vec<DiagnosticInfo>,

    // === Todo/Tasks ===
    /// Current todo items (plain TodoWrite).
    pub todos: Vec<TodoItem>,
    /// Rich structured tasks (from TaskCreate/TaskUpdate).
    pub structured_tasks: Vec<StructuredTaskInfo>,

    // === Cron Jobs ===
    /// Current cron jobs for reminder injection.
    pub cron_jobs: Vec<CronJobInfo>,

    // === Nested memory ===
    /// Paths that trigger nested memory lookup.
    pub nested_memory_triggers: HashSet<PathBuf>,

    // === Full content flags ===
    /// Per-generator full-content flags, pre-computed by the orchestrator.
    /// Maps attachment type to whether full (true) or sparse (false) content
    /// should be generated this turn.
    pub full_content_flags: HashMap<AttachmentType, bool>,

    // === Typed extension data ===
    /// Hook state (async responses, contexts, blocking).
    pub hook_state: HookState,
    /// Available skills for the Skill tool.
    pub available_skills: Vec<SkillInfo>,
    /// Currently invoked skills.
    pub invoked_skills: Vec<InvokedSkillInfo>,
    /// Large files compacted but not restored.
    pub compacted_large_files: Vec<CompactedLargeFile>,

    // === Delegate mode state ===
    /// Whether delegate mode is active.
    pub is_delegate_mode: bool,
    /// Whether exiting delegate mode this turn.
    pub delegate_mode_exiting: bool,
    /// Information about delegated agents.
    pub delegated_agents: Vec<DelegatedAgentInfo>,

    // === Auto mode state ===
    /// Whether auto mode (autonomous execution) is active.
    pub is_auto_mode: bool,
    /// Whether auto mode exit attachment should be generated this turn.
    pub auto_mode_exit_pending: bool,

    // === Token/budget tracking ===
    /// Token usage statistics.
    pub token_usage: Option<TokenUsageStats>,
    /// Budget information.
    pub budget: Option<BudgetInfo>,

    // === Collaboration notifications ===
    /// Pending collaboration notifications from other agents.
    pub collab_notifications: Vec<CollabNotification>,

    // === Team context ===
    /// Team context for the current agent (if in a team).
    pub team_context: Option<TeamContextData>,
    /// Unread mailbox messages for the current agent.
    pub unread_messages: Vec<UnreadMessage>,

    // === Real-time steering ===
    /// Queued commands from user (Enter during streaming).
    /// Consumed once and injected as steering that asks the model to address each message.
    pub queued_commands: Vec<QueuedCommandInfo>,

    // === Global state flags ===
    /// Whether plan mode exit is pending (triggers one-time exit instructions).
    pub plan_mode_exit_pending: bool,

    // === Compaction state ===
    /// Whether auto-compaction is enabled for this session.
    pub is_auto_compact_enabled: bool,

    // === Rewind state ===
    /// Information about a rewind that just occurred (consumed once).
    pub rewind_info: Option<RewindContextInfo>,

    // === Auto memory ===
    /// Auto memory state for prompt injection and relevant memories search.
    pub auto_memory_state: Option<Arc<cocode_auto_memory::AutoMemoryState>>,

    // === Mention read records ===
    /// Records of files read via @mention syntax during this turn.
    ///
    /// Shared buffer populated by the @mentioned_files generator when it reads
    /// file contents. Uses `Arc<Mutex<>>` so generators can push records via
    /// `&self` without needing mutable access to the context.
    /// After reminder generation, the driver drains and syncs these records
    /// back to the tools FileTracker for proper state tracking.
    pub mention_read_records: std::sync::Arc<std::sync::Mutex<Vec<MentionReadRecord>>>,

    // === Worktree state ===
    /// Number of active worktrees in the session.
    pub active_worktree_count: i32,

    // === Sandbox violations ===
    /// Recent sandbox violations (operation, path, command_tag).
    pub sandbox_violations: Vec<(String, Option<String>, Option<String>)>,

    // === Delta tracking ===
    /// Deferred tools added since last turn.
    pub deferred_tools_added: Vec<String>,
    /// Deferred tools removed since last turn.
    pub deferred_tools_removed: Vec<String>,
    /// MCP instruction changes (server → new instruction).
    pub mcp_instructions_changes: Vec<(String, String)>,

    // === Session info ===
    /// Session name (if set).
    pub session_name: Option<String>,
    /// Configuration changes since last turn.
    pub config_changes: Vec<String>,

    /// Whether auto mode is active (autonomous execution mode).
    pub auto_mode: bool,
    /// Whether exiting auto mode this turn (was active, now inactive).
    pub auto_mode_exiting: bool,
    /// Current reasoning effort level for ultrathink reminder.
    pub thinking_effort: Option<cocode_protocol::model::ReasoningEffort>,
    /// Last recorded date for date change detection.
    pub last_recorded_date: Option<String>,
    /// IDE selection context (user-selected lines in IDE).
    pub ide_selection: Option<IdeSelection>,
    /// IDE opened file (user opened a file in IDE).
    pub ide_opened_file: Option<String>,
}

/// Lightweight rewind info for system reminder generation.
///
/// Avoids a dependency on `cocode-file-backup` by carrying only the
/// data needed for the reminder text.
#[derive(Debug, Clone)]
pub struct RewindContextInfo {
    /// The turn number that was rewound.
    pub rewound_turn_number: i32,
    /// Number of files restored.
    pub restored_file_count: i32,
    /// Whether git restore was used (vs file-only backup).
    pub used_git_restore: bool,
    /// The rewind mode used. Determines whether a reminder is emitted.
    ///
    /// - `CodeOnly` → emit reminder (model needs to know files were reverted
    ///   while conversation continues).
    /// - `CodeAndConversation` / `ConversationOnly` → skip (conversation is
    ///   truncated so the model has no memory of the rewound turns).
    pub rewind_mode: RewindMode,
}

/// Re-export for convenience; avoids downstream crates needing protocol dep
/// just for the mode enum.
pub use cocode_protocol::RewindMode;

impl<'a> GeneratorContext<'a> {
    /// Create a builder for constructing generator context.
    pub fn builder() -> GeneratorContextBuilder<'a> {
        GeneratorContextBuilder::default()
    }

    /// Check if this generator should produce full content this turn.
    /// Falls back to `true` (full) when no flag is set (e.g., in tests).
    pub fn should_use_full_content(&self, attachment_type: AttachmentType) -> bool {
        self.full_content_flags
            .get(&attachment_type)
            .copied()
            .unwrap_or(true)
    }
}

/// Builder for [`GeneratorContext`].
pub struct GeneratorContextBuilder<'a> {
    config: Option<&'a SystemReminderConfig>,
    turn_number: i32,
    is_main_agent: bool,
    has_user_input: bool,
    context_window: i32,
    user_prompt: Option<&'a str>,
    user_mentioned_files: Vec<PathBuf>,
    user_mentioned_agents: Vec<String>,
    file_tracker: Option<&'a FileTracker>,
    cwd: Option<PathBuf>,
    plan_file_path: Option<PathBuf>,
    is_plan_mode: bool,
    is_plan_reentry: bool,
    is_plan_interview_phase: bool,
    is_ultraplan: bool,
    phase4_variant: crate::generators::plan_mode::Phase4Variant,
    explore_agent_count: i32,
    plan_agent_count: i32,
    approved_plan: Option<ApprovedPlanInfo>,
    restored_plan: Option<RestoredPlanInfo>,
    background_tasks: Vec<BackgroundTaskInfo>,
    diagnostics: Vec<DiagnosticInfo>,
    todos: Vec<TodoItem>,
    structured_tasks: Vec<StructuredTaskInfo>,
    cron_jobs: Vec<CronJobInfo>,
    nested_memory_triggers: HashSet<PathBuf>,
    full_content_flags: HashMap<AttachmentType, bool>,
    hook_state: HookState,
    available_skills: Vec<SkillInfo>,
    invoked_skills: Vec<InvokedSkillInfo>,
    compacted_large_files: Vec<CompactedLargeFile>,
    is_delegate_mode: bool,
    delegate_mode_exiting: bool,
    delegated_agents: Vec<DelegatedAgentInfo>,
    is_auto_mode: bool,
    auto_mode_exit_pending: bool,
    token_usage: Option<TokenUsageStats>,
    budget: Option<BudgetInfo>,
    collab_notifications: Vec<CollabNotification>,
    team_context: Option<TeamContextData>,
    unread_messages: Vec<UnreadMessage>,
    queued_commands: Vec<QueuedCommandInfo>,
    plan_mode_exit_pending: bool,
    rewind_info: Option<RewindContextInfo>,
    mention_read_records: std::sync::Arc<std::sync::Mutex<Vec<MentionReadRecord>>>,
    is_auto_compact_enabled: bool,
    auto_memory_state: Option<Arc<cocode_auto_memory::AutoMemoryState>>,
    active_worktree_count: i32,
    sandbox_violations: Vec<(String, Option<String>, Option<String>)>,
    deferred_tools_added: Vec<String>,
    deferred_tools_removed: Vec<String>,
    mcp_instructions_changes: Vec<(String, String)>,
    session_name: Option<String>,
    config_changes: Vec<String>,
    auto_mode: bool,
    auto_mode_exiting: bool,
    thinking_effort: Option<cocode_protocol::model::ReasoningEffort>,
    last_recorded_date: Option<String>,
    ide_selection: Option<IdeSelection>,
    ide_opened_file: Option<String>,
}

impl Default for GeneratorContextBuilder<'_> {
    fn default() -> Self {
        Self {
            config: None,
            turn_number: 0,
            is_main_agent: false,
            has_user_input: false,
            context_window: 0,
            user_prompt: None,
            user_mentioned_files: Vec::new(),
            user_mentioned_agents: Vec::new(),
            file_tracker: None,
            cwd: None,
            plan_file_path: None,
            is_plan_mode: false,
            is_plan_reentry: false,
            is_plan_interview_phase: false,
            is_ultraplan: false,
            phase4_variant: crate::generators::plan_mode::Phase4Variant::default(),
            explore_agent_count: cocode_protocol::DEFAULT_PLAN_EXPLORE_AGENT_COUNT,
            plan_agent_count: cocode_protocol::DEFAULT_PLAN_AGENT_COUNT,
            approved_plan: None,
            restored_plan: None,
            background_tasks: Vec::new(),
            diagnostics: Vec::new(),
            todos: Vec::new(),
            structured_tasks: Vec::new(),
            cron_jobs: Vec::new(),
            nested_memory_triggers: HashSet::new(),
            full_content_flags: HashMap::new(),
            hook_state: HookState::default(),
            available_skills: Vec::new(),
            invoked_skills: Vec::new(),
            compacted_large_files: Vec::new(),
            is_delegate_mode: false,
            delegate_mode_exiting: false,
            delegated_agents: Vec::new(),
            is_auto_mode: false,
            auto_mode_exit_pending: false,
            token_usage: None,
            budget: None,
            collab_notifications: Vec::new(),
            team_context: None,
            unread_messages: Vec::new(),
            queued_commands: Vec::new(),
            plan_mode_exit_pending: false,
            rewind_info: None,
            mention_read_records: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            is_auto_compact_enabled: false,
            auto_memory_state: None,
            active_worktree_count: 0,
            sandbox_violations: Vec::new(),
            deferred_tools_added: Vec::new(),
            deferred_tools_removed: Vec::new(),
            mcp_instructions_changes: Vec::new(),
            session_name: None,
            config_changes: Vec::new(),
            auto_mode: false,
            auto_mode_exiting: false,
            thinking_effort: None,
            last_recorded_date: None,
            ide_selection: None,
            ide_opened_file: None,
        }
    }
}

impl<'a> GeneratorContextBuilder<'a> {
    pub fn config(mut self, config: &'a SystemReminderConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn turn_number(mut self, turn: i32) -> Self {
        self.turn_number = turn;
        self
    }

    pub fn is_main_agent(mut self, is_main: bool) -> Self {
        self.is_main_agent = is_main;
        self
    }

    pub fn has_user_input(mut self, has_input: bool) -> Self {
        self.has_user_input = has_input;
        self
    }

    pub fn context_window(mut self, tokens: i32) -> Self {
        self.context_window = tokens;
        self
    }

    pub fn user_prompt(mut self, prompt: &'a str) -> Self {
        self.user_prompt = Some(prompt);
        self
    }

    pub fn user_mentioned_files(mut self, files: Vec<PathBuf>) -> Self {
        self.user_mentioned_files = files;
        self
    }

    pub fn user_mentioned_agents(mut self, agents: Vec<String>) -> Self {
        self.user_mentioned_agents = agents;
        self
    }

    pub fn file_tracker(mut self, tracker: &'a FileTracker) -> Self {
        self.file_tracker = Some(tracker);
        self
    }

    pub fn cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    pub fn plan_file_path(mut self, path: PathBuf) -> Self {
        self.plan_file_path = Some(path);
        self
    }

    pub fn is_plan_mode(mut self, is_plan: bool) -> Self {
        self.is_plan_mode = is_plan;
        self
    }

    pub fn is_plan_reentry(mut self, is_reentry: bool) -> Self {
        self.is_plan_reentry = is_reentry;
        self
    }

    pub fn is_plan_interview_phase(mut self, is_interview: bool) -> Self {
        self.is_plan_interview_phase = is_interview;
        self
    }

    pub fn is_ultraplan(mut self, is_ultraplan: bool) -> Self {
        self.is_ultraplan = is_ultraplan;
        self
    }

    pub fn phase4_variant(mut self, variant: crate::generators::plan_mode::Phase4Variant) -> Self {
        self.phase4_variant = variant;
        self
    }

    pub fn explore_agent_count(mut self, count: i32) -> Self {
        self.explore_agent_count = count;
        self
    }

    pub fn plan_agent_count(mut self, count: i32) -> Self {
        self.plan_agent_count = count;
        self
    }

    pub fn approved_plan(mut self, plan: ApprovedPlanInfo) -> Self {
        self.approved_plan = Some(plan);
        self
    }

    pub fn restored_plan(mut self, plan: RestoredPlanInfo) -> Self {
        self.restored_plan = Some(plan);
        self
    }

    pub fn background_tasks(mut self, tasks: Vec<BackgroundTaskInfo>) -> Self {
        self.background_tasks = tasks;
        self
    }

    pub fn diagnostics(mut self, diags: Vec<DiagnosticInfo>) -> Self {
        self.diagnostics = diags;
        self
    }

    pub fn todos(mut self, todos: Vec<TodoItem>) -> Self {
        self.todos = todos;
        self
    }

    pub fn structured_tasks(mut self, tasks: Vec<StructuredTaskInfo>) -> Self {
        self.structured_tasks = tasks;
        self
    }

    pub fn cron_jobs(mut self, jobs: Vec<CronJobInfo>) -> Self {
        self.cron_jobs = jobs;
        self
    }

    pub fn nested_memory_triggers(mut self, triggers: HashSet<PathBuf>) -> Self {
        self.nested_memory_triggers = triggers;
        self
    }

    pub fn hook_state(mut self, state: HookState) -> Self {
        self.hook_state = state;
        self
    }

    pub fn available_skills(mut self, skills: Vec<SkillInfo>) -> Self {
        self.available_skills = skills;
        self
    }

    pub fn invoked_skills(mut self, skills: Vec<InvokedSkillInfo>) -> Self {
        self.invoked_skills = skills;
        self
    }

    pub fn compacted_large_files(mut self, files: Vec<CompactedLargeFile>) -> Self {
        self.compacted_large_files = files;
        self
    }

    pub fn is_delegate_mode(mut self, is_delegate: bool) -> Self {
        self.is_delegate_mode = is_delegate;
        self
    }

    pub fn delegate_mode_exiting(mut self, exiting: bool) -> Self {
        self.delegate_mode_exiting = exiting;
        self
    }

    pub fn is_auto_mode(mut self, active: bool) -> Self {
        self.is_auto_mode = active;
        self
    }

    pub fn auto_mode_exit_pending(mut self, pending: bool) -> Self {
        self.auto_mode_exit_pending = pending;
        self
    }

    pub fn delegated_agents(mut self, agents: Vec<DelegatedAgentInfo>) -> Self {
        self.delegated_agents = agents;
        self
    }

    pub fn token_usage(mut self, usage: TokenUsageStats) -> Self {
        self.token_usage = Some(usage);
        self
    }

    pub fn budget(mut self, budget: BudgetInfo) -> Self {
        self.budget = Some(budget);
        self
    }

    pub fn collab_notifications(mut self, notifications: Vec<CollabNotification>) -> Self {
        self.collab_notifications = notifications;
        self
    }

    pub fn team_context(mut self, ctx: TeamContextData) -> Self {
        self.team_context = Some(ctx);
        self
    }

    pub fn unread_messages(mut self, messages: Vec<UnreadMessage>) -> Self {
        self.unread_messages = messages;
        self
    }

    pub fn queued_commands(mut self, commands: Vec<QueuedCommandInfo>) -> Self {
        self.queued_commands = commands;
        self
    }

    pub fn plan_mode_exit_pending(mut self, pending: bool) -> Self {
        self.plan_mode_exit_pending = pending;
        self
    }

    pub fn rewind_info(mut self, info: RewindContextInfo) -> Self {
        self.rewind_info = Some(info);
        self
    }

    pub fn mention_read_records(
        mut self,
        records: std::sync::Arc<std::sync::Mutex<Vec<MentionReadRecord>>>,
    ) -> Self {
        self.mention_read_records = records;
        self
    }

    pub fn is_auto_compact_enabled(mut self, enabled: bool) -> Self {
        self.is_auto_compact_enabled = enabled;
        self
    }

    pub fn auto_memory_state(mut self, state: Arc<cocode_auto_memory::AutoMemoryState>) -> Self {
        self.auto_memory_state = Some(state);
        self
    }

    pub fn active_worktree_count(mut self, count: i32) -> Self {
        self.active_worktree_count = count;
        self
    }

    pub fn sandbox_violations(
        mut self,
        violations: Vec<(String, Option<String>, Option<String>)>,
    ) -> Self {
        self.sandbox_violations = violations;
        self
    }

    pub fn deferred_tools_added(mut self, tools: Vec<String>) -> Self {
        self.deferred_tools_added = tools;
        self
    }

    pub fn deferred_tools_removed(mut self, tools: Vec<String>) -> Self {
        self.deferred_tools_removed = tools;
        self
    }

    pub fn mcp_instructions_changes(mut self, changes: Vec<(String, String)>) -> Self {
        self.mcp_instructions_changes = changes;
        self
    }

    pub fn session_name(mut self, name: impl Into<String>) -> Self {
        self.session_name = Some(name.into());
        self
    }

    pub fn config_changes(mut self, changes: Vec<String>) -> Self {
        self.config_changes = changes;
        self
    }

    pub fn auto_mode(mut self, active: bool) -> Self {
        self.auto_mode = active;
        self
    }

    pub fn auto_mode_exiting(mut self, exiting: bool) -> Self {
        self.auto_mode_exiting = exiting;
        self
    }

    pub fn thinking_effort(mut self, effort: cocode_protocol::model::ReasoningEffort) -> Self {
        self.thinking_effort = Some(effort);
        self
    }

    pub fn last_recorded_date(mut self, date: impl Into<String>) -> Self {
        self.last_recorded_date = Some(date.into());
        self
    }

    pub fn ide_selection(mut self, selection: IdeSelection) -> Self {
        self.ide_selection = Some(selection);
        self
    }

    pub fn ide_opened_file(mut self, path: impl Into<String>) -> Self {
        self.ide_opened_file = Some(path.into());
        self
    }

    /// Build the generator context.
    ///
    /// # Panics
    ///
    /// Panics if `config` or `cwd` is not set.
    #[allow(clippy::expect_used)]
    pub fn build(self) -> GeneratorContext<'a> {
        GeneratorContext {
            config: self.config.expect("config is required"),
            turn_number: self.turn_number,
            is_main_agent: self.is_main_agent,
            has_user_input: self.has_user_input,
            context_window: self.context_window,
            user_prompt: self.user_prompt,
            user_mentioned_files: self.user_mentioned_files,
            user_mentioned_agents: self.user_mentioned_agents,
            file_tracker: self.file_tracker,
            cwd: self.cwd.expect("cwd is required"),
            plan_file_path: self.plan_file_path,
            is_plan_mode: self.is_plan_mode,
            is_plan_reentry: self.is_plan_reentry,
            is_plan_interview_phase: self.is_plan_interview_phase,
            is_ultraplan: self.is_ultraplan,
            phase4_variant: self.phase4_variant,
            explore_agent_count: self.explore_agent_count,
            plan_agent_count: self.plan_agent_count,
            approved_plan: self.approved_plan,
            restored_plan: self.restored_plan,
            background_tasks: self.background_tasks,
            diagnostics: self.diagnostics,
            todos: self.todos,
            structured_tasks: self.structured_tasks,
            cron_jobs: self.cron_jobs,
            nested_memory_triggers: self.nested_memory_triggers,
            full_content_flags: self.full_content_flags,
            hook_state: self.hook_state,
            available_skills: self.available_skills,
            invoked_skills: self.invoked_skills,
            compacted_large_files: self.compacted_large_files,
            is_delegate_mode: self.is_delegate_mode,
            delegate_mode_exiting: self.delegate_mode_exiting,
            delegated_agents: self.delegated_agents,
            is_auto_mode: self.is_auto_mode,
            auto_mode_exit_pending: self.auto_mode_exit_pending,
            token_usage: self.token_usage,
            budget: self.budget,
            collab_notifications: self.collab_notifications,
            team_context: self.team_context,
            unread_messages: self.unread_messages,
            queued_commands: self.queued_commands,
            plan_mode_exit_pending: self.plan_mode_exit_pending,
            is_auto_compact_enabled: self.is_auto_compact_enabled,
            rewind_info: self.rewind_info,
            mention_read_records: self.mention_read_records,
            auto_memory_state: self.auto_memory_state,
            active_worktree_count: self.active_worktree_count,
            sandbox_violations: self.sandbox_violations,
            deferred_tools_added: self.deferred_tools_added,
            deferred_tools_removed: self.deferred_tools_removed,
            mcp_instructions_changes: self.mcp_instructions_changes,
            session_name: self.session_name,
            config_changes: self.config_changes,
            auto_mode: self.auto_mode,
            auto_mode_exiting: self.auto_mode_exiting,
            thinking_effort: self.thinking_effort,
            last_recorded_date: self.last_recorded_date,
            ide_selection: self.ide_selection,
            ide_opened_file: self.ide_opened_file,
        }
    }
}

#[cfg(test)]
#[path = "generator.test.rs"]
mod tests;
