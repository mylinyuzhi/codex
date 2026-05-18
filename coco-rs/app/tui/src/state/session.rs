//! Session state — agent-synchronized data.
//!
//! Updated by server notification handlers when the agent loop emits events.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::time::Instant;

use coco_types::IdeDiagnosticsUpdatedParams;
use coco_types::IdeSelectionChangedParams;
use coco_types::ModelRole;
use coco_types::PermissionMode;
use coco_types::ReasoningEffort;

/// Provider configuration issue that makes all models under that
/// provider unavailable in the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderUnavailableReason {
    /// `base_url` is empty after config resolution.
    MissingBaseUrl,
    /// No API key was resolved from the configured env var or fallback
    /// `providers.<name>.api_key` value.
    MissingApiKey { env_key: String },
    /// The provider has no model rows visible to the picker.
    NoModels,
}

/// Session-frozen provider availability status used by `/model`.
#[derive(Debug, Clone, Default)]
pub struct ProviderStatus {
    /// Human-facing provider label used in picker section headers.
    pub provider_display: String,
    /// Empty means provider config is usable.
    pub unavailable_reasons: Vec<ProviderUnavailableReason>,
}

impl ProviderStatus {
    pub fn is_available(&self) -> bool {
        self.unavailable_reasons.is_empty()
    }
}

/// One (provider, model) entry in the TUI's session-frozen model
/// directory. Seeded from `RuntimeConfig.model_registry` (L0 builtin +
/// L1 `~/.coco/models.json` + L2 per-provider overrides) at session
/// start; the picker and `Ctrl+T` thinking cycle both consult this
/// snapshot.
///
/// The data is intentionally frozen for the session lifetime — model
/// metadata is a runtime-config concern, not a per-turn one. If the
/// user edits `~/.coco/models.json` mid-session they need to restart
/// to see the new entries (matches the rest of the runtime_config
/// snapshot policy).
#[derive(Debug, Clone)]
pub struct ModelCatalogEntry {
    /// Canonical provider id (e.g. `"anthropic"`, `"openai"`).
    pub provider: String,
    /// Human-facing provider label used in picker section headers.
    pub provider_display: String,
    /// Model id, e.g. `"claude-sonnet-4-6"`.
    pub model_id: String,
    /// Display name; falls back to `model_id` if unset upstream.
    pub display_name: String,
    /// Total context-window size (input + output) when known.
    pub context_window: Option<i64>,
    /// Efforts the model declares it supports, in declaration order.
    /// `Ctrl+T` cycles through this slice; the picker effort footer
    /// renders the same set.
    pub supported_efforts: Vec<ReasoningEffort>,
    /// Effort the model declares as its default when none is set.
    pub default_effort: Option<ReasoningEffort>,
}

/// UI-facing projection of a slash command. Re-exported from
/// `coco-types` so the same type can travel both on the
/// [`coco_types::TuiOnlyEvent::AvailableCommandsRefreshed`] wire and
/// inside [`SessionState`] without a conversion layer.
pub use coco_types::SlashCommandInfo;

/// Live binding of one [`ModelRole`] inside the TUI state. Mirrors
/// `SessionRuntime.role_overrides` but in display-friendly form so the
/// picker can mark "current" entries without an async hop.
#[derive(Debug, Clone)]
pub struct ModelBinding {
    pub model_id: String,
    pub provider: String,
    /// `None` ⇒ engine uses the model's `default_thinking_level`.
    pub effort: Option<ReasoningEffort>,
}

/// One queued steering command as rendered by the TUI footer.
///
/// Mirrors a single entry in the engine's
/// [`coco_query::CommandQueue`]; the `id` matches the
/// `CommandQueued{id}` / `CommandDequeued{id}` notifications so the
/// display can remove the right entry even when priority ordering or
/// agent scoping causes the engine to drain something other than the
/// queue front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedCommandDisplay {
    /// Stable identifier — `coco_query::QueuedCommand::id.to_string()`.
    pub id: String,
    /// Short preview of the queued prompt (caller-truncated; the
    /// engine builds it via `QueuedCommand::preview`).
    pub preview: String,
}

/// TUI-side label for the active compaction sub-phase. Built from
/// `coco_types::CompactionPhaseParams` so the renderer can pick a
/// localized spinner string without re-deriving it each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionPhaseLabel {
    /// "Running PreCompact hooks…"
    PreCompactHooks,
    /// "Running PostCompact hooks…"
    PostCompactHooks,
    /// "Running SessionStart hooks…"
    SessionStartHooks,
    /// "Compacting conversation"
    Summarizing,
}

/// Current Unix epoch in milliseconds (best-effort — clamps to 0
/// if the system clock is before the epoch, which only happens in
/// pathological setups).
///
/// `pub(crate)` so call sites outside this module (the protocol
/// handler that times subagent starts) reuse the same clamp instead
/// of open-coding `SystemTime::now()`.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Agent-synchronized session state.
#[derive(Debug)]
pub struct SessionState {
    /// Legacy TUI-local chat messages. Mostly vestigial after
    /// `engine-tui-unified-transcript-plan.md` Commits 2/3 — the
    /// engine `MessageHistory` is the source of truth and the TUI
    /// derives cells from `MessageAppended` events into
    /// [`Self::transcript`]. The few remaining direct writers
    /// (`protocol.rs::apply_reasoning_tokens_to_response` synthesising
    /// `MessageContent::Thinking` to carry duration / token metadata,
    /// `update.rs` Ctrl+L clear, the rewind-picker truncate) are kept
    /// for renderer-adapter convenience until a full `RenderedCell`
    /// rewrite of the renderer chain (plan §4/§5) replaces
    /// `ChatMessage` / `MessageContent` outright. All read paths
    /// should source from [`Self::transcript_messages`] — that view
    /// overlays engine cells on top of this slice.
    pub messages: Vec<ChatMessage>,
    /// Engine-authoritative view of `MessageHistory`, populated by the
    /// `MessageAppended` / `MessageTruncated` / `SessionResetForResume`
    /// protocol handlers. Engines push every message through
    /// `history_push_and_emit` so cells stay coherent with the
    /// JSONL transcript on disk — this is the source of truth for
    /// "what is in the conversation".
    pub transcript: super::transcript_view::TranscriptView,
    /// Message UUID set by `apply_auto_restore` when an auto-restore
    /// fires. The App loop drains this after each `handle_core_event`
    /// and dispatches `UserCommand::Rewind { mode: AutoRestore }` so
    /// the engine truncates its authoritative history and emits
    /// `ServerNotification::MessageTruncated`. Keeps engine ↔ TUI ↔
    /// SDK converged on a single truncation signal.
    ///
    /// See `engine-tui-unified-transcript-plan.md` §7.4.
    pub pending_auto_restore_truncate: Option<String>,
    /// TUI-originated system messages waiting to be dispatched as
    /// `UserCommand::PushSystemMessage` after the current
    /// `handle_core_event` returns. Notification handlers don't have a
    /// `command_tx` (pure-fold signature), so they enqueue here and the
    /// App loop drains the queue in `drain_pending_system_pushes`.
    /// Mirrors the `pending_auto_restore_truncate` pattern.
    pub pending_system_pushes: std::collections::VecDeque<crate::command::SystemPushKind>,
    /// Active model id (e.g. `claude-sonnet-4-6`, `gpt-5`, `gemini-2.5-pro`).
    pub model: String,
    /// Active provider id for [`Self::model`] (e.g. `anthropic`, `openai`,
    /// `google`). Sourced from `RuntimeConfig.model_roles[Main].provider` at
    /// session bootstrap; the picker reads provider metadata from the
    /// session-frozen model catalog rather than inferring it from model ids.
    pub provider: String,
    /// Session-frozen view of every `(provider, model_id)` pair known
    /// to the runtime. Seeded once at startup; consumed by
    /// `update::CycleThinkingLevel` (read `supported_efforts` for the
    /// active Main model) and `update::show::build_model_entries`
    /// (picker rendering, including L1 user-catalog + L2 per-provider
    /// overrides that `builtin_models_partial()` alone wouldn't surface).
    pub model_catalog: Vec<ModelCatalogEntry>,
    /// Session-frozen provider config validation results. The picker
    /// uses this to mark unavailable provider/model rows before the
    /// user hits Enter.
    pub provider_statuses: HashMap<String, ProviderStatus>,
    /// Live per-role bindings. Empty entries inherit
    /// `RuntimeConfig.model_roles[role]`; populated entries reflect
    /// in-memory picker selections. Drives the picker's
    /// `is_current_for_role` flag and (for `Main`) the Ctrl+T cycle.
    pub model_by_role: HashMap<ModelRole, ModelBinding>,
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
    /// Conversation identifier — rotated on rewind so cache breaks
    /// invalidate cleanly on the next request. TS: REPL holds a
    /// `conversationId` minted from `randomUUID()` and re-mints it
    /// inside `rewindConversationTo` (`screens/REPL.tsx:3673`).
    pub conversation_id: Option<String>,
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
    /// Current compaction sub-phase (drives the spinner text). `None`
    /// when no compaction is running. Maps to TS REPL.tsx:2502
    /// `spinnerMessage` switch on `onCompactProgress` events.
    pub compaction_phase: Option<CompactionPhaseLabel>,
    /// Whether the post-compact warning suppressor is active. When
    /// true, the TokenWarning banner is hidden because the displayed
    /// pre-compact token count is stale. TS:
    /// `services/compact/compactWarningHook.ts` subscribes
    /// `compactWarningStore`.
    pub compact_warning_suppressed: bool,
    /// Connected MCP servers.
    pub mcp_servers: Vec<McpServerStatus>,
    /// `true` when at least one LSP server is healthy. Populated from
    /// `ServerNotification::SessionStarted.lsp_active`; drives the
    /// "LSP" badge on the status bar.
    pub lsp_active: bool,
    /// Focused subagent index for teammate/activity views.
    pub focused_subagent_index: Option<i32>,
    /// Current turn number (within multi-turn loop).
    pub current_turn_number: Option<i32>,
    /// Transcript index where the current LLM response started.
    /// Thinking token totals reported at `TurnCompleted` are scoped to
    /// this response, even if its streaming thinking was flushed before
    /// a tool call.
    pub current_turn_message_start: Option<usize>,
    /// Wall-clock start for the current LLM response. Used when a
    /// provider reports reasoning tokens but hides the reasoning text.
    pub current_turn_started_at: Option<Instant>,
    /// Queued commands for mid-turn injection — projection of the
    /// engine's `CommandQueue` populated via `CommandQueued` /
    /// `CommandDequeued` notifications. Each entry pairs the engine
    /// queue item's stable id with a short preview of the prompt so
    /// `CommandDequeued{id}` can remove the matching entry even if
    /// priority reordering caused the item not to be at the front.
    pub queued_commands: VecDeque<QueuedCommandDisplay>,
    /// Available models for model picker.
    pub available_models: Vec<String>,
    /// Whether file checkpointing is enabled for rewind.
    /// Set by the orchestrator (tui_runner) at startup.
    pub file_history_enabled: bool,
    /// Whether the rewind picker should expose `Summarize up to here`.
    /// TS gates this behind `'external' === 'ant'`; we surface it via
    /// `settings.json` (`rewind.allow_summarize_up_to`, default false).
    pub allow_summarize_up_to: bool,
    /// Available slash commands for `/` autocomplete and `/help` palette.
    /// Snapshotted from `CommandRegistry::visible()` at session start
    /// (see `app/cli/src/tui_runner.rs`). Filtered + ranked by
    /// [`crate::autocomplete::slash`] when the user types `/`.
    pub available_commands: Vec<SlashCommandInfo>,
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
    /// Current git branch name shown in the header. Populated at
    /// startup from `coco_git::operations::get_current_branch`; live
    /// updates would flow through `git_index_watcher.rs` once the
    /// watcher gains a branch-refresh emit. `None` when the cwd is
    /// outside a git work tree or HEAD is detached.
    pub git_branch: Option<String>,
    /// Active thinking effort for the current session. Mirrors the
    /// engine's resolved level (set on session start), cycled by
    /// `TuiCommand::CycleThinkingLevel` (Ctrl+T). `Auto` keeps the
    /// model's per-call default — distinct from `Disable` which
    /// explicitly turns thinking off on supported providers.
    pub thinking_effort: coco_types::ReasoningEffort,
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
    /// Durable V2 plan-item snapshot, mirrored from `ToolAppState`
    /// via `ServerNotification::TaskPanelChanged`. Read by
    /// [`crate::widgets::PlanPanel`] when expanded_view == Tasks.
    pub plan_tasks: Vec<coco_types::TaskRecord>,
    /// V1 per-agent TodoWrite snapshots, keyed by agent_id or session_id.
    pub todos_by_agent: std::collections::HashMap<String, Vec<coco_types::TodoRecord>>,
    /// Which task panel is expanded. Defaults to `None`.
    pub expanded_view: coco_types::ExpandedView,
    /// Verification-nudge banner flag.
    pub verification_nudge_pending: bool,
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
    /// Wall-clock at which the most recent turn completed. Drives
    /// idle-prompt notification firing (TS REPL.tsx:3933 —
    /// `lastQueryCompletionTime` + `messageIdleNotifThresholdMs`).
    /// Set by `on_turn_completed`; cleared on new submit / `idle_prompt_fired`.
    pub last_query_completion_at: Option<Instant>,
    /// Wall-clock of the most recent user keystroke or input event.
    /// Used to short-circuit idle firing when the user has interacted
    /// since the turn completed. TS: `getLastInteractionTime()`.
    pub last_user_interaction_at: Instant,
    /// Idle-prompt single-shot. After a turn completes we fire
    /// `idle_prompt` notification at most once per
    /// `last_query_completion_at` epoch. Reset to `false` whenever
    /// `last_query_completion_at` is set or cleared.
    pub idle_prompt_fired: bool,
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
        self.set_tool_use_status(call_id, ToolUseStatus::Running);
    }

    /// Complete a tool execution.
    pub fn complete_tool(&mut self, call_id: &str, is_error: bool) {
        let use_status = if is_error {
            ToolUseStatus::Failed
        } else {
            ToolUseStatus::Completed
        };
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
        self.set_tool_use_status(call_id, use_status);
    }

    fn set_tool_use_status(&mut self, call_id: &str, next_status: ToolUseStatus) {
        for message in &mut self.messages {
            if let MessageContent::ToolUse {
                call_id: message_call_id,
                status,
                ..
            } = &mut message.content
                && message_call_id == call_id
            {
                *status = next_status;
            }
        }
    }

    /// Merged transcript view: legacy `session.messages` overlaid with
    /// engine-derived cells from `session.transcript`. Engine-authoritative
    /// entries supersede TUI optimistic ones on matching `id`; cells with
    /// no `session.messages` counterpart append at the end. This is the
    /// single source of truth for everything that wants to *render* the
    /// chat (transcript modal, viewport, history_lines, …) — both
    /// `session.messages` (which now receives almost no writes post
    /// Commit 2) and `session.transcript` (engine-driven) on their own
    /// are partial. Commit 4/5 will retire the legacy field and this
    /// helper folds away.
    pub fn transcript_messages(&self) -> Vec<ChatMessage> {
        crate::state::derive::merged_chat_messages(&self.messages, self.transcript.cells())
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
            transcript: super::transcript_view::TranscriptView::new(),
            pending_auto_restore_truncate: None,
            pending_system_pushes: std::collections::VecDeque::new(),
            model: String::new(),
            provider: String::new(),
            model_catalog: Vec::new(),
            provider_statuses: HashMap::new(),
            model_by_role: HashMap::new(),
            permission_mode: PermissionMode::Default,
            bypass_permissions_available: false,
            auto_mode_available: false,
            tool_executions: Vec::new(),
            subagents: Vec::new(),
            token_usage: TokenUsage::default(),
            session_id: None,
            conversation_id: None,
            working_dir: None,
            turn_count: 0,
            context_window_used: 0,
            context_window_total: 0,
            estimated_cost_cents: 0,
            fast_mode: false,
            busy: false,
            fallback_model: None,
            is_compacting: false,
            compaction_phase: None,
            compact_warning_suppressed: false,
            mcp_servers: Vec::new(),
            lsp_active: false,
            focused_subagent_index: None,
            current_turn_number: None,
            current_turn_message_start: None,
            current_turn_started_at: None,
            queued_commands: VecDeque::new(),
            available_models: Vec::new(),
            file_history_enabled: false,
            allow_summarize_up_to: false,
            available_commands: Vec::new(),
            available_agents: Vec::new(),
            saved_sessions: Vec::new(),
            session_state: coco_types::SessionState::Idle,
            worktree_path: None,
            git_branch: None,
            thinking_effort: coco_types::ReasoningEffort::Auto,
            model_fallback_banner: None,
            rate_limit_info: None,
            context_usage_percent: None,
            sandbox_active: false,
            stream_stall: false,
            active_tasks: Vec::new(),
            plan_tasks: Vec::new(),
            todos_by_agent: std::collections::HashMap::new(),
            expanded_view: coco_types::ExpandedView::None,
            verification_nudge_pending: false,
            active_hooks: Vec::new(),
            prompt_suggestions: Vec::new(),
            local_command_output: VecDeque::new(),
            available_output_styles: Vec::new(),
            available_plugins: Vec::new(),
            last_agent_markdown: None,
            ide_selection: None,
            ide_diagnostics: None,
            last_query_completion_at: None,
            last_user_interaction_at: Instant::now(),
            idle_prompt_fired: false,
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
    /// Creation timestamp (Unix epoch ms). Used by the rewind picker
    /// to render `formatRelativeTimeAgo(ts)`. Defaults to "now" at
    /// construction time.
    pub created_at_ms: i64,
    /// Compact-summary marker. The compact pipeline emits a synthetic
    /// "summary" user message to seed the rolled-up history; rewind
    /// must skip these because rewinding to a summary would lose the
    /// archived turns. TS: `UserMessage.isCompactSummary` in messages.ts.
    pub is_compact_summary: bool,
    /// Transcript-only visibility — message is present in the JSONL
    /// log for replay but not selectable in the rewind picker. TS:
    /// `UserMessage.isVisibleInTranscriptOnly` in messages.ts.
    pub is_visible_in_transcript_only: bool,
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
        reasoning_tokens: Option<i64>,
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
    /// Compaction summary message — the LLM-or-SM-generated summary
    /// that replaces the archived turns. TS: `CompactSummary.tsx`.
    CompactSummary {
        /// Summary text.
        summary: String,
        /// Number of messages summarized (None when unknown).
        messages_summarized: Option<i32>,
        /// User-supplied focus directive (from `/compact <text>` or
        /// PreCompact hook). None means no metadata banner.
        user_context: Option<String>,
        /// How compaction was triggered. Drives the heading text:
        /// "Summarized via session memory" vs "Conversation summary".
        trigger: coco_types::CompactTrigger,
    },
    /// Advisor message from coordinator agent.
    Advisor { advisor_id: String, content: String },
    /// Task assignment notification.
    TaskAssignment {
        task_id: String,
        assignee: String,
        description: String,
    },
    /// Synthetic marker rendered after a Ctrl+C cancellation.
    ///
    /// TS parity: `[Request interrupted by user]` user message rendered
    /// as `<InterruptedByUser />` (see `InterruptedByUser.tsx`). The
    /// engine pushes the literal text to `MessageHistory` for next-turn
    /// model context; the TUI's `on_turn_interrupted` handler appends
    /// this variant for the visible chat row. `for_tool_use=true` mirrors
    /// `createUserInterruptionMessage({toolUse: true})` and is set when
    /// in-flight tool calls were interrupted.
    InterruptionMarker { for_tool_use: bool },
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
            created_at_ms: now_ms(),
            is_compact_summary: false,
            is_visible_in_transcript_only: false,
            permission_mode: None,
        }
    }

    /// Create a bash-input user message (rendered as `> $ <command>`).
    /// TS parity: `UserBashInputMessage`.
    pub fn user_bash_input(id: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: ChatRole::User,
            content: MessageContent::BashInput {
                command: command.into(),
            },
            is_meta: false,
            created_at_ms: now_ms(),
            is_compact_summary: false,
            is_visible_in_transcript_only: false,
            permission_mode: None,
        }
    }

    /// Create a bash-output user message (rendered as indented body).
    /// Same id as the matching `BashInput` so rewind groups them.
    /// TS parity: `UserBashOutputMessage`.
    pub fn user_bash_output(
        id: impl Into<String>,
        output: impl Into<String>,
        exit_code: i32,
    ) -> Self {
        Self {
            id: id.into(),
            role: ChatRole::User,
            content: MessageContent::BashOutput {
                output: output.into(),
                exit_code,
            },
            is_meta: false,
            created_at_ms: now_ms(),
            is_compact_summary: false,
            is_visible_in_transcript_only: false,
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
            created_at_ms: now_ms(),
            is_compact_summary: false,
            is_visible_in_transcript_only: false,
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
            created_at_ms: now_ms(),
            is_compact_summary: false,
            is_visible_in_transcript_only: false,
            permission_mode: None,
        }
    }

    /// Create the synthetic interrupt marker chat row. Mirrors TS
    /// `<InterruptedByUser />`. `for_tool_use=true` when the cancel
    /// happened while tools were running.
    pub fn interruption_marker(id: impl Into<String>, for_tool_use: bool) -> Self {
        Self {
            id: id.into(),
            role: ChatRole::User,
            content: MessageContent::InterruptionMarker { for_tool_use },
            is_meta: false,
            created_at_ms: now_ms(),
            is_compact_summary: false,
            is_visible_in_transcript_only: false,
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
            created_at_ms: now_ms(),
            is_compact_summary: false,
            is_visible_in_transcript_only: false,
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
            created_at_ms: now_ms(),
            is_compact_summary: false,
            is_visible_in_transcript_only: false,
            permission_mode: None,
        }
    }

    /// Create a teammate-attributed message for the leader's view.
    ///
    /// Tagged `is_meta=true` so the chat widget hides it from the
    /// regular scroll (the filter at `widgets/chat/mod.rs` skips
    /// `is_meta && !show_system_reminders`); the transcript reader
    /// renders with system reminders enabled so these surface there.
    /// `is_visible_in_transcript_only=true` also marks the
    /// message as a non-rewindable anchor (`update_rewind` skips it).
    /// ID is `teammate:{agent}:{uuid}` so concurrent teammates
    /// can't collide.
    pub fn teammate_message(teammate: impl Into<String>, content: impl Into<String>) -> Self {
        let teammate = teammate.into();
        let uuid = uuid::Uuid::new_v4();
        Self {
            id: format!("teammate:{teammate}:{uuid}"),
            role: ChatRole::User,
            content: MessageContent::TeammateMessage {
                teammate,
                content: content.into(),
            },
            is_meta: true,
            created_at_ms: now_ms(),
            is_compact_summary: false,
            is_visible_in_transcript_only: true,
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
            MessageContent::CompactSummary { summary, .. } => summary,
            MessageContent::Advisor { content, .. } => content,
            MessageContent::TaskAssignment { description, .. } => description,
            MessageContent::ChannelMessage { content, .. } => content,
            MessageContent::ResourceUpdate { target, .. } => target,
            MessageContent::InterruptionMarker { .. } => "",
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
///
/// `started_at_ms` and `token_usage` are optional so the TUI can render
/// elapsed-time / token-cost telemetry alongside the spinner line —
/// TS parity with `CoordinatorAgentStatus.tsx`. The protocol handler
/// populates them on `SubagentStarted` / `SubagentTokens` notifications
/// when available, and the renderer hides each field when unset rather
/// than synthesising fake zeros.
#[derive(Debug, Clone)]
pub struct SubagentInstance {
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    pub status: SubagentStatus,
    pub color: Option<String>,
    /// Unix-epoch ms when the subagent started. `None` while the
    /// protocol handler hasn't populated it yet. The renderer shows
    /// `elapsed = now - started_at` only when this is set.
    pub started_at_ms: Option<i64>,
    /// Cumulative token usage for this teammate. The renderer shows
    /// `↑input ↓output` arrows so the user sees direction at a glance.
    pub token_usage: Option<TokenUsage>,
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
    pub reasoning_tokens: i64,
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
