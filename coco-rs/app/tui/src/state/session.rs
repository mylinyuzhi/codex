//! Session state — agent-synchronized data.
//!
//! Updated by server notification handlers when the agent loop emits events.

use std::collections::VecDeque;
use std::time::Instant;

use coco_types::IdeDiagnosticsUpdatedParams;
use coco_types::IdeSelectionChangedParams;
use coco_types::PermissionMode;

/// Agent-synchronized session state.
#[derive(Debug)]
pub struct SessionState {
    /// Conversation messages.
    pub messages: Vec<ChatMessage>,
    /// Active model name.
    pub model: String,
    /// Current permission mode. Plan-mode status is derived from this
    /// (`permission_mode == Plan`) — no separate bool.
    pub permission_mode: PermissionMode,
    /// Whether the current session may cycle into `BypassPermissions`.
    /// Gate flag for [`PermissionMode::next_in_cycle`]; set by the
    /// runtime based on auth + settings.
    pub bypass_permissions_available: bool,
    /// Whether the classifier-backed `Auto` mode is available.
    /// Gate flag for [`PermissionMode::next_in_cycle`]; set by the
    /// runtime based on feature flags.
    pub auto_mode_available: bool,
    /// Active tool executions.
    pub tool_executions: Vec<ToolExecution>,
    /// Subagent instances.
    pub subagents: Vec<SubagentInstance>,
    /// Token usage.
    pub token_usage: TokenUsage,
    /// Session identifier.
    pub session_id: Option<String>,
    /// Working directory.
    pub working_dir: Option<String>,
    /// Turn counter.
    pub turn_count: i32,
    /// Context window usage.
    pub context_window_used: i32,
    /// Context window total capacity.
    pub context_window_total: i32,
    /// Estimated cost in cents.
    pub estimated_cost_cents: i32,
    /// Whether fast mode is active.
    pub fast_mode: bool,
    /// Whether agent is currently busy.
    busy: bool,
    /// Fallback model name (shown when model switches).
    pub fallback_model: Option<String>,
    /// Whether compaction is in progress.
    pub is_compacting: bool,
    /// Connected MCP servers.
    pub mcp_servers: Vec<McpServerStatus>,
    /// Focused subagent index for side panel.
    pub focused_subagent_index: Option<i32>,
    /// Current turn number (within multi-turn loop).
    pub current_turn_number: Option<i32>,
    /// Queued commands for mid-turn injection.
    pub queued_commands: VecDeque<String>,
    /// Available models for model picker.
    pub available_models: Vec<String>,
    /// Whether file checkpointing is enabled for rewind.
    /// Set by the orchestrator (tui_runner) at startup.
    pub file_history_enabled: bool,
    /// Whether the last turn was user-interrupted (for auto-restore).
    /// TS: abortController.signal.reason === 'user-cancel'
    pub was_interrupted: bool,
    /// Available slash commands for command palette.
    pub available_commands: Vec<(String, Option<String>)>,
    /// Available agents for `@agent-*` autocomplete. Populated by the
    /// session handler when the agent registry is loaded; used synchronously
    /// by `autocomplete::refresh_suggestions` for the Agent trigger.
    pub available_agents: Vec<crate::autocomplete::AgentInfo>,
    /// Saved sessions for session browser.
    pub saved_sessions: Vec<SavedSession>,

    // === WS-3: new fields for full event coverage ===
    /// Session state visible to SDK consumers (idle/running/requires_action).
    pub session_state: coco_types::SessionState,
    /// Active worktree path (set by WorktreeEntered, cleared by WorktreeExited).
    pub worktree_path: Option<String>,
    /// Model fallback banner message (set by ModelFallbackStarted, cleared on Completed).
    pub model_fallback_banner: Option<String>,
    /// Rate limit status (set by RateLimit notification).
    pub rate_limit_info: Option<RateLimitInfo>,
    /// Context usage percentage (set by ContextUsageWarning).
    pub context_usage_percent: Option<f64>,
    /// Sandbox active state (set by SandboxStateChanged).
    pub sandbox_active: bool,
    /// Stream health: stall detected (set by StreamStallDetected, cleared on next turn).
    pub stream_stall: bool,
    /// Active background tasks (set by TaskStarted, updated by TaskProgress/Completed).
    pub active_tasks: Vec<TaskEntry>,
    /// Active hook executions (set by HookStarted, updated by HookProgress/Response).
    pub active_hooks: Vec<HookEntry>,
    /// Prompt suggestions from the model (set by PromptSuggestion).
    pub prompt_suggestions: Vec<String>,
    /// Local command output lines (set by LocalCommandOutput, capped at 50).
    pub local_command_output: VecDeque<String>,
    /// Available output styles for picker (set by OutputStylesReady).
    pub available_output_styles: Vec<String>,
    /// Available plugins for picker (set by PluginDataReady).
    pub available_plugins: Vec<serde_json::Value>,
    /// Raw markdown of the most recent completed agent response. Populated on
    /// `TurnCompleted` and consumed by the `/copy` / Ctrl+O flow. Cleared when
    /// a new session is configured. See `record_agent_markdown()`.
    pub last_agent_markdown: Option<String>,
    /// Latest IDE selection (set by IdeSelectionChanged, replaces prior value).
    pub ide_selection: Option<IdeSelectionChangedParams>,
    /// Latest IDE diagnostics update (set by IdeDiagnosticsUpdated, replaces prior value).
    pub ide_diagnostics: Option<IdeDiagnosticsUpdatedParams>,
}

impl SessionState {
    /// Add a chat message.
    pub fn add_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
    }

    /// Get the last message.
    pub fn last_message(&self) -> Option<&ChatMessage> {
        self.messages.last()
    }

    /// Whether the agent is busy.
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Set busy state.
    pub fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    /// Record the raw markdown of the current agent turn for the `/copy`
    /// / Ctrl+O flow. Mirrors codex-rs `record_agent_markdown()`.
    pub fn record_agent_markdown(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.last_agent_markdown = Some(text.to_string());
    }

    /// Update token usage.
    pub fn update_tokens(&mut self, usage: TokenUsage) {
        self.token_usage = usage;
    }

    /// Queue a tool execution (called from ToolUseQueued).
    pub fn start_tool(&mut self, call_id: String, name: String) {
        self.tool_executions.push(ToolExecution {
            call_id,
            name,
            status: ToolStatus::Queued,
            started_at: Instant::now(),
            completed_at: None,
            description: None,
            streaming_input: None,
        });
    }

    /// Transition a queued tool to running (called from ToolUseStarted).
    pub fn run_tool(&mut self, call_id: &str) {
        if let Some(tool) = self
            .tool_executions
            .iter_mut()
            .find(|t| t.call_id == call_id)
        {
            tool.status = ToolStatus::Running;
        } else {
            tracing::debug!(call_id, "run_tool: tool not found in tool_executions");
        }
    }

    /// Complete a tool execution.
    pub fn complete_tool(&mut self, call_id: &str, is_error: bool) {
        if let Some(tool) = self
            .tool_executions
            .iter_mut()
            .find(|t| t.call_id == call_id)
        {
            tool.status = if is_error {
                ToolStatus::Failed
            } else {
                ToolStatus::Completed
            };
            tool.completed_at = Some(Instant::now());
        } else {
            tracing::debug!(call_id, "complete_tool: tool not found in tool_executions");
        }
    }

    /// Count of connected MCP servers.
    pub fn connected_mcp_count(&self) -> i32 {
        self.mcp_servers.iter().filter(|s| s.connected).count() as i32
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            model: String::new(),
            permission_mode: PermissionMode::Default,
            bypass_permissions_available: false,
            auto_mode_available: false,
            tool_executions: Vec::new(),
            subagents: Vec::new(),
            token_usage: TokenUsage::default(),
            session_id: None,
            working_dir: None,
            turn_count: 0,
            context_window_used: 0,
            context_window_total: 0,
            estimated_cost_cents: 0,
            fast_mode: false,
            busy: false,
            fallback_model: None,
            is_compacting: false,
            mcp_servers: Vec::new(),
            focused_subagent_index: None,
            current_turn_number: None,
            queued_commands: VecDeque::new(),
            available_models: Vec::new(),
            file_history_enabled: false,
            was_interrupted: false,
            available_commands: Vec::new(),
            available_agents: Vec::new(),
            saved_sessions: Vec::new(),
            session_state: coco_types::SessionState::Idle,
            worktree_path: None,
            model_fallback_banner: None,
            rate_limit_info: None,
            context_usage_percent: None,
            sandbox_active: false,
            stream_stall: false,
            active_tasks: Vec::new(),
            active_hooks: Vec::new(),
            prompt_suggestions: Vec::new(),
            local_command_output: VecDeque::new(),
            available_output_styles: Vec::new(),
            available_plugins: Vec::new(),
            last_agent_markdown: None,
            ide_selection: None,
            ide_diagnostics: None,
        }
    }
}

/// A rendered chat message.
/// A rendered chat message — rich enum matching TS's 30+ message types.
///
/// TS: src/components/messages/ (41 files)
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub content: MessageContent,
    pub is_meta: bool,
    /// Permission mode active when this message was created (for rewind restoration).
    /// TS: UserMessage.permissionMode in messages.ts
    pub permission_mode: Option<PermissionMode>,
}

/// Message author role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}

/// Rich message content variants aligned with TS component types.
#[derive(Debug, Clone)]
pub enum MessageContent {
    // ── User messages (TS: 15 types) ──
    /// Plain text from user.
    Text(String),
    /// User image attachment.
    Image { path: String },
    /// User bash input command.
    BashInput { command: String },
    /// User bash output display.
    BashOutput { output: String, exit_code: i32 },
    /// Plan mode entry/exit marker.
    PlanMarker { action: PlanAction },
    /// Memory update content.
    MemoryInput { content: String },
    /// Agent notification summary.
    AgentNotification { agent_id: String, summary: String },
    /// Teammate message.
    TeammateMessage { teammate: String, content: String },
    /// Attachment display.
    Attachment {
        attachment_type: String,
        preview: String,
    },
    /// Channel-scoped message (e.g., a plugin Slack/Discord bridge).
    /// TS: UserChannelMessage.tsx — parses `<channel source user>body</channel>`.
    ChannelMessage {
        source: String,
        user: Option<String>,
        content: String,
    },
    /// MCP resource or tool-polling update notification.
    /// TS: UserResourceUpdateMessage.tsx — parses `<mcp-resource-update ...>`
    /// and `<mcp-polling-update ...>` blocks.
    ResourceUpdate {
        kind: ResourceUpdateKind,
        server: String,
        target: String,
        reason: Option<String>,
    },

    // ── Assistant messages (TS: 5 types) ──
    /// Assistant text response (rendered as markdown).
    AssistantText(String),
    /// Extended thinking content (collapsible).
    Thinking {
        content: String,
        duration_ms: Option<i64>,
    },
    /// Redacted thinking block.
    RedactedThinking,
    /// Tool use invocation display.
    ToolUse {
        tool_name: String,
        call_id: String,
        input_preview: String,
        status: ToolUseStatus,
    },

    // ── Tool results (TS: 7 types) ──
    /// Successful tool result.
    ToolSuccess { tool_name: String, output: String },
    /// Tool execution error.
    ToolError { tool_name: String, error: String },
    /// Tool use rejected by user.
    ToolRejected { tool_name: String, reason: String },
    /// Tool use canceled by user.
    ToolCanceled { tool_name: String },
    /// File edit with diff.
    FileEditDiff {
        path: String,
        diff: String,
        old_content: Option<String>,
        new_content: Option<String>,
    },
    /// File write result.
    FileWriteResult { path: String, bytes_written: i64 },

    // ── System messages (TS: 8 types) ──
    /// System text notice.
    SystemText(String),
    /// API error with retry info.
    ApiError {
        error: String,
        retryable: bool,
        status_code: Option<i32>,
    },
    /// Rate limit notification.
    RateLimit {
        message: String,
        resets_at: Option<i64>,
    },
    /// Shutdown notice.
    Shutdown { reason: String },
    /// Teammate shutdown request.
    ShutdownRequest {
        from: String,
        reason: Option<String>,
    },
    /// Teammate shutdown rejected.
    ShutdownRejected { from: String, reason: String },
    /// Hook completed successfully.
    HookSuccess { hook_name: String, output: String },
    /// Hook failed with a non-blocking error.
    HookNonBlockingError { hook_name: String, error: String },
    /// Hook failed with a blocking error that prevents continuation.
    HookBlockingError {
        hook_name: String,
        error: String,
        command: String,
    },
    /// Hook was cancelled.
    HookCancelled { hook_name: String },
    /// Hook emitted a system message.
    HookSystemMessage { hook_name: String, message: String },
    /// Hook provided additional context.
    HookAdditionalContext { hook_name: String, context: String },
    /// Hook stopped continuation with a reason.
    HookStoppedContinuation { hook_name: String, reason: String },
    /// Hook completed asynchronously.
    HookAsyncResponse { hook_name: String, output: String },
    /// Plan approval request.
    PlanApproval { plan: String, request_id: String },
    /// Compaction boundary marker.
    CompactBoundary,
    /// Advisor message from coordinator agent.
    Advisor { advisor_id: String, content: String },
    /// Task assignment notification.
    TaskAssignment {
        task_id: String,
        assignee: String,
        description: String,
    },
}

/// Plan mode action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanAction {
    Enter,
    Exit,
}

/// MCP update notification kind.
///
/// TS: UserResourceUpdateMessage.tsx distinguishes `<mcp-resource-update>`
/// (resource content changed — e.g. a file or DB row) from
/// `<mcp-polling-update>` (a polled tool's output changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceUpdateKind {
    /// Resource changed (URI-addressed).
    Resource,
    /// Polled tool output changed.
    Polling,
}

/// Tool use inline status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolUseStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

impl ChatMessage {
    /// Create a simple user text message.
    pub fn user_text(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: ChatRole::User,
            content: MessageContent::Text(text.into()),
            is_meta: false,
            permission_mode: None,
        }
    }

    /// Create a simple assistant text message.
    pub fn assistant_text(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: ChatRole::Assistant,
            content: MessageContent::AssistantText(text.into()),
            is_meta: false,
            permission_mode: None,
        }
    }

    /// Create a system text message.
    pub fn system_text(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: ChatRole::System,
            content: MessageContent::SystemText(text.into()),
            is_meta: false,
            permission_mode: None,
        }
    }

    /// Create a tool success result.
    pub fn tool_success(
        id: impl Into<String>,
        tool_name: impl Into<String>,
        output: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            role: ChatRole::Tool,
            content: MessageContent::ToolSuccess {
                tool_name: tool_name.into(),
                output: output.into(),
            },
            is_meta: false,
            permission_mode: None,
        }
    }

    /// Create a tool error result.
    pub fn tool_error(
        id: impl Into<String>,
        tool_name: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            role: ChatRole::Tool,
            content: MessageContent::ToolError {
                tool_name: tool_name.into(),
                error: error.into(),
            },
            is_meta: false,
            permission_mode: None,
        }
    }

    /// Get the text content for simple display.
    pub fn text_content(&self) -> &str {
        match &self.content {
            MessageContent::Text(s)
            | MessageContent::AssistantText(s)
            | MessageContent::SystemText(s) => s,
            MessageContent::BashInput { command } => command,
            MessageContent::BashOutput { output, .. } => output,
            MessageContent::ToolSuccess { output, .. } => output,
            MessageContent::ToolError { error, .. } => error,
            MessageContent::ToolRejected { reason, .. } => reason,
            MessageContent::ToolCanceled { tool_name } => tool_name,
            MessageContent::Thinking { content, .. } => content,
            MessageContent::MemoryInput { content } => content,
            MessageContent::HookSuccess { output, .. }
            | MessageContent::HookAsyncResponse { output, .. } => output,
            MessageContent::HookNonBlockingError { error, .. }
            | MessageContent::HookBlockingError { error, .. } => error,
            MessageContent::HookCancelled { hook_name } => hook_name,
            MessageContent::HookSystemMessage { message, .. } => message,
            MessageContent::HookAdditionalContext { context, .. } => context,
            MessageContent::HookStoppedContinuation { reason, .. } => reason,
            MessageContent::PlanApproval { plan, .. } => plan,
            MessageContent::RateLimit { message, .. } => message,
            MessageContent::ApiError { error, .. } => error,
            MessageContent::Shutdown { reason } => reason,
            MessageContent::ShutdownRequest { from, .. } => from,
            MessageContent::ShutdownRejected { reason, .. } => reason,
            MessageContent::FileEditDiff { diff, .. } => diff,
            MessageContent::FileWriteResult { path, .. } => path,
            MessageContent::AgentNotification { summary, .. } => summary,
            MessageContent::TeammateMessage { content, .. } => content,
            MessageContent::Attachment { preview, .. } => preview,
            MessageContent::Image { path } => path,
            MessageContent::ToolUse { input_preview, .. } => input_preview,
            MessageContent::RedactedThinking => "[redacted]",
            MessageContent::PlanMarker { .. } => "",
            MessageContent::CompactBoundary => "---",
            MessageContent::Advisor { content, .. } => content,
            MessageContent::TaskAssignment { description, .. } => description,
            MessageContent::ChannelMessage { content, .. } => content,
            MessageContent::ResourceUpdate { target, .. } => target,
        }
    }
}

/// Tool execution tracking.
#[derive(Debug, Clone)]
pub struct ToolExecution {
    pub call_id: String,
    pub name: String,
    pub status: ToolStatus,
    pub started_at: Instant,
    /// When the tool reached a terminal status (Completed or Failed). Set by
    /// `complete_tool()` so `elapsed()` freezes after completion instead of
    /// continuing to grow while the message stays in the transcript.
    pub completed_at: Option<Instant>,
    pub description: Option<String>,
    /// Streaming tool input delta (typing effect for bash/powershell).
    pub streaming_input: Option<String>,
}

impl ToolExecution {
    /// Elapsed time between start and terminal status (or now, if still running).
    pub fn elapsed(&self) -> std::time::Duration {
        match self.completed_at {
            Some(end) => end.duration_since(self.started_at),
            None => self.started_at.elapsed(),
        }
    }
}

/// Tool execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

/// Subagent instance tracking.
#[derive(Debug, Clone)]
pub struct SubagentInstance {
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    pub status: SubagentStatus,
    pub color: Option<String>,
}

/// Subagent lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatus {
    Running,
    Completed,
    Backgrounded,
    Failed,
}

/// Token usage counters.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
}

/// MCP server connection status.
#[derive(Debug, Clone)]
pub struct McpServerStatus {
    pub name: String,
    pub connected: bool,
    pub tool_count: i32,
}

/// Saved session metadata for the session browser.
#[derive(Debug, Clone)]
pub struct SavedSession {
    pub id: String,
    pub label: String,
    pub message_count: i32,
    pub created_at: String,
    pub model: Option<String>,
}

/// Rate limit info from the last RateLimit notification.
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    pub remaining: Option<i64>,
    pub reset_at: Option<i64>,
    pub provider: Option<String>,
}

/// Background task entry for the task panel.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub task_id: String,
    pub description: String,
    pub status: TaskEntryStatus,
}

/// Task entry lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskEntryStatus {
    Running,
    Completed,
    Failed,
    Stopped,
}

/// Hook execution entry for the hook panel.
#[derive(Debug, Clone)]
pub struct HookEntry {
    pub hook_id: String,
    pub hook_name: String,
    pub status: HookEntryStatus,
    pub output: Option<String>,
}

/// Hook entry lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEntryStatus {
    Running,
    Completed,
    Failed,
}
