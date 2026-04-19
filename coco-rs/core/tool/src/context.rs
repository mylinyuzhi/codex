use coco_context::FileHistoryState;
use coco_context::FileReadState;
use coco_types::AgentId;
use coco_types::AgentTypeId;
use coco_types::Message;
use coco_types::ThinkingLevel;
use coco_types::ToolPermissionContext;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use std::path::PathBuf;

use crate::agent_handle::AgentHandleRef;
use crate::hook_handle::HookHandleRef;
use crate::mcp_handle::McpHandleRef;
use crate::permission_bridge::ToolPermissionBridgeRef;
use crate::registry::ToolRegistry;
use crate::schedule_store::ScheduleStoreRef;
use crate::side_query::SideQueryHandle;
use crate::task_handle::TaskHandleRef;
use crate::traits::ProgressSender;

/// Local denial tracking state for auto-mode fail-safe.
///
/// When consecutive or total denials exceed thresholds, the agent
/// falls back to prompting instead of continuing autonomously.
#[derive(Debug)]
pub struct DenialTrackingState {
    pub consecutive_denials: i32,
    pub total_denials: i32,
    /// Max consecutive before fallback to prompting.
    pub max_consecutive: i32,
    /// Max total before fallback to prompting.
    pub max_total: i32,
}

impl Default for DenialTrackingState {
    fn default() -> Self {
        Self::new()
    }
}

impl DenialTrackingState {
    pub fn new() -> Self {
        Self {
            consecutive_denials: 0,
            total_denials: 0,
            max_consecutive: 3,
            max_total: 20,
        }
    }

    pub fn record_denial(&mut self) {
        self.consecutive_denials += 1;
        self.total_denials += 1;
    }

    pub fn record_approval(&mut self) {
        self.consecutive_denials = 0;
    }

    pub fn should_fallback_to_prompting(&self) -> bool {
        self.consecutive_denials >= self.max_consecutive || self.total_denials >= self.max_total
    }
}

/// Context provided to every tool execution.
///
/// Maps to TS ToolUseContext (40+ fields). Organized into logical groups.
/// Passed by reference to Tool::execute(); mutated via callback closures.
pub struct ToolUseContext {
    // ── Options (from QueryEngineConfig) ──
    /// Available tools registry.
    pub tools: Arc<ToolRegistry>,
    /// Main loop model identifier.
    pub main_loop_model: String,
    /// Thinking level configuration.
    pub thinking_level: Option<ThinkingLevel>,
    /// Whether this is a non-interactive session (SDK/headless).
    pub is_non_interactive: bool,
    /// Cost budget limit (USD).
    pub max_budget_usd: Option<f64>,
    /// Custom system prompt override.
    pub custom_system_prompt: Option<String>,
    /// Appended system prompt.
    pub append_system_prompt: Option<String>,
    /// Debug mode.
    pub debug: bool,
    /// Verbose mode.
    pub verbose: bool,

    // ── Core State ──
    /// Cancellation token for aborting tool execution.
    pub cancel: CancellationToken,
    /// Message history (shared, read via lock).
    pub messages: Arc<RwLock<Vec<Message>>>,
    /// Permission context (mode + rules).
    pub permission_context: ToolPermissionContext,

    // ── Agent Identity ──
    /// Current tool use ID (set per tool call).
    pub tool_use_id: Option<String>,
    /// UUID of the user message that triggered this turn.
    /// Used by file history to key snapshots to user messages (not tool calls).
    /// TS: `parentMessage.uuid` passed to `fileHistoryTrackEdit()`.
    pub user_message_id: Option<String>,
    /// Agent running this tool.
    pub agent_id: Option<AgentId>,
    /// Agent type.
    pub agent_type: Option<AgentTypeId>,

    // ── File Tracking ──
    /// File reading limits.
    pub file_reading_limits: FileReadingLimits,
    /// Glob search limits.
    pub glob_limits: GlobLimits,

    // ── Tracking Sets (session-scoped dedup) ──
    /// Paths that triggered nested memory loading.
    ///
    /// TS `FileReadTool.ts:848,870,1038`
    /// `context.nestedMemoryAttachmentTriggers?.add(fullFilePath)` —
    /// every successful Read pushes the path here so
    /// `getNestedMemoryAttachments` (TS `utils/attachments.ts:2165`)
    /// can load any nested CLAUDE.md / memory files in the file's
    /// ancestry on the next turn boundary.
    ///
    /// Wrapped in `Arc<RwLock<>>` so concurrent-safe tools sharing a
    /// cloned context all push into the same set, mirroring the
    /// `dynamic_skill_dir_triggers` design.
    pub nested_memory_attachment_triggers: Arc<RwLock<HashSet<String>>>,
    /// Already-loaded nested memory file paths.
    pub loaded_nested_memory_paths: HashSet<String>,
    /// Directories that triggered dynamic skill discovery.
    ///
    /// TS `FileReadTool.ts:583` `context.dynamicSkillDirTriggers?.add(dir)` —
    /// when Read/Write/Edit touch a file, we walk up to find any
    /// `.claude/skills/` ancestor dir and record it here. The app/query
    /// layer drains this set after the tool batch completes and asks the
    /// SkillManager to load any newly-discovered dirs.
    ///
    /// Wrapped in `Arc<RwLock<>>` so concurrent-safe tools sharing a
    /// cloned context all push into the same set, and so the app/query
    /// drain sees everything from the just-completed batch.
    pub dynamic_skill_dir_triggers: Arc<RwLock<HashSet<String>>>,
    /// Skill names discovered during this session.
    pub discovered_skill_names: HashSet<String>,

    // ── Decision Tracking ──
    /// Per-tool execution decisions (accept/reject).
    pub tool_decisions: HashMap<String, ToolDecision>,

    // ── Flags ──
    /// Whether this context is running as a teammate in a swarm team.
    ///
    /// TS: `isTeammate()` — NOT the same as `agent_id.is_some()`.
    /// Regular subagents (Agent tool spawns) have `agent_id` set but are NOT
    /// teammates. Teammates are swarm members that coordinate via mailbox.
    /// Set by the team spawner; tools check this for teammate-specific behavior
    /// (e.g., ExitPlanMode bypasses permission UI for teammates).
    pub is_teammate: bool,

    /// When `true`, this teammate MUST request plan approval from the
    /// team lead before exiting plan mode. TS: `isPlanModeRequired()` —
    /// tied to the role definition in the team file or the
    /// `COCO_PLAN_MODE_REQUIRED` env var. When `false`,
    /// teammates in plan mode can exit "voluntarily" without leader
    /// approval (the tool skips the mailbox write and restores mode
    /// locally like a non-swarm session).
    ///
    /// Only meaningful when `is_teammate == true`. Non-teammate
    /// contexts ignore this field.
    pub plan_mode_required: bool,

    /// This teammate's own agent name (swarm identity). Pre-resolved by
    /// the engine from its configured identity + env fallback so tools
    /// don't each re-read process env. TS: `getAgentName()`.
    /// `None` in non-swarm sessions.
    pub agent_name: Option<String>,

    /// The team name this teammate belongs to. Same rationale as
    /// [`Self::agent_name`]. TS: `getTeamName()`.
    pub team_name: Option<String>,

    /// When `true`, ExitPlanMode compares the plan-file mtime against
    /// `plan_mode_entry_ms` and appends a soft advisory note if the
    /// model called Exit without actually editing the plan file.
    /// Enabled via `settings.plan_mode.verify_execution` or the
    /// `COCO_VERIFY_PLAN` env var.
    pub plan_verify_execution: bool,
    /// Whether user modified input during permission prompt.
    pub user_modified: bool,
    /// Require can_use_tool check before execution.
    pub require_can_use_tool: bool,
    /// Preserve tool use results (don't tombstone during compaction).
    pub preserve_tool_use_results: bool,

    // ── Cached Prompt ──
    /// Rendered system prompt (for tools that need prompt context).
    pub rendered_system_prompt: Option<String>,
    /// Experimental critical system reminder.
    pub critical_system_reminder: Option<String>,

    // ── IDs for active tool tracking ──
    /// Currently in-progress tool use IDs.
    pub in_progress_tool_use_ids: Arc<RwLock<HashSet<String>>>,

    // ── LLM Side Queries ──
    /// Handle for making LLM side-queries from tools.
    pub side_query: SideQueryHandle,

    // ── MCP ──
    /// Handle for MCP operations (list/read resources, call tools, auth).
    pub mcp: McpHandleRef,

    // ── Scheduling ──
    /// Handle for cron/trigger operations.
    pub schedules: ScheduleStoreRef,

    // ── Agent Operations ──
    /// Handle for agent spawning, messaging, and team management.
    pub agent: AgentHandleRef,

    // ── Swarm Mailbox ──
    /// Handle for writing protocol messages to swarm mailboxes.
    /// Used by ExitPlanModeTool (teammate plan_approval_request) and
    /// SendMessageTool (generic team-to-team messages). `NoOpMailboxHandle`
    /// in non-swarm contexts so tool calls fail fast with a clear error
    /// rather than silently dropping the message.
    pub mailbox: crate::MailboxHandleRef,

    // ── Working Directory Override ──
    /// CWD override for worktree-isolated agents.
    /// TS: cwdOverridePath in AgentTool.tsx
    pub cwd_override: Option<PathBuf>,

    // ── Permission Forwarding ──
    /// Bridge for forwarding permission requests from teammate agents.
    /// None for main agent (uses normal permission pipeline).
    /// TS: createInProcessCanUseTool() in inProcessRunner.ts
    pub permission_bridge: Option<ToolPermissionBridgeRef>,

    // ── Progress Reporting ──
    /// Channel for tool progress updates. Tools send ToolProgress here;
    /// StreamingToolExecutor yields them immediately to the TUI.
    /// TS: `onProgress` callback in `tool.call()`.
    pub progress_tx: Option<ProgressSender>,

    // ── Background Task Management ──
    /// Handle for background task operations (shell tasks, agent tasks).
    /// TS: `spawnShellTask()`, `TaskOutput`, stall watchdog.
    pub task_handle: Option<TaskHandleRef>,

    // ── Hook Pipeline ──
    /// Optional callback into the hook pipeline (PreToolUse / PostToolUse /
    /// PostToolUseFailure). When `None`, the executor skips hook invocations
    /// entirely. The higher-layer orchestrator (`app/query`) implements this
    /// trait by bridging to `coco_hooks::HookRegistry` + `execute_pre_tool_use()`
    /// / `execute_post_tool_use()`.
    /// TS: `services/tools/toolExecution.ts:800-862` hook invocation.
    pub hook_handle: Option<HookHandleRef>,

    // ── File State ──
    /// Session-level file read state for @mention dedup, edit safety, changed-file detection.
    /// TS: `readFileState` (FileStateCache) in toolUseContext.
    pub file_read_state: Option<Arc<RwLock<FileReadState>>>,

    // ── File History ──
    /// File history for checkpoint/rewind. Shared across concurrent tool calls.
    /// TS: `updateFileHistoryState` callback in toolUseContext.
    pub file_history: Option<Arc<RwLock<FileHistoryState>>>,
    /// Config home directory for file history backup storage.
    pub config_home: Option<PathBuf>,
    /// Session ID for file history backup naming.
    pub session_id_for_history: Option<String>,

    // ── Plan mode ──
    /// Resolved plans directory for plan-mode file I/O. Pre-computed by
    /// the engine from `config_home` + project root + `plansDirectory`
    /// setting, so tools can locate the plan file without re-deriving.
    /// TS: `getPlanFilePath(agentId)` reads the session-level setting.
    pub plans_dir: Option<PathBuf>,

    // ── App State ──
    /// Read-only handle to the shared cross-turn application state
    /// (plan-mode latches, reminder throttle counters, pending
    /// teammate approvals, live permission mode). Typed struct — see
    /// [`coco_types::ToolAppState`] for the field catalog.
    ///
    /// **Write access is deliberately not exposed** — tools cannot
    /// call `.write()` on this handle. Mutations route through
    /// [`coco_types::ToolResult::app_state_patch`], applied
    /// post-execute by the executor. TS parity:
    /// `orchestration.ts:queuedContextModifiers` — tools return a
    /// `(ctx) => newCtx` modifier; the orchestrator applies them
    /// after the concurrent batch finishes. Rust encodes the same
    /// discipline in the type system so a tool that tries to
    /// mutate shared state simply won't compile.
    pub app_state: Option<coco_types::AppStateReadHandle>,

    // ── Denial Tracking ──
    /// Local denial tracking state for auto-mode fail-safe.
    pub local_denial_tracking: Option<Arc<RwLock<DenialTrackingState>>>,

    // ── Query Tracking ──
    /// Query chain ID for telemetry grouping.
    pub query_chain_id: Option<String>,
    /// Query depth (0 = main, 1+ = subagent).
    pub query_depth: i32,
}

/// File reading limits for tools.
#[derive(Debug, Clone, Default)]
pub struct FileReadingLimits {
    /// Maximum tokens for file content.
    pub max_tokens: Option<i64>,
    /// Maximum file size in bytes.
    pub max_size_bytes: Option<i64>,
}

/// Glob search limits.
#[derive(Debug, Clone, Default)]
pub struct GlobLimits {
    /// Maximum number of glob results.
    pub max_results: Option<i32>,
}

/// A tool execution decision record.
#[derive(Debug, Clone)]
pub struct ToolDecision {
    pub source: String,
    pub decision: ToolDecisionKind,
    pub timestamp: i64,
}

/// Accept or reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolDecisionKind {
    Accept,
    Reject,
}

#[cfg(any(test, feature = "testing"))]
use coco_types::PermissionMode;

impl ToolUseContext {
    /// Clone the context for use in concurrent tool execution.
    ///
    /// Shares Arc-wrapped state (messages, in_progress IDs, app_state, denial tracking)
    /// while cloning value types. Concurrent tools are read-only and self-contained,
    /// so they only need the shared references and config.
    pub fn clone_for_concurrent(&self) -> Self {
        Self {
            tools: self.tools.clone(),
            main_loop_model: self.main_loop_model.clone(),
            thinking_level: self.thinking_level.clone(),
            is_non_interactive: self.is_non_interactive,
            max_budget_usd: self.max_budget_usd,
            custom_system_prompt: self.custom_system_prompt.clone(),
            append_system_prompt: self.append_system_prompt.clone(),
            debug: self.debug,
            verbose: self.verbose,
            cancel: self.cancel.clone(),
            messages: self.messages.clone(),
            permission_context: self.permission_context.clone(),
            tool_use_id: None, // each concurrent tool gets its own ID
            user_message_id: self.user_message_id.clone(),
            agent_id: self.agent_id.clone(),
            agent_type: self.agent_type.clone(),
            file_reading_limits: self.file_reading_limits.clone(),
            glob_limits: self.glob_limits.clone(),
            // Share both trigger sets across concurrent siblings so all
            // pushes from the batch land in one place for app/query to
            // drain. See field docs on the struct.
            nested_memory_attachment_triggers: self.nested_memory_attachment_triggers.clone(),
            loaded_nested_memory_paths: HashSet::new(),
            dynamic_skill_dir_triggers: self.dynamic_skill_dir_triggers.clone(),
            discovered_skill_names: HashSet::new(),
            tool_decisions: HashMap::new(),
            is_teammate: self.is_teammate,
            plan_mode_required: self.plan_mode_required,
            agent_name: self.agent_name.clone(),
            team_name: self.team_name.clone(),
            plan_verify_execution: self.plan_verify_execution,
            user_modified: false,
            require_can_use_tool: self.require_can_use_tool,
            preserve_tool_use_results: self.preserve_tool_use_results,
            rendered_system_prompt: self.rendered_system_prompt.clone(),
            critical_system_reminder: self.critical_system_reminder.clone(),
            in_progress_tool_use_ids: self.in_progress_tool_use_ids.clone(),
            side_query: self.side_query.clone(),
            mcp: self.mcp.clone(),
            schedules: self.schedules.clone(),
            agent: self.agent.clone(),
            mailbox: self.mailbox.clone(),
            cwd_override: self.cwd_override.clone(),
            permission_bridge: self.permission_bridge.clone(),
            progress_tx: self.progress_tx.clone(),
            task_handle: self.task_handle.clone(),
            hook_handle: self.hook_handle.clone(),
            file_read_state: self.file_read_state.clone(),
            file_history: self.file_history.clone(),
            config_home: self.config_home.clone(),
            session_id_for_history: self.session_id_for_history.clone(),
            plans_dir: self.plans_dir.clone(),
            app_state: self.app_state.clone(),
            local_denial_tracking: self.local_denial_tracking.clone(),
            query_chain_id: self.query_chain_id.clone(),
            query_depth: self.query_depth,
        }
    }

    /// Create a minimal context for testing.
    #[cfg(any(test, feature = "testing"))]
    pub fn test_default() -> Self {
        Self {
            tools: Arc::new(ToolRegistry::new()),
            main_loop_model: "test-model".into(),
            thinking_level: None,
            is_non_interactive: false,
            max_budget_usd: None,
            custom_system_prompt: None,
            append_system_prompt: None,
            debug: false,
            verbose: false,
            cancel: CancellationToken::new(),
            messages: Arc::new(RwLock::new(Vec::new())),
            permission_context: ToolPermissionContext {
                mode: PermissionMode::BypassPermissions,
                additional_dirs: HashMap::new(),
                allow_rules: HashMap::new(),
                deny_rules: HashMap::new(),
                ask_rules: HashMap::new(),
                bypass_available: true,
                pre_plan_mode: None,
                stripped_dangerous_rules: None,
                session_plan_file: None,
            },
            tool_use_id: None,
            user_message_id: None,
            agent_id: None,
            agent_type: None,
            file_reading_limits: FileReadingLimits::default(),
            glob_limits: GlobLimits::default(),
            nested_memory_attachment_triggers: Arc::new(RwLock::new(HashSet::new())),
            loaded_nested_memory_paths: HashSet::new(),
            dynamic_skill_dir_triggers: Arc::new(RwLock::new(HashSet::new())),
            discovered_skill_names: HashSet::new(),
            tool_decisions: HashMap::new(),
            is_teammate: false,
            plan_mode_required: false,
            agent_name: None,
            team_name: None,
            plan_verify_execution: false,
            user_modified: false,
            require_can_use_tool: false,
            preserve_tool_use_results: false,
            rendered_system_prompt: None,
            critical_system_reminder: None,
            in_progress_tool_use_ids: Arc::new(RwLock::new(HashSet::new())),
            side_query: Arc::new(crate::side_query::NoOpSideQuery),
            mcp: Arc::new(crate::mcp_handle::NoOpMcpHandle),
            schedules: Arc::new(crate::schedule_store::NoOpScheduleStore),
            agent: Arc::new(crate::agent_handle::NoOpAgentHandle),
            mailbox: Arc::new(crate::NoOpMailboxHandle),
            cwd_override: None,
            permission_bridge: None,
            progress_tx: None,
            task_handle: None,
            hook_handle: None,
            file_read_state: None,
            file_history: None,
            config_home: None,
            session_id_for_history: None,
            plans_dir: None,
            app_state: None,
            local_denial_tracking: None,
            query_chain_id: None,
            query_depth: 0,
        }
    }
}
