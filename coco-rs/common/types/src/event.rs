use serde::Deserialize;
use serde::Serialize;

use crate::TokenUsage;
use crate::wire_tagged::wire_tagged_enum;

/// Three-layer event envelope.
///
/// All consumers (TUI, SDK, CLI, App-Server) receive `CoreEvent` via
/// `mpsc::channel`. Each consumer matches on the layer it cares about:
///
/// - **TUI**: all 3 layers (exhaustive match, no intermediate bridge type)
/// - **SDK/CLI**: Protocol + Stream (via `StreamAccumulator`; TuiEvent dropped)
/// - **App-Server**: Protocol + Stream (TuiEvent dropped)
///
/// # Ordering contract
///
/// `mpsc` provides FIFO ordering **per sender**. When multiple tasks clone
/// the same `Sender<CoreEvent>` and emit concurrently, cross-sender
/// ordering is **not guaranteed**.
///
/// Where ordering matters, all related events must be emitted from a
/// single task. Current ownership (one sequence = one task):
///
/// - **Turn lifecycle** (`TurnStarted → Stream* → TurnCompleted|Failed|Interrupted`):
///   emitted by `run_session_loop` in `coco-query::engine`.
/// - **Session lifecycle** (`SessionStarted → (Running ↔ Idle ↔ RequiresAction)*
///   → SessionResult → SessionEnded`): emitted by `run_internal_with_messages`
///   in `coco-query::engine`; `SessionStateChanged` transitions are deduped
///   via `SessionStateTracker` (see `coco-query::session_state`).
/// - **Hook lifecycle** (`HookStarted → HookProgress* → HookResponse`):
///   emitted by the `forward_hook_events` child task in `coco-query::engine`.
///   Cancellation + 5s drain-on-shutdown protect trailing events.
/// - **Task lifecycle** (`TaskStarted → TaskProgress* → TaskCompleted`):
///   emitted by `TaskManager` when built with `with_event_sink(tx)`.
///   One task manager serializes emissions for all managed tasks.
/// - **Item lifecycle** (`ItemStarted → ItemUpdated → ItemCompleted`) and
///   content deltas (`AgentMessageDelta`, `ReasoningDelta`):
///   **SDK path only**. Produced by `StreamAccumulator` inside the SDK
///   dispatcher's writer task (single task, per-turn accumulator). The
///   TUI consumes `AgentStreamEvent` directly and never sees these.
/// - **Wire serialization**: the SDK dispatcher's writer task is the single
///   serializer — all events pass through one `tokio::select!` loop with
///   `biased;` preferring notifications over replies, so wire order matches
///   channel-receive order.
///
/// ## Known cross-sender emission sites (tolerated)
///
/// - `ContextCompacted` is emitted from two sites inside `run_session_loop`
///   (reactive compaction and auto-compaction). Semantics are idempotent;
///   consumers may see two notifications carrying the same summary.
/// - `Error` may be emitted from budget-exhaustion and query-execution
///   paths. Consumers MUST treat Errors as independent signals; they are
///   not sequenced relative to other events.
///
/// See `event-system-design.md` §12 and plan WS-8.
#[derive(Debug, Clone)]
pub enum CoreEvent {
    /// Protocol-level notifications visible to ALL consumers.
    Protocol(ServerNotification),

    /// Agent-loop stream events requiring accumulation before SDK consumption.
    /// TUI consumes directly for real-time display; SDK passes through
    /// `StreamAccumulator` which converts to `Protocol(ItemStarted/Updated/Completed)`.
    Stream(AgentStreamEvent),

    /// TUI-exclusive events (overlays, toasts, streaming deltas for display).
    /// SDK and App-Server consumers DROP these.
    Tui(TuiOnlyEvent),
}

// ---------------------------------------------------------------------------
// AgentStreamEvent — accumulation-layer stream events
// ---------------------------------------------------------------------------

/// Agent-loop stream events. Higher-level than `coco_types::StreamEvent`
/// (which represents raw LLM inference deltas). Adds:
/// - Tool lifecycle states (Queued → Started → Completed)
/// - MCP tool call tracking
/// - Turn-scoped item IDs
///
/// Input to `StreamAccumulator`.
/// See `event-system-design.md` Section 1.5.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentStreamEvent {
    /// Text content delta from assistant response.
    TextDelta { turn_id: String, delta: String },
    /// Thinking/reasoning delta from extended thinking.
    ThinkingDelta { turn_id: String, delta: String },
    /// Tool use block received from API (input complete). Creates a ThreadItem.
    ToolUseQueued {
        call_id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool execution has begun (after permission check).
    ToolUseStarted {
        call_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        batch_id: Option<String>,
    },
    /// Tool execution completed with result.
    ///
    /// `name` is carried here so StreamAccumulator and TUI consumers can
    /// reconstruct display state without maintaining their own call_id → name map.
    ToolUseCompleted {
        call_id: String,
        name: String,
        output: String,
        is_error: bool,
    },
    /// MCP tool call initiated (separate from builtin tools).
    McpToolCallBegin {
        server: String,
        tool: String,
        call_id: String,
    },
    /// MCP tool call completed.
    McpToolCallEnd {
        server: String,
        tool: String,
        call_id: String,
        is_error: bool,
    },
}

// ---------------------------------------------------------------------------
// ThreadItem — semantic conversation thread items
// ---------------------------------------------------------------------------

/// Semantic representation of a conversation thread item.
/// Produced by `StreamAccumulator` from `AgentStreamEvent` sequences.
/// Used in `ServerNotification::ItemStarted / ItemUpdated / ItemCompleted`.
///
/// See `event-system-design.md` Section 1.6.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadItem {
    pub item_id: String,
    pub turn_id: String,
    pub details: ThreadItemDetails,
}

/// Tool-specific semantic mapping.
///
/// Mapping rules (from `event-system-design.md` Section 6.2):
/// - Bash → `CommandExecution`
/// - Edit/Write → `FileChange`
/// - WebSearch → `WebSearch`
/// - mcp__* → `McpToolCall`
/// - Agent/Task → `Subagent`
/// - all others → `ToolCall`
/// - text content → `AgentMessage`
/// - thinking → `Reasoning`
/// - errors → `Error`
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadItemDetails {
    /// Bash tool → command execution with output.
    CommandExecution {
        command: String,
        output: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        status: ItemStatus,
    },
    /// Edit/Write tools → file change with diff info.
    FileChange {
        changes: Vec<FileChangeInfo>,
        status: ItemStatus,
    },
    /// WebSearch tool.
    WebSearch { query: String, status: ItemStatus },
    /// MCP server tool call.
    McpToolCall {
        server: String,
        tool: String,
        arguments: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        status: ItemStatus,
    },
    /// Agent/Task tool → subagent lifecycle.
    Subagent {
        agent_id: String,
        agent_type: String,
        description: String,
        #[serde(default)]
        is_background: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        status: ItemStatus,
    },
    /// All other tools (Read, Glob, Grep, etc.).
    ToolCall {
        tool: String,
        input: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(default)]
        is_error: bool,
        status: ItemStatus,
    },
    /// Assistant text content.
    AgentMessage { text: String },
    /// Reasoning/thinking content.
    Reasoning { text: String },
    /// Error during processing.
    Error { message: String },
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangeInfo {
    pub path: String,
    pub kind: FileChangeKind,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Create,
    Modify,
    Delete,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    InProgress,
    Completed,
    Failed,
    Declined,
}

// ---------------------------------------------------------------------------
// NotificationMethod + ServerNotification — protocol-layer notifications (66 variants)
// ---------------------------------------------------------------------------

wire_tagged_enum! {
    method_enum = NotificationMethod,
    tagged_enum = ServerNotification,
    method_doc = "\
Wire-method identifier for every `ServerNotification` variant.\n\n\
Cross-language protocol constant exported to the JSON schema bundle so \
Python / other SDK codegens obtain the same vocabulary. Consumers should \
reference `NotificationMethod::SessionStarted` rather than compare against \
raw wire strings.",
    tagged_doc = "\
Protocol-level notifications visible to all consumers.\n\n\
66 variants across 20 categories. See `event-system-design.md` Section 2. \
Each variant's wire method is generated together with the matching \
`NotificationMethod` discriminant.",
    variants = {
    // === Session lifecycle (3) ===

    /// New session started.
    "session/started" => SessionStarted(SessionStartedParams),
    /// Session result (final usage, cost, stop reason).
    "session/result" => SessionResult(Box<SessionResultParams>),
    /// Session ended.
    "session/ended" => SessionEnded(SessionEndedParams),

    // === Turn lifecycle (4) ===

    /// Agent turn started.
    "turn/started" => TurnStarted(TurnStartedParams),
    /// Agent turn completed successfully.
    "turn/completed" => TurnCompleted(TurnCompletedParams),
    /// Agent turn failed with error.
    "turn/failed" => TurnFailed(TurnFailedParams),
    /// Turn interrupted by user.
    "turn/interrupted" => TurnInterrupted(TurnInterruptedParams),

    // === Item lifecycle (3) ===

    /// Thread item started (from StreamAccumulator).
    "item/started" => ItemStarted { item: ThreadItem },
    /// Thread item updated (e.g. tool execution began).
    "item/updated" => ItemUpdated { item: ThreadItem },
    /// Thread item completed.
    "item/completed" => ItemCompleted { item: ThreadItem },

    // === Content deltas (2) ===

    /// Text content delta from assistant.
    "agentMessage/delta" => AgentMessageDelta(ContentDeltaParams),
    /// Reasoning/thinking delta.
    "reasoning/delta" => ReasoningDelta(ContentDeltaParams),

    // === Subagent (4) ===

    /// Subagent spawned.
    "subagent/spawned" => SubagentSpawned(SubagentSpawnedParams),
    /// Subagent completed.
    "subagent/completed" => SubagentCompleted(SubagentCompletedParams),
    /// Subagent moved to background.
    "subagent/backgrounded" => SubagentBackgrounded(SubagentBackgroundedParams),
    /// Subagent progress update.
    "subagent/progress" => SubagentProgress(SubagentProgressParams),

    // === MCP (2) ===

    /// MCP server startup status.
    "mcp/startupStatus" => McpStartupStatus(McpStartupStatusParams),
    /// All MCP servers finished startup.
    "mcp/startupComplete" => McpStartupComplete(McpStartupCompleteParams),

    // === Context (5) ===

    /// Context compacted.
    "context/compacted" => ContextCompacted(ContextCompactedParams),
    /// Context usage warning.
    "context/usageWarning" => ContextUsageWarning(ContextUsageWarningParams),
    /// Compaction started.
    "context/compactionStarted" => CompactionStarted,
    /// Compaction failed.
    "context/compactionFailed" => CompactionFailed(CompactionFailedParams),
    /// Context cleared (e.g. new mode).
    "context/cleared" => ContextCleared(ContextClearedParams),

    // === Task (6) ===

    /// Background task started.
    "task/started" => TaskStarted(TaskStartedParams),
    /// Background task completed.
    "task/completed" => TaskCompleted(TaskCompletedParams),
    /// Background task progress.
    "task/progress" => TaskProgress(TaskProgressParams),
    /// Durable plan-item / V1 todo snapshot — emitted after
    /// `TaskCreate`/`TaskUpdate`/`TodoWrite` tools mutate state so
    /// the TUI can refresh its panel without pulling the store
    /// directly. TS parity: `notifyTasksUpdated` subscriber callback
    /// in `utils/tasks.ts`.
    "task_panel/changed" => TaskPanelChanged(TaskPanelChangedParams),
    /// Team lead received a plan-approval request from a teammate
    /// (via mailbox). The TUI surfaces this as a modal overlay.
    /// TS parity: `ExitPlanModeV2Tool.ts:137-141` teammate request flow.
    "plan_approval/requested" => PlanApprovalRequested(PlanApprovalRequestedParams),
    /// Agents killed.
    "agents/killed" => AgentsKilled(AgentsKilledParams),

    // === Model (3) ===

    /// Model fallback started.
    "model/fallbackStarted" => ModelFallbackStarted(ModelFallbackParams),
    /// Model fallback completed.
    "model/fallbackCompleted" => ModelFallbackCompleted,
    /// Fast mode state changed.
    "model/fastModeChanged" => FastModeChanged { active: bool },

    // === Permission (1) ===

    /// Permission mode changed.
    "permission/modeChanged" => PermissionModeChanged(PermissionModeChangedParams),

    // === Prompt (1) ===

    /// Prompt suggestions.
    "prompt/suggestion" => PromptSuggestion { suggestions: Vec<String> },

    // === System (3) ===

    /// Error notification.
    "error" => Error(ErrorParams),
    /// Rate limit notification.
    "rateLimit" => RateLimit(RateLimitParams),
    /// Keep-alive heartbeat.
    "keepAlive" => KeepAlive { timestamp: i64 },

    // === IDE (2) ===

    /// IDE selection changed.
    "ide/selectionChanged" => IdeSelectionChanged(IdeSelectionChangedParams),
    /// IDE diagnostics updated.
    "ide/diagnosticsUpdated" => IdeDiagnosticsUpdated(IdeDiagnosticsUpdatedParams),

    // === Plan (1) ===

    /// Plan mode changed.
    "plan/modeChanged" => PlanModeChanged(PlanModeChangedParams),

    // === Queue (3) ===

    /// Command queue state changed.
    "queue/stateChanged" => QueueStateChanged { queued: i32 },
    /// Command queued.
    "queue/commandQueued" => CommandQueued { id: String, preview: String },
    /// Command dequeued.
    "queue/commandDequeued" => CommandDequeued { id: String },

    // === Rewind (2) ===

    /// File rewind completed.
    "rewind/completed" => RewindCompleted(RewindCompletedParams),
    /// File rewind failed.
    "rewind/failed" => RewindFailed { error: String },

    // === Cost (1) ===

    /// Cost threshold warning.
    "cost/warning" => CostWarning(CostWarningParams),

    // === Sandbox (2) ===

    /// Sandbox state changed.
    "sandbox/stateChanged" => SandboxStateChanged(SandboxStateChangedParams),
    /// Sandbox violations detected.
    "sandbox/violationsDetected" => SandboxViolationsDetected { count: i32 },

    // === Agent (1) ===

    /// Agents registered.
    "agents/registered" => AgentsRegistered { agents: Vec<AgentInfo> },

    // === Hook (3 — TS lifecycle trio) ===

    /// Hook execution started.
    "hook/started" => HookStarted(HookStartedParams),
    /// Hook execution progress (TS gap P1 — stdout/stderr streaming).
    "hook/progress" => HookProgress(HookProgressParams),
    /// Hook execution completed (TS gap P1).
    "hook/response" => HookResponse(HookResponseParams),

    // === Worktree (2) ===

    /// Entered a worktree.
    "worktree/entered" => WorktreeEntered(WorktreeEnteredParams),
    /// Exited a worktree.
    "worktree/exited" => WorktreeExited(WorktreeExitedParams),

    // === Summarize (2) ===

    /// Summarization completed.
    "summarize/completed" => SummarizeCompleted(SummarizeCompletedParams),
    /// Summarization failed.
    "summarize/failed" => SummarizeFailed { error: String },

    // === Stream health (3) ===

    /// Stream stall detected.
    "stream/stallDetected" => StreamStallDetected {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
    },
    /// Stream watchdog warning.
    "stream/watchdogWarning" => StreamWatchdogWarning { elapsed_secs: f64 },
    /// Stream request ended (with usage).
    "stream/requestEnd" => StreamRequestEnd { usage: TokenUsage },

    // === TS Gap P1: Session state (1) ===

    /// Session state changed (idle/running/requires_action).
    "session/stateChanged" => SessionStateChanged { state: SessionState },

    // === Max turns (1) ===

    /// Max turns reached.
    "turn/maxReached" => MaxTurnsReached {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_turns: Option<i32>,
    },

    // === TS gap P2: additional SDK notifications (5) ===

    /// Output from a user-executed local command (REPL `!` prefix).
    /// Matches TS `SDKLocalCommandOutputMessage` (coreSchemas.ts:1590-1602).
    "localCommand/output" => LocalCommandOutput(LocalCommandOutputParams),
    /// Files persisted to disk (file upload/snapshot completion).
    /// Matches TS `SDKFilesPersistedEvent` (coreSchemas.ts:1672-1692).
    "files/persisted" => FilesPersisted(FilesPersistedParams),
    /// MCP elicitation completed (form submission or cancellation).
    /// Matches TS `SDKElicitationCompleteMessage` (coreSchemas.ts:1779-1792).
    "elicitation/complete" => ElicitationComplete(ElicitationCompleteParams),
    /// Tool use summary from background haiku summarization.
    /// Matches TS `SDKToolUseSummaryMessage` (coreSchemas.ts:1769-1777).
    "tool/useSummary" => ToolUseSummary(ToolUseSummaryParams),
    /// Tool execution progress (bash/powershell long-running).
    /// Matches TS `SDKToolProgressMessage` (coreSchemas.ts:1648-1659).
    /// Sent at most once per 30 seconds per `parent_tool_use_id`.
    "tool/progress" => ToolProgress(ToolProgressParams),
    }
}

// Compile-time regression guard: keep `ServerNotification` from growing
// unbounded. The enum's size is the size of the largest variant; every
// `CoreEvent` pays this cost (inlined in mpsc channel buffers). If a new
// variant pushes this past the limit, either `Box<T>` the offending params
// (like `SessionResult(Box<SessionResultParams>)`) or justify raising the
// limit. Don't let it drift silently.
const _: () = assert!(
    std::mem::size_of::<ServerNotification>() <= 400,
    "ServerNotification exceeded 400 bytes; Box<T> the largest variant"
);

// ---------------------------------------------------------------------------
// ServerNotification param structs
// ---------------------------------------------------------------------------

/// Matches TS `SDKSystemMessageSchema` with subtype 'init' (coreSchemas.ts:1457-1494).
/// Sent once at session startup; carries the full bootstrap context the SDK
/// consumer needs to render a UI.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartedParams {
    pub session_id: String,
    /// coco-rs extension: protocol version negotiation.
    pub protocol_version: String,
    pub cwd: String,
    pub model: String,
    pub permission_mode: String,
    /// Builtin + MCP tool names.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Slash commands available in this session.
    #[serde(default)]
    pub slash_commands: Vec<String>,
    /// Agent type names available for Agent tool spawning.
    #[serde(default)]
    pub agents: Vec<String>,
    /// Skill names loaded.
    #[serde(default)]
    pub skills: Vec<String>,
    /// MCP server status at initialization.
    #[serde(default)]
    pub mcp_servers: Vec<McpServerInit>,
    /// Loaded plugin metadata.
    #[serde(default)]
    pub plugins: Vec<PluginInit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub betas: Vec<String>,
    /// Release version of the coco-rs binary.
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_mode_state: Option<FastModeState>,
}

/// MCP server init entry (inline struct in TS).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInit {
    pub name: String,
    pub status: crate::server_request::McpConnectionStatus,
}

/// Plugin init entry (inline struct in TS).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInit {
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Matches TS `SDKResultMessageSchema` (coreSchemas.ts:1407-1451).
/// TS has two subtype variants (success/error) unified here with `is_error` flag.
pub struct SessionResultParams {
    pub session_id: String,
    pub total_turns: i32,
    pub duration_ms: i64,
    pub duration_api_ms: i64,
    #[serde(default)]
    pub is_error: bool,
    pub stop_reason: String,
    pub total_cost_usd: f64,
    pub usage: TokenUsage,
    /// Per-model usage breakdown (TS `modelUsage: Record<string, ModelUsage>`).
    #[serde(default)]
    pub model_usage: std::collections::HashMap<String, SessionModelUsage>,
    /// Permission denials accumulated during the session.
    #[serde(default)]
    pub permission_denials: Vec<PermissionDenialInfo>,
    /// Success variant: the agent's final result text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Error variant: list of error strings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_mode_state: Option<FastModeState>,
    /// coco-rs extension: num_api_calls for observability (not in TS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_api_calls: Option<i32>,
}

/// Matches TS `ModelUsageSchema` (coreSchemas.ts:17-28).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionModelUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub web_search_requests: i64,
    pub cost_usd: f64,
    pub context_window: i64,
    pub max_output_tokens: i64,
}

/// Matches TS `SDKPermissionDenialSchema` (coreSchemas.ts:1399-1405).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDenialInfo {
    pub tool_name: String,
    pub tool_use_id: String,
    pub tool_input: serde_json::Value,
}

/// Matches TS `FastModeStateSchema` (coreSchemas.ts:1883-1889).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FastModeState {
    Off,
    Cooldown,
    On,
}

// ---------------------------------------------------------------------------
// TS gap P2: additional SDK notification params
// ---------------------------------------------------------------------------

/// Matches TS `SDKLocalCommandOutputMessage` (coreSchemas.ts:1590-1602).
///
/// TS emits this when the user runs a local bash command via the REPL `!`
/// prefix (not a tool call). The `content` field is the command output;
/// TS types it as the raw output structure (typically stdout/stderr).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalCommandOutputParams {
    pub content: serde_json::Value,
}

/// Matches TS `SDKFilesPersistedEvent` (coreSchemas.ts:1672-1692).
///
/// TS emits this when files are uploaded or persisted (e.g. after a
/// successful `filesApi` operation).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesPersistedParams {
    pub files: Vec<PersistedFileInfo>,
    #[serde(default)]
    pub failed: Vec<PersistedFileError>,
    pub processed_at: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedFileInfo {
    pub filename: String,
    pub file_id: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedFileError {
    pub filename: String,
    pub error: String,
}

/// Matches TS `SDKElicitationCompleteMessage` (coreSchemas.ts:1779-1792).
///
/// Emitted after an MCP server's elicitation request is resolved
/// (either submitted or cancelled).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationCompleteParams {
    pub mcp_server_name: String,
    pub elicitation_id: String,
}

/// Matches TS `SDKToolUseSummaryMessage` (coreSchemas.ts:1769-1777).
///
/// Background Haiku-based summary of a batch of tool uses. TS uses this
/// to compress verbose tool output before it's displayed or archived.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseSummaryParams {
    pub summary: String,
    pub preceding_tool_use_ids: Vec<String>,
}

/// Matches TS `SDKToolProgressMessage` (coreSchemas.ts:1648-1659).
///
/// Long-running tool progress (Bash, PowerShell). TS throttles emission to
/// ≤1 per 30 seconds per `parent_tool_use_id`. coco-rs StreamAccumulator
/// may emit this independently from `AgentStreamEvent::ToolUseStarted`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgressParams {
    pub tool_use_id: String,
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
    pub elapsed_time_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEndedParams {
    pub reason: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnStartedParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub turn_number: i32,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnCompletedParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub usage: TokenUsage,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnFailedParams {
    pub error: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnInterruptedParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDeltaParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub delta: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentSpawnedParams {
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentCompletedParams {
    pub agent_id: String,
    pub result: String,
    #[serde(default)]
    pub is_error: bool,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentBackgroundedParams {
    pub agent_id: String,
    pub output_file: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentProgressParams {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_steps: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStartupStatusParams {
    pub server: String,
    pub status: crate::server_request::McpConnectionStatus,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStartupCompleteParams {
    pub servers: Vec<String>,
    #[serde(default)]
    pub failed: Vec<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompactedParams {
    pub removed_messages: i32,
    pub summary_tokens: i32,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUsageWarningParams {
    pub estimated_tokens: i64,
    pub warning_threshold: i64,
    pub percent_left: f64,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionFailedParams {
    pub error: String,
    #[serde(default)]
    pub attempts: i32,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextClearedParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_mode: Option<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Matches TS `SDKTaskStartedMessage` (coreSchemas.ts:1715-1733).
/// TS has `description` required and `task_type` optional.
pub struct TaskStartedParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Matches TS `SDKTaskNotificationMessage` (coreSchemas.ts:1694-1713).
/// TS calls this `task/notification`; coco-rs uses `task/completed` as the
/// wire method for brevity, but fields match TS exactly.
pub struct TaskCompletedParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub status: TaskCompletionStatus,
    pub output_file: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TaskUsage>,
}

/// Matches TS `z.enum(['completed', 'failed', 'stopped'])` for task_notification status.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskCompletionStatus {
    Completed,
    Failed,
    Stopped,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Matches TS `SDKTaskProgressMessage` (coreSchemas.ts:1750-1767).
/// In TS, `description` and `usage` are required; other fields optional.
/// The `workflow_progress` field carries the streaming state of local_workflow
/// tasks — a delta batch of workflow state changes.
pub struct TaskProgressParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub description: String,
    pub usage: TaskUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workflow_progress: Vec<serde_json::Value>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUsage {
    pub total_tokens: i64,
    pub tool_uses: i32,
    pub duration_ms: i64,
}

/// A teammate's plan-approval request, surfaced to the team lead's
/// TUI for approve/deny. Payload byte-matches TS
/// `PlanApprovalRequestSchema` — see `tools/ExitPlanModeTool/`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanApprovalRequestedParams {
    /// Correlation id carried back in the response envelope.
    pub request_id: String,
    /// Teammate agent name.
    pub from: String,
    /// Optional on-disk plan file path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_file_path: Option<String>,
    /// Plan text rendered from the teammate's plan file.
    pub plan_content: String,
}

/// Snapshot of the task panel state — tools emit this post-mutation
/// so the TUI can redraw without reaching into `ToolAppState` directly.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPanelChangedParams {
    pub plan_tasks: Vec<crate::TaskRecord>,
    #[serde(default)]
    pub todos_by_agent: std::collections::HashMap<String, Vec<crate::TodoRecord>>,
    pub expanded_view: crate::ExpandedView,
    pub verification_nudge_pending: bool,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsKilledParams {
    pub count: i32,
    #[serde(default)]
    pub agent_ids: Vec<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFallbackParams {
    pub from_model: String,
    pub to_model: String,
    pub reason: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionModeChangedParams {
    pub mode: crate::PermissionMode,
    #[serde(default)]
    pub bypass_available: bool,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorParams {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default)]
    pub retryable: bool,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    // TS gap: enhanced fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<RateLimitStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization: Option<f64>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitStatus {
    Allowed,
    AllowedWarning,
    Rejected,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeSelectionChangedParams {
    pub file_path: String,
    pub selected_text: String,
    pub start_line: i32,
    pub end_line: i32,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeDiagnosticsUpdatedParams {
    pub file_path: String,
    pub new_count: i32,
    #[serde(default)]
    pub diagnostics: Vec<serde_json::Value>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanModeChangedParams {
    pub entered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<bool>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindCompletedParams {
    pub rewound_turn: i32,
    pub restored_files: i32,
    pub messages_removed: i32,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostWarningParams {
    pub current_cost_cents: i64,
    pub threshold_cents: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_cents: Option<i64>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxStateChangedParams {
    pub active: bool,
    pub enforcement: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookStartedParams {
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Matches TS `SDKHookProgressMessage` (coreSchemas.ts:1616-1629).
pub struct HookProgressParams {
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub output: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Matches TS `SDKHookResponseMessage` (coreSchemas.ts:1631-1646).
pub struct HookResponseParams {
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String,
    pub output: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub outcome: HookOutcomeStatus,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutcomeStatus {
    Success,
    Error,
    Cancelled,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeEnteredParams {
    pub worktree_path: String,
    pub branch: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeExitedParams {
    pub worktree_path: String,
    pub action: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarizeCompletedParams {
    pub from_turn: i32,
    pub summary_tokens: i32,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// Turn completed, waiting for user input.
    Idle,
    /// Agent is actively processing.
    Running,
    /// Waiting for user action (approval, question, elicitation).
    RequiresAction,
}

// ---------------------------------------------------------------------------
// TuiOnlyEvent — TUI-exclusive events (21 variants)
// ---------------------------------------------------------------------------

/// TUI-exclusive events.
///
/// These events are dropped by SDK and App-Server consumers. They drive
/// overlays, toasts, and UI-only state transitions that are not part of the
/// protocol contract.
///
/// Per `event-system-design.md` Section 1.7, the design listed this type as
/// owned by `coco-tui`. Since `CoreEvent::Tui(TuiOnlyEvent)` is part of the
/// envelope enum defined in `coco-types`, the type itself must live in
/// `coco-types` to avoid a cyclic dependency. The semantic contract
/// (TUI-only, never sent to SDK) is preserved via consumer dispatch rules
/// in `StreamAccumulator` and `handle_core_event()`.
///
/// 21 variants (20 from design §4.1 + 1 coco-rs extension).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TuiOnlyEvent {
    // === Permission / Question overlays (4) ===
    /// Permission approval overlay needed.
    ApprovalRequired {
        request_id: String,
        tool_name: String,
        description: String,
        input_preview: String,
    },
    /// AskUserQuestion overlay needed.
    QuestionAsked { request_id: String, message: String },
    /// MCP elicitation overlay needed.
    ElicitationRequested {
        request_id: String,
        server: String,
        schema: serde_json::Value,
    },
    /// Sandbox approval overlay needed.
    SandboxApprovalRequired {
        request_id: String,
        operation: String,
    },

    // === Picker data-ready events (4) ===
    /// Plugin picker data loaded.
    PluginDataReady { plugins: Vec<serde_json::Value> },
    /// Output style picker data loaded.
    OutputStylesReady { styles: Vec<String> },
    /// Rewind selector checkpoints loaded.
    RewindCheckpointsReady { checkpoints: Vec<serde_json::Value> },
    /// Rewind diff preview loaded.
    DiffStatsReady {
        message_id: String,
        files_changed: i32,
        insertions: i64,
        deletions: i64,
    },

    // === Compaction / speculation toasts (4) ===
    /// Compaction circuit breaker opened.
    CompactionCircuitBreakerOpen { failures: i32 },
    /// Micro-compaction applied notification.
    MicroCompactionApplied { removed: i32 },
    /// Session memory compaction applied notification.
    SessionMemoryCompactApplied { summary_tokens: i32 },
    /// Speculative execution rolled back.
    SpeculativeRolledBack { reason: String },

    // === Memory extraction toasts (3) ===
    /// Memory extraction started.
    SessionMemoryExtractionStarted,
    /// Memory extraction completed.
    SessionMemoryExtractionCompleted { extracted: i32 },
    /// Memory extraction failed.
    SessionMemoryExtractionFailed { error: String },

    // === Cron toasts (2) ===
    /// Cron job disabled by circuit breaker.
    CronJobDisabled { job_id: String, reason: String },
    /// Missed cron job fires.
    CronJobsMissed { count: i32 },

    // === Streaming tool display (3) ===
    /// Streaming tool input delta (typing effect).
    ///
    /// # Status: reserved scaffolding, not yet wired
    ///
    /// The TUI has a handler (`server_notification_handler::handle_tui_only`)
    /// that appends the delta to `ToolExecution.streaming_input` for a
    /// typing-effect display, but **no producer currently emits this variant**
    /// in coco-rs.
    ///
    /// The inference layer's `StreamEvent::ToolCallDelta` (a different type,
    /// internal to `coco-inference`) is fully accumulated into the complete
    /// tool input before the engine emits `AgentStreamEvent::ToolUseQueued`
    /// with the finalized input. Consumers see the complete input at once.
    ///
    /// Future work to wire this up would require the inference layer to
    /// forward the partial JSON fragments alongside the accumulation, and
    /// the engine to emit them here as `CoreEvent::Tui(ToolCallDelta { ... })`.
    ///
    /// # Why keep it in TuiOnlyEvent (not AgentStreamEvent)
    ///
    /// Per `event-system-design.md` §3.3: partial JSON deltas serve a purely
    /// UI display purpose (typing effect) and the SDK does not need them —
    /// `ToolUseQueued` already contains the complete input. Promoting to
    /// `AgentStreamEvent` would burden SDK consumers with partial JSON they
    /// must re-assemble, with no behavioral benefit.
    ToolCallDelta { call_id: String, delta: String },
    /// Tool progress update (progress bar).
    ToolProgress {
        tool_use_id: String,
        data: serde_json::Value,
    },
    /// Tool execution aborted notification.
    ToolExecutionAborted { tool_use_id: String, reason: String },

    // === coco-rs extensions (not in the design's 20) ===
    /// Rewind completed — TUI truncates messages and restores input state.
    /// coco-rs extension: UI-only because it carries TUI-specific identifiers
    /// for message truncation and input repopulation. Out-of-band from the
    /// design's `rewind/completed` ServerNotification which carries protocol
    /// metadata only.
    RewindCompleted {
        /// UUID of the target user message. Empty = code-only rewind.
        target_message_id: String,
        /// Number of files restored (0 if conversation-only).
        files_changed: i32,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "event.test.rs"]
mod tests;
