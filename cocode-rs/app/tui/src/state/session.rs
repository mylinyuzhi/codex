//! Session state from the agent.
//!
//! This module contains state that comes from or is synchronized with
//! the core agent loop.

use std::path::PathBuf;
use std::time::Instant;

use cocode_protocol::AgentProgress;
use cocode_protocol::RoleSelection;
use cocode_protocol::TokenUsage;
use cocode_protocol::UserQueuedCommand;

/// State synchronized with the agent session.
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    /// Messages in the conversation.
    pub messages: Vec<ChatMessage>,

    /// Current model + thinking level selection.
    pub current_selection: Option<RoleSelection>,

    /// Whether plan mode is active.
    pub plan_mode: bool,

    /// Current permission mode (Default/AcceptEdits/Plan/Bypass/DontAsk).
    pub permission_mode: cocode_protocol::PermissionMode,

    /// Current phase in plan mode (if active).
    pub plan_phase: Option<PlanPhase>,

    /// Path to the plan file (when in plan mode).
    pub plan_file: Option<PathBuf>,

    /// Active tool executions.
    pub tool_executions: Vec<ToolExecution>,

    /// Active subagent instances.
    pub subagents: Vec<SubagentInstance>,

    /// Active team info (if agent is part of a team).
    pub team: Option<TeamInfo>,

    /// Total token usage for the session.
    pub token_usage: TokenUsage,

    /// Session ID (if resuming).
    pub session_id: Option<String>,

    /// Thinking tokens used in the current turn.
    pub thinking_tokens_used: i32,

    /// Connected MCP servers.
    pub mcp_servers: Vec<McpServerStatus>,

    /// Fallback model being used (if model fallback occurred).
    pub fallback_model: Option<String>,

    /// Whether context compaction is in progress.
    pub is_compacting: bool,

    /// Queue of commands for steering injection (Enter during streaming).
    /// Commands are consumed once in the agent loop and injected as steering
    /// system-reminders (consume-then-remove pattern).
    pub queued_commands: Vec<UserQueuedCommand>,

    /// Current working directory from shell.
    pub working_dir: Option<String>,

    /// Number of completed turns.
    pub turn_count: i32,

    /// Context window tokens used.
    pub context_window_used: i32,

    /// Context window total capacity.
    pub context_window_total: i32,

    /// Estimated cost in cents.
    pub estimated_cost_cents: i32,

    /// Active output style name (for status bar display).
    pub output_style: Option<String>,

    /// Current turn number (set by TurnStarted, used for message tagging).
    pub current_turn_number: Option<i32>,

    /// Active worktree paths (count derived via `.len()`, branch via `.last()`).
    pub active_worktree_paths: Vec<WorktreeInfo>,

    /// Active background tasks.
    pub background_tasks: Vec<BackgroundTask>,

    /// Active MCP tool calls (tracked separately from regular tools).
    pub mcp_tool_calls: Vec<McpToolCall>,

    /// Whether fast mode is active.
    pub fast_mode: bool,

    /// Whether bypass-permissions mode is available for this session.
    ///
    /// Set to `true` when the session was launched with `--dangerously-skip-permissions`.
    /// Gates the ClearAndBypass option in the plan exit dialog and Bypass in mode cycling.
    pub bypass_available: bool,

    /// Index of the currently focused subagent in the panel (for quick-switch).
    pub focused_subagent_index: Option<i32>,

    /// Whether sandbox mode is active.
    pub sandbox_active: bool,

    /// Recent sandbox violation count (auto-clears after flash expires).
    pub sandbox_violation_count: i32,

    /// Deadline after which the violation count resets to 0.
    /// Set to `Instant::now() + 5s` on each new violation batch;
    /// new violations extend the deadline.
    pub sandbox_violation_flash_until: Option<Instant>,
}

impl SessionState {
    /// Add a message to the conversation.
    pub fn add_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
    }

    /// Get the last message, if any.
    pub fn last_message(&self) -> Option<&ChatMessage> {
        self.messages.last()
    }

    /// Get the last assistant message, if any.
    pub fn last_assistant_message(&self) -> Option<&ChatMessage> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant)
    }

    /// Update token usage from internal `TokenUsage`.
    pub fn update_tokens(&mut self, usage: TokenUsage) {
        self.token_usage.input_tokens += usage.input_tokens;
        self.token_usage.output_tokens += usage.output_tokens;
        if let Some(cache) = usage.cache_read_tokens {
            *self.token_usage.cache_read_tokens.get_or_insert(0) += cache;
        }
    }

    /// Update token usage from protocol `Usage`.
    pub fn update_tokens_from_protocol(&mut self, usage: &cocode_app_server_protocol::Usage) {
        self.token_usage.input_tokens += usage.input_tokens;
        self.token_usage.output_tokens += usage.output_tokens;
        if let Some(cache) = usage.cache_read_tokens {
            *self.token_usage.cache_read_tokens.get_or_insert(0) += cache;
        }
    }

    /// Reset thinking tokens used (call at start of turn).
    pub fn reset_thinking_tokens(&mut self) {
        self.thinking_tokens_used = 0;
    }

    /// Add thinking tokens used.
    pub fn add_thinking_tokens(&mut self, tokens: i32) {
        self.thinking_tokens_used += tokens;
    }

    /// Set the current plan phase.
    pub fn set_plan_phase(&mut self, phase: Option<PlanPhase>) {
        self.plan_phase = phase;
    }

    /// Add or update an MCP server status.
    pub fn update_mcp_server(&mut self, name: String, connected: bool, tool_count: i32) {
        if let Some(server) = self.mcp_servers.iter_mut().find(|s| s.name == name) {
            server.connected = connected;
            server.tool_count = tool_count;
        } else {
            self.mcp_servers
                .push(McpServerStatus::new(name, connected, tool_count));
        }
    }

    /// Remove an MCP server.
    pub fn remove_mcp_server(&mut self, name: &str) {
        self.mcp_servers.retain(|s| s.name != name);
    }

    /// Get the count of connected MCP servers.
    pub fn connected_mcp_count(&self) -> i32 {
        self.mcp_servers.iter().filter(|s| s.connected).count() as i32
    }

    /// Start a tool execution.
    pub fn start_tool_with_batch(
        &mut self,
        call_id: String,
        name: String,
        batch_id: Option<String>,
    ) {
        self.tool_executions.push(ToolExecution {
            call_id,
            name,
            status: ToolStatus::Running,
            progress: None,
            output: None,
            started_at: Some(Instant::now()),
            elapsed: None,
            batch_id,
        });
    }

    /// Update tool progress.
    pub fn update_tool_progress(&mut self, call_id: &str, progress: String) {
        if let Some(tool) = self
            .tool_executions
            .iter_mut()
            .find(|t| t.call_id == call_id)
        {
            tool.progress = Some(progress);
        }
    }

    /// Complete a tool execution.
    pub fn complete_tool(&mut self, call_id: &str, output: String, is_error: bool) {
        if let Some(tool) = self
            .tool_executions
            .iter_mut()
            .find(|t| t.call_id == call_id)
        {
            tool.elapsed = tool.started_at.map(|t| t.elapsed());
            tool.status = if is_error {
                ToolStatus::Failed
            } else {
                ToolStatus::Completed
            };
            tool.output = Some(output);
        }
    }

    /// Remove completed tools older than a certain threshold.
    pub fn cleanup_completed_tools(&mut self, max_completed: usize) {
        let completed_count = self
            .tool_executions
            .iter()
            .filter(|t| matches!(t.status, ToolStatus::Completed | ToolStatus::Failed))
            .count();

        if completed_count > max_completed {
            let to_remove = completed_count - max_completed;
            let mut removed = 0;
            self.tool_executions.retain(|t| {
                if removed >= to_remove {
                    return true;
                }
                if matches!(t.status, ToolStatus::Completed | ToolStatus::Failed) {
                    removed += 1;
                    return false;
                }
                true
            });
        }
    }

    /// Look up a subagent's type name by ID, falling back to `"agent"`.
    pub fn subagent_type_name(&self, agent_id: &str) -> String {
        self.subagents
            .iter()
            .find(|s| s.id == agent_id)
            .map(|s| s.agent_type.clone())
            .unwrap_or_else(|| "agent".to_string())
    }

    /// Start a new subagent.
    pub fn start_subagent(
        &mut self,
        agent_id: String,
        agent_type: String,
        description: String,
        color: Option<String>,
    ) {
        self.subagents.push(SubagentInstance {
            id: agent_id,
            agent_type,
            description,
            status: SubagentStatus::Running,
            progress: None,
            result: None,
            output_file: None,
            color,
            started_at: Instant::now(),
        });
    }

    /// Update subagent progress.
    pub fn update_subagent_progress(&mut self, agent_id: &str, progress: AgentProgress) {
        if let Some(subagent) = self.subagents.iter_mut().find(|s| s.id == agent_id) {
            subagent.progress = Some(progress);
        }
    }

    /// Complete a subagent.
    pub fn complete_subagent(&mut self, agent_id: &str, result: String) {
        if let Some(subagent) = self.subagents.iter_mut().find(|s| s.id == agent_id) {
            subagent.status = SubagentStatus::Completed;
            subagent.result = Some(result);
        }
    }

    /// Mark a subagent as failed.
    pub fn fail_subagent(&mut self, agent_id: &str, error: String) {
        if let Some(subagent) = self.subagents.iter_mut().find(|s| s.id == agent_id) {
            subagent.status = SubagentStatus::Failed;
            subagent.result = Some(error);
        }
    }

    /// Kill a subagent (mark as killed by user action).
    pub fn kill_subagent(&mut self, agent_id: &str) {
        if let Some(subagent) = self.subagents.iter_mut().find(|s| s.id == agent_id) {
            subagent.status = SubagentStatus::Killed;
        }
    }

    /// Move a subagent to background.
    pub fn background_subagent(&mut self, agent_id: &str, output_file: PathBuf) {
        if let Some(subagent) = self.subagents.iter_mut().find(|s| s.id == agent_id) {
            subagent.status = SubagentStatus::Backgrounded;
            subagent.output_file = Some(output_file);
        }
    }

    /// Focus the next subagent in the panel (wraps around).
    pub fn focus_next_subagent(&mut self) {
        if self.subagents.is_empty() {
            return;
        }
        let max = self.subagents.len() as i32 - 1;
        self.focused_subagent_index = Some(match self.focused_subagent_index {
            Some(i) if i < max => i + 1,
            _ => 0,
        });
    }

    /// Focus the previous subagent in the panel (wraps around).
    pub fn focus_prev_subagent(&mut self) {
        if self.subagents.is_empty() {
            return;
        }
        let max = self.subagents.len() as i32 - 1;
        self.focused_subagent_index = Some(match self.focused_subagent_index {
            Some(i) if i > 0 => i - 1,
            _ => max,
        });
    }

    /// Check if there are any running subagents.
    pub fn has_running_subagents(&self) -> bool {
        self.subagents
            .iter()
            .any(|s| s.status == SubagentStatus::Running)
    }

    /// Remove terminal subagents older than a certain threshold.
    pub fn cleanup_completed_subagents(&mut self, max_completed: usize) {
        let completed_count = self
            .subagents
            .iter()
            .filter(|s| s.status.is_terminal())
            .count();

        if completed_count > max_completed {
            let to_remove = completed_count - max_completed;
            let mut removed = 0;
            self.subagents.retain(|s| {
                if removed >= to_remove {
                    return true;
                }
                if s.status.is_terminal() {
                    removed += 1;
                    return false;
                }
                true
            });
            if let Some(idx) = self.focused_subagent_index
                && idx >= self.subagents.len() as i32
            {
                self.focused_subagent_index = if self.subagents.is_empty() {
                    None
                } else {
                    Some(self.subagents.len() as i32 - 1)
                };
            }
        }
    }

    // ========== Background Task Management ==========

    /// Start tracking a background task.
    pub fn start_background_task(&mut self, task_id: String, task_type: cocode_protocol::TaskType) {
        self.background_tasks.push(BackgroundTask {
            task_id,
            task_type,
            status: BackgroundTaskStatus::Running,
            progress: None,
            started_at: Instant::now(),
        });
    }

    /// Update progress of a background task.
    pub fn update_background_task_progress(&mut self, task_id: &str, message: String) {
        if let Some(task) = self
            .background_tasks
            .iter_mut()
            .find(|t| t.task_id == task_id)
        {
            task.progress = Some(message);
        }
    }

    /// Complete a background task.
    pub fn complete_background_task(&mut self, task_id: &str) {
        if let Some(task) = self
            .background_tasks
            .iter_mut()
            .find(|t| t.task_id == task_id)
        {
            task.status = BackgroundTaskStatus::Completed;
        }
    }

    /// Remove completed background tasks beyond `max_completed`.
    pub fn cleanup_completed_background_tasks(&mut self, max_completed: usize) {
        let completed_count = self
            .background_tasks
            .iter()
            .filter(|t| {
                matches!(
                    t.status,
                    BackgroundTaskStatus::Completed | BackgroundTaskStatus::Failed
                )
            })
            .count();

        if completed_count > max_completed {
            let to_remove = completed_count - max_completed;
            let mut removed = 0;
            self.background_tasks.retain(|t| {
                if removed >= to_remove {
                    return true;
                }
                if matches!(
                    t.status,
                    BackgroundTaskStatus::Completed | BackgroundTaskStatus::Failed
                ) {
                    removed += 1;
                    return false;
                }
                true
            });
        }
    }

    // ========== MCP Tool Call Management ==========

    /// Start tracking an MCP tool call.
    pub fn start_mcp_tool_call(&mut self, call_id: String, server: String, tool: String) {
        self.mcp_tool_calls.push(McpToolCall {
            call_id,
            server,
            tool,
            status: ToolStatus::Running,
            started_at: Instant::now(),
        });
    }

    /// Complete an MCP tool call.
    pub fn complete_mcp_tool_call(&mut self, call_id: &str, is_error: bool) {
        if let Some(call) = self
            .mcp_tool_calls
            .iter_mut()
            .find(|c| c.call_id == call_id)
        {
            call.status = if is_error {
                ToolStatus::Failed
            } else {
                ToolStatus::Completed
            };
        }
    }

    /// Remove completed MCP tool calls beyond `max_completed`.
    pub fn cleanup_completed_mcp_calls(&mut self, max_completed: usize) {
        let completed_count = self
            .mcp_tool_calls
            .iter()
            .filter(|c| matches!(c.status, ToolStatus::Completed | ToolStatus::Failed))
            .count();

        if completed_count > max_completed {
            let to_remove = completed_count - max_completed;
            let mut removed = 0;
            self.mcp_tool_calls.retain(|c| {
                if removed >= to_remove {
                    return true;
                }
                if matches!(c.status, ToolStatus::Completed | ToolStatus::Failed) {
                    removed += 1;
                    return false;
                }
                true
            });
        }
    }

    // ========== Queue Management ==========

    /// Queue a visible command for later processing (Enter during streaming).
    ///
    /// Returns the command ID.
    pub fn queue_command(&mut self, prompt: impl Into<String>) -> String {
        let cmd = UserQueuedCommand::new(prompt);
        let id = cmd.id.clone();
        self.queued_commands.push(cmd);
        id
    }

    /// Dequeue the next command to process.
    pub fn dequeue_command(&mut self) -> Option<UserQueuedCommand> {
        if self.queued_commands.is_empty() {
            None
        } else {
            Some(self.queued_commands.remove(0))
        }
    }

    /// Get the number of queued commands.
    pub fn queued_count(&self) -> i32 {
        self.queued_commands.len() as i32
    }

    /// Clear all queued commands.
    pub fn clear_queues(&mut self) {
        self.queued_commands.clear();
    }

    /// Check if there are any queued commands.
    pub fn has_queued_items(&self) -> bool {
        !self.queued_commands.is_empty()
    }
}

/// Status of a background task tracked by the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundTaskStatus {
    /// Task is currently running.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed.
    Failed,
}

/// A background task being tracked in the TUI.
#[derive(Debug, Clone)]
pub struct BackgroundTask {
    /// Task identifier.
    pub task_id: String,
    /// Type of task (for display).
    pub task_type: cocode_protocol::TaskType,
    /// Current status.
    pub status: BackgroundTaskStatus,
    /// Progress message (if available).
    pub progress: Option<String>,
    /// When this task started.
    pub started_at: Instant,
}

/// An MCP tool call being tracked in the TUI.
#[derive(Debug, Clone)]
pub struct McpToolCall {
    /// Call identifier.
    pub call_id: String,
    /// Server name.
    pub server: String,
    /// Tool name.
    pub tool: String,
    /// Current status.
    pub status: ToolStatus,
    /// When this call started.
    pub started_at: Instant,
}

/// A message in the conversation.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// Unique identifier for this message.
    pub id: String,

    /// Role of the message sender.
    pub role: MessageRole,

    /// Content of the message.
    pub content: String,

    /// Whether this message is still being streamed.
    pub streaming: bool,

    /// Thinking content (if applicable).
    pub thinking: Option<String>,

    /// Inline tool calls associated with this message.
    pub tool_calls: Vec<InlineToolCall>,

    /// Turn number this message belongs to (for precise rewind).
    pub turn_number: Option<i32>,

    /// Whether this is a meta message (hidden from chat, visible to model).
    /// System reminders and injected context use this flag.
    pub is_meta: bool,

    /// Category label for meta messages (e.g., "ChangedFiles", "PlanMode").
    pub meta_category: Option<String>,
}

/// An inline tool call displayed within a chat message.
#[derive(Debug, Clone)]
pub struct InlineToolCall {
    /// Tool name (e.g., "Bash", "Read", "Edit").
    pub tool_name: String,
    /// Current status of the tool call.
    pub status: ToolStatus,
    /// Short description (e.g., "ls -la src/" or "src/main.rs").
    pub description: String,
    /// How long the tool took (for completed tools).
    pub elapsed: Option<std::time::Duration>,
    /// Batch ID for parallel execution grouping.
    pub batch_id: Option<String>,
}

impl ChatMessage {
    /// Create a new user message.
    pub fn user(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: MessageRole::User,
            content: content.into(),
            streaming: false,
            thinking: None,
            tool_calls: Vec::new(),
            turn_number: None,
            is_meta: false,
            meta_category: None,
        }
    }

    /// Create a new assistant message.
    pub fn assistant(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: MessageRole::Assistant,
            content: content.into(),
            streaming: false,
            thinking: None,
            tool_calls: Vec::new(),
            turn_number: None,
            is_meta: false,
            meta_category: None,
        }
    }

    /// Create a new system message (e.g., errors, notifications).
    pub fn system(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: MessageRole::System,
            content: content.into(),
            streaming: false,
            thinking: None,
            tool_calls: Vec::new(),
            turn_number: None,
            is_meta: false,
            meta_category: None,
        }
    }

    /// Create a new streaming assistant message.
    pub fn streaming_assistant(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: MessageRole::Assistant,
            content: String::new(),
            streaming: true,
            thinking: None,
            tool_calls: Vec::new(),
            turn_number: None,
            is_meta: false,
            meta_category: None,
        }
    }

    /// Set the meta category for this message.
    pub fn with_meta_category(mut self, category: impl Into<String>) -> Self {
        self.meta_category = Some(category.into());
        self
    }

    /// Append content to the message.
    pub fn append(&mut self, delta: &str) {
        self.content.push_str(delta);
    }

    /// Append thinking content.
    pub fn append_thinking(&mut self, delta: &str) {
        self.thinking
            .get_or_insert_with(String::new)
            .push_str(delta);
    }

    /// Mark the message as complete (no longer streaming).
    pub fn complete(&mut self) {
        self.streaming = false;
    }
}

/// Role of a message sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    /// User (human) message.
    User,
    /// Assistant (AI) message.
    Assistant,
    /// System message.
    System,
}

/// Status of a tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// Tool is currently running.
    Running,
    /// Tool completed successfully.
    Completed,
    /// Tool failed with an error.
    Failed,
}

/// Information about an active worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree directory.
    pub path: String,
    /// Git branch associated with the worktree.
    pub branch: String,
}

/// A tool execution in progress or completed.
#[derive(Debug, Clone)]
pub struct ToolExecution {
    /// Call identifier.
    pub call_id: String,
    /// Tool name.
    pub name: String,
    /// Current status.
    pub status: ToolStatus,
    /// Progress message (if available).
    pub progress: Option<String>,
    /// Output (when completed).
    pub output: Option<String>,
    /// When this tool started executing.
    pub started_at: Option<Instant>,
    /// How long the tool took (set on completion).
    pub elapsed: Option<std::time::Duration>,
    /// Batch ID for parallel execution grouping.
    pub batch_id: Option<String>,
}

/// Status of a subagent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatus {
    /// Subagent is currently running.
    Running,
    /// Subagent completed successfully.
    Completed,
    /// Subagent failed with an error.
    Failed,
    /// Subagent was moved to background.
    Backgrounded,
    /// Subagent was explicitly killed by user action.
    Killed,
}

impl SubagentStatus {
    /// Whether this status represents a terminal (finished) state.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed)
    }
}

/// Phase of plan mode workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanPhase {
    /// Understanding the requirements.
    Understanding,
    /// Designing the solution.
    Design,
    /// Reviewing existing code.
    Review,
    /// Creating the plan.
    Planning,
    /// Waiting for user approval.
    Approval,
}

impl PlanPhase {
    /// Get a short display name for this phase.
    pub fn display_name(&self) -> &'static str {
        match self {
            PlanPhase::Understanding => "Understanding",
            PlanPhase::Design => "Design",
            PlanPhase::Review => "Review",
            PlanPhase::Planning => "Planning",
            PlanPhase::Approval => "Approval",
        }
    }

    /// Get a short emoji indicator for this phase.
    pub fn emoji(&self) -> &'static str {
        match self {
            PlanPhase::Understanding => "🔍",
            PlanPhase::Design => "🎨",
            PlanPhase::Review => "📖",
            PlanPhase::Planning => "📝",
            PlanPhase::Approval => "✅",
        }
    }
}

/// Status of a connected MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerStatus {
    /// Name of the MCP server.
    pub name: String,
    /// Whether the server is connected.
    pub connected: bool,
    /// Number of tools provided by this server.
    pub tool_count: i32,
}

impl McpServerStatus {
    /// Create a new MCP server status.
    pub fn new(name: impl Into<String>, connected: bool, tool_count: i32) -> Self {
        Self {
            name: name.into(),
            connected,
            tool_count,
        }
    }
}

/// A subagent instance spawned by the main agent.
#[derive(Debug, Clone)]
pub struct SubagentInstance {
    /// Unique identifier for this subagent.
    pub id: String,
    /// Type of agent (e.g., "Explore", "Plan").
    pub agent_type: String,
    /// Short description of what the agent is doing.
    pub description: String,
    /// Current status.
    pub status: SubagentStatus,
    /// Progress information (if available).
    pub progress: Option<AgentProgress>,
    /// Result from the agent (when completed).
    pub result: Option<String>,
    /// Path to output file (when backgrounded).
    pub output_file: Option<PathBuf>,
    /// Display color from agent definition (for TUI rendering).
    pub color: Option<String>,
    /// When this subagent was spawned.
    pub started_at: Instant,
}

// ============================================================================
// Team Member State
// ============================================================================

/// Runtime status of a team member (mirrored from `cocode_team::MemberStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TeamMemberStatus {
    /// Agent is actively processing work.
    Active,
    /// Agent has finished current work and is awaiting new tasks.
    Idle,
    /// Shutdown has been requested but not yet completed.
    ShuttingDown,
    /// Agent has stopped executing.
    Stopped,
}

impl std::fmt::Display for TeamMemberStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Idle => write!(f, "idle"),
            Self::ShuttingDown => write!(f, "shutting down"),
            Self::Stopped => write!(f, "stopped"),
        }
    }
}

/// A team member entry for TUI display.
#[derive(Debug, Clone)]
pub struct TeamMemberEntry {
    /// Agent ID.
    pub agent_id: String,
    /// Display name (if set).
    pub name: Option<String>,
    /// Agent type.
    pub agent_type: Option<String>,
    /// Current status.
    pub status: TeamMemberStatus,
    /// Whether this is the team leader.
    pub is_leader: bool,
}

impl TeamMemberEntry {
    /// Get the display name (name if set, otherwise agent_id).
    pub fn display_name(&self) -> String {
        self.name.as_deref().unwrap_or(&self.agent_id).to_string()
    }
}

/// Team info for TUI display.
#[derive(Debug, Clone, Default)]
pub struct TeamInfo {
    /// Team name.
    pub name: String,
    /// Team members.
    pub members: Vec<TeamMemberEntry>,
}

#[cfg(test)]
#[path = "session.test.rs"]
mod tests;
