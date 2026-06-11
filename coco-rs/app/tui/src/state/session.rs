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
    /// An OAuth-subscription provider with no logged-in credential. Distinct
    /// from `MissingApiKey` (whose hint names an env var) — the fix is
    /// `coco login <provider>`, not setting a key.
    NotLoggedIn { provider: String },
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
    pub context_window: Option<i64>,
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
    /// Whether Esc/Up may pull this queued item back into the composer.
    pub editable: bool,
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

impl CompactionPhaseLabel {
    pub fn status_label(self) -> &'static str {
        match self {
            Self::PreCompactHooks => "Running PreCompact hooks",
            Self::PostCompactHooks => "Running PostCompact hooks",
            Self::SessionStartHooks => "Running SessionStart hooks",
            Self::Summarizing => "Compacting conversation",
        }
    }
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
    /// Engine-authoritative view of `MessageHistory`, populated by the
    /// `MessageAppended` / `MessageTruncated` / `SessionResetForResume`
    /// protocol handlers. Engines push every message through
    /// `history_push_and_emit` so cells stay coherent with the
    /// JSONL transcript on disk — this is the source of truth for
    /// "what is in the conversation".
    pub transcript: super::transcript_view::TranscriptView,
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
    /// True when all currently running tools are cancel-interruptible.
    pub has_submit_interruptible_tool_in_progress: bool,
    /// Side-cache for `ServerNotification::ToolUseSummary` payloads.
    ///
    /// Tool-use summaries are UI-only polish (mobile-row label
    /// generated by the Fast model post-turn) and intentionally
    /// **not** part of `MessageHistory` — keeping them out of the
    /// authoritative transcript upholds I-3 from
    /// `engine-tui-unified-transcript-plan.md`.
    ///
    /// Keyed by the first `preceding_tool_use_id` of the summarized
    /// tool batch so renderers can attach the label to the assistant
    /// turn whose first tool_use produced that id. Cleared on session
    /// reset; never persisted.
    pub tool_group_summaries: HashMap<String, String>,
    /// Side-cache for per-assistant-message reasoning metadata
    /// (`duration_ms`, `reasoning_tokens`). The engine emits aggregate
    /// reasoning usage on `TurnCompleted` after the assistant message
    /// has already streamed and committed; this side-cache lets the
    /// renderer surface `Thinking · 1.3s · 15 reasoning tokens`
    /// without mutating the derived `RenderedCell` (preserves I-2).
    ///
    /// Keyed by the assistant message UUID. Cleared on session reset
    /// and pruned on `MessageTruncated`.
    pub reasoning_metadata: HashMap<uuid::Uuid, ReasoningMetadata>,
    /// Subagent instances.
    pub subagents: Vec<SubagentInstance>,
    /// Token usage.
    pub token_usage: TokenUsage,
    /// Cumulative session usage and cost snapshot.
    pub session_usage: Option<coco_types::SessionUsageSnapshot>,
    /// Session identifier.
    pub session_id: Option<String>,
    /// OS process id, surfaced in the header so concurrent coco sessions
    /// can be told apart and matched to their per-PID log file
    /// (`<config_home>/logs/coco.<pid>.log.<date>`). `0` is the unset
    /// sentinel (never a real user process) used by tests and pre-bootstrap
    /// state; the header hides the pid badge while it is `0`. Set once in
    /// `App::new` from `std::process::id()`.
    pub pid: u32,
    /// Conversation identifier — rotated on rewind so cache breaks
    /// invalidate cleanly on the next request. TS: REPL holds a
    /// `conversationId` minted from `randomUUID()` and re-mints it
    /// inside `rewindConversationTo` (`screens/REPL.tsx:3673`).
    pub conversation_id: Option<String>,
    /// Working directory.
    pub working_dir: Option<String>,
    /// Turn counter.
    pub turn_count: i32,
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
    /// Start time for the current compaction operation, if visible in the UI.
    pub compaction_started_at: Option<Instant>,
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
    /// Queued commands for mid-turn injection — projection of the
    /// engine's `CommandQueue` populated via `CommandQueued` /
    /// `CommandDequeued` notifications. Each entry pairs the engine
    /// queue item's stable id with a short preview of the prompt so
    /// `CommandDequeued{id}` can remove the matching entry even if
    /// priority reordering caused the item not to be at the front.
    pub queued_commands: VecDeque<QueuedCommandDisplay>,
    /// Available models for model picker. `None` means unrestricted;
    /// `Some([])` means the allowlist is explicitly empty.
    pub available_models: Option<Vec<String>>,
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
    /// Available agents for the unified `@` autocomplete popup. Populated
    /// by the session handler when the agent registry is loaded; used
    /// synchronously by `autocomplete::unified::seed_agent_items` whenever
    /// the user types `@<query>` so agents appear inline before async
    /// file-search results arrive.
    pub available_agents: Vec<crate::autocomplete::AgentInfo>,
    /// Saved sessions for session browser.
    pub saved_sessions: Vec<SavedSession>,
    /// MCP resources available to unified `@` completion. Empty until a
    /// typed source is wired by the TUI bootstrap/runtime.
    pub available_mcp_resources: Vec<crate::completion::McpResourceCompletion>,
    /// Slack channels available to channel completion. Empty by default; the
    /// completion layer must not guess rows without a typed source.
    pub available_slack_channels: Vec<crate::completion::SlackChannelCompletion>,

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
    /// Sandbox active state (set by SandboxStateChanged).
    pub sandbox_active: bool,
    /// Stream health: stall detected (set by StreamStallDetected, cleared on next turn).
    pub stream_stall: bool,
    /// Active background tasks (set by TaskStarted, updated by TaskProgress/Completed).
    pub active_tasks: Vec<TaskEntry>,
    /// Durable V2 plan-item snapshot, mirrored from `ToolAppState`
    /// via `ServerNotification::TaskPanelChanged`. Read by
    /// `presentation::activity::plan_surface` when expanded_view == Tasks.
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
    /// Active output style name from session bootstrap.
    pub output_style: Option<String>,
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
    /// Whether the agent is busy.
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Update the pause/resume accumulators from the current prompt
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
            // Set later by `on_message_appended` when the assistant
            // turn that owns this tool_use commits. Mid-stream, the
            // engine assistant message UUID isn't known yet.
            message_uuid: None,
        });
    }

    /// Stamp the parent assistant message UUID onto every ToolExecution
    /// whose `call_id` matches a `ToolCall` content block in `msg`.
    /// Called from the `MessageAppended` handler when an Assistant
    /// message lands. After this stamp, [`Self::retain_tool_executions_for_messages`]
    /// can decide which overlays survive a truncate.
    pub fn stamp_tool_executions_with_assistant_uuid(&mut self, msg: &coco_messages::Message) {
        let coco_messages::Message::Assistant(a) = msg else {
            return;
        };
        let coco_messages::LlmMessage::Assistant { content, .. } = &a.message else {
            return;
        };
        for part in content {
            if let coco_messages::AssistantContent::ToolCall(tc) = part
                && let Some(exec) = self
                    .tool_executions
                    .iter_mut()
                    .find(|t| t.call_id == tc.tool_call_id)
            {
                exec.message_uuid = Some(a.uuid);
            }
        }
    }

    /// Drop tool executions whose anchor assistant-message UUID is no
    /// longer in `surviving_uuids`. Executions that were never stamped
    /// (`message_uuid = None`) are kept — they belong to an in-flight
    /// stream that survives any user-initiated truncate, since the
    /// stream itself was already cancelled by the same UI flow.
    pub fn retain_tool_executions_for_messages(
        &mut self,
        surviving_uuids: &std::collections::HashSet<uuid::Uuid>,
    ) {
        self.tool_executions.retain(|t| match t.message_uuid {
            Some(uuid) => surviving_uuids.contains(&uuid),
            None => true,
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

    pub fn insert_reasoning_metadata(&mut self, uuid: uuid::Uuid, metadata: ReasoningMetadata) {
        // The renderer reads this side-cache at cell-build time
        // (`history_options`), and the finalize draw emits the assistant cell
        // append-only with the duration/tokens already baked in. So attaching
        // metadata does NOT change `HistoryDisplayState` and does NOT force a
        // full `replay_all_capped` — that per-turn rewrite was the cost we drop.
        self.reasoning_metadata.insert(uuid, metadata);
    }

    pub fn retain_reasoning_metadata_for_messages(
        &mut self,
        surviving_uuids: &std::collections::HashSet<uuid::Uuid>,
    ) {
        // Prune anchors to surviving messages. The triggering `MessageTruncated`
        // already replays history, so no extra invalidation is needed here.
        self.reasoning_metadata
            .retain(|uuid, _| surviving_uuids.contains(uuid));
    }

    pub fn clear_reasoning_metadata(&mut self) {
        self.reasoning_metadata.clear();
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            transcript: super::transcript_view::TranscriptView::new(),
            model: String::new(),
            provider: String::new(),
            model_catalog: Vec::new(),
            provider_statuses: HashMap::new(),
            model_by_role: HashMap::new(),
            permission_mode: PermissionMode::Default,
            bypass_permissions_available: false,
            auto_mode_available: false,
            tool_executions: Vec::new(),
            has_submit_interruptible_tool_in_progress: false,
            tool_group_summaries: HashMap::new(),
            reasoning_metadata: HashMap::new(),
            subagents: Vec::new(),
            token_usage: TokenUsage::default(),
            session_usage: None,
            session_id: None,
            pid: 0,
            conversation_id: None,
            working_dir: None,
            turn_count: 0,
            estimated_cost_cents: 0,
            fast_mode: false,
            busy: false,
            fallback_model: None,
            is_compacting: false,
            compaction_started_at: None,
            compaction_phase: None,
            compact_warning_suppressed: false,
            mcp_servers: Vec::new(),
            lsp_active: false,
            focused_subagent_index: None,
            current_turn_number: None,
            queued_commands: VecDeque::new(),
            available_models: None,
            file_history_enabled: false,
            allow_summarize_up_to: false,
            available_commands: Vec::new(),
            available_agents: Vec::new(),
            saved_sessions: Vec::new(),
            available_mcp_resources: Vec::new(),
            available_slack_channels: Vec::new(),
            session_state: coco_types::SessionState::Idle,
            worktree_path: None,
            git_branch: None,
            thinking_effort: coco_types::ReasoningEffort::Auto,
            model_fallback_banner: None,
            rate_limit_info: None,
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
            output_style: None,
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
    /// UUID of the engine `Message::Assistant` that emitted this
    /// tool_use content block. Populated when `MessageAppended` for the
    /// owning assistant turn arrives and walks `ToolCall` blocks to
    /// pair `call_id` with the parent message UUID. `None` until then
    /// (mid-stream window — the engine assistant message hasn't been
    /// committed yet).
    ///
    /// Used by the `MessageTruncated` handler to drop only executions
    /// anchored to messages that no longer survive the truncation,
    /// rather than clearing every in-flight tool overlay.
    pub message_uuid: Option<uuid::Uuid>,
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
/// TS parity with `CoordinatorAgentStatus.tsx` / `AgentProgressLine` /
/// `TeammateSpinnerLine`. Lifecycle is split into two orthogonal axes:
/// terminal lifecycle in [`SubagentStatus`], and the UI-only
/// foreground-vs-background flag in [`Self::is_backgrounded`]. A task
/// can be backgrounded while still `Running` (TS: `task.status ===
/// 'running' && task.extras.isBackgrounded`).
#[derive(Debug, Clone)]
pub struct SubagentInstance {
    /// Which TS concept this row tracks. See [`SubagentKind`].
    pub kind: SubagentKind,
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    pub status: SubagentStatus,
    pub color: Option<String>,
    /// Team name for `kind == Teammate`. `None` for subagents (they
    /// have no team affiliation) and for legacy / dormant entries.
    pub team_name: Option<String>,
    /// `tool_use_id` of the parent `Agent` tool invocation that
    /// spawned this row (when the entry came from a TaskStarted bridge
    /// for a `BgAgent` task). Used by the inline `AgentProgressLine`
    /// renderer to attach this row visually beneath the matching
    /// `ToolExecution` in the transcript. `None` for teammates (they
    /// have no originating tool call).
    pub tool_use_id: Option<String>,
    /// Unix-epoch ms when the subagent started. `None` while the
    /// protocol handler hasn't populated it yet. The renderer shows
    /// `elapsed = now - started_at` only when this is set.
    pub started_at_ms: Option<i64>,
    /// Most recently dispatched tool. Mirror of `TaskProgress.last_tool_name`
    /// for BgAgent rows so the AgentProgressLine subline can render
    /// `<tool> · N tools · M tok` like the TS reference. `None` before
    /// the first tool call.
    pub last_tool_name: Option<String>,
    /// Cumulative tool invocation count. Lifted from `TaskProgress.usage.tool_uses`
    /// for parity with TS `AgentProgressLine`'s "8 tool uses" metric.
    /// Monotonically maxed at the bridge so out-of-order progress
    /// snapshots don't roll the counter backwards.
    pub tool_count: i32,
    /// Cumulative total token count. Mirror of
    /// `TaskProgress.usage.total_tokens` — monotonically maxed for the
    /// same reason as [`Self::tool_count`]. Zero means "not yet reported".
    pub total_tokens: i64,
    /// UI-only flag for the foreground→background transition (Ctrl+B).
    /// Not produced by any wire event — the optimistic flip lives in
    /// `update::handle_command(TuiCommand::BackgroundAllTasks)`. TS:
    /// `task.extras.isBackgrounded`, orthogonal to status.
    pub is_backgrounded: bool,
    /// Cap-5 ring buffer of recent tool activities invoked by this
    /// subagent. Mirrors TS `MAX_RECENT_ACTIVITIES = 5`
    /// (`tasks/LocalAgentTask/LocalAgentTask.tsx:40`). Populated by
    /// copying [`coco_types::TaskProgress::recent_activities`] verbatim
    /// — the coordinator-side rings (`runner_loop.rs`,
    /// `agent_handle/spawn.rs`) own the push policy. Renderers display
    /// in insertion order (oldest first).
    pub recent_activities: Vec<coco_types::TaskActivity>,
    /// Final assistant message after the subagent completes — first
    /// 80 chars rendered inline so the user sees the closing statement
    /// without expanding the transcript.
    pub final_message: Option<String>,
}

/// Subagent lifecycle status. Mirrors the terminal axis of TS
/// `TaskStatus` filtered down to what the TUI displays. The orthogonal
/// foreground/background axis lives on [`SubagentInstance::is_backgrounded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatus {
    Running,
    Completed,
    Failed,
}

/// What TS concept a [`SubagentInstance`] row represents.
///
/// TS keeps these strictly separate (`InProcessTeammateTask` vs
/// `LocalAgentTask`); coco-rs collapses them into one TUI struct
/// (`SubagentInstance`) but tags the kind so renderers can show the
/// right badge / placement and lifecycle handlers can apply the right
/// semantics (teammate lives across `/clear`; subagent evicts).
///
/// The unification at the TUI struct level is a coco-rs choice — both
/// surfaces share the same status / tool-count / token / final-message
/// vocabulary, so duplicating types yielded no benefit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentKind {
    /// `Agent`-tool spawned worker. TS `LocalAgentTask` (type
    /// `'local_agent'`). Transient — evicts on completion. Has
    /// `tool_use_id` pointing to the parent assistant's `ToolUse`.
    Subagent,
    /// Coordinator-spawned persistent team member. TS
    /// `InProcessTeammateTask` (type `'in_process_teammate'`).
    /// Lives across `/clear`. Identity is `agent_name@team_name`.
    Teammate,
}

/// Per-message reasoning metadata stamped on `TurnCompleted`.
///
/// Kept in `SessionState.reasoning_metadata` keyed by assistant
/// message UUID so the renderer can surface
/// `Thinking · <duration> · <reasoning_tokens>` without mutating
/// the derived `RenderedCell` — preserves I-2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReasoningMetadata {
    pub duration_ms: Option<i64>,
    pub reasoning_tokens: i64,
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
