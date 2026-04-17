# coco-query — Crate Plan

TS source: `src/QueryEngine.ts`, `src/query.ts`, `src/query/tokenBudget.ts`, `src/query/buildQueryConfig.ts`

## Dependencies

```
coco-query depends on:
  - coco-types       (Message, TokenUsage, SessionId)
  - coco-config      (ModelRoles, ModelInfo — role resolution)
  - coco-inference   (ModelHub, ApiClient — LLM calls)
  - coco-tool        (ToolRegistry, StreamingToolExecutor, run_tools)
  - coco-context     (build_system_prompt, get_context_window)
  - coco-messages    (normalize_for_api, history)
  - coco-compact     (compact_conversation, should_auto_compact)
  - coco-permissions (evaluate_permission — for pre-tool hooks)
  - coco-hooks       (run_hooks — pre/post tool use)
  - coco-state       (AppState — shared state, lateral dep within app/)
  - coco-error

coco-query does NOT depend on:
  - coco-tools       (concrete tools — injected via ToolRegistry at init)
  - coco-commands    (commands — injected via CommandRegistry)
  - coco-skills      (skills — loaded into commands before query starts)
  - coco-shell       (used by BashTool inside coco-tools, not by query)
```

## Data Definitions

### QueryEngine (from `QueryEngine.ts`)

```rust
pub struct QueryEngine {
    config: QueryEngineConfig,
    tool_registry: Arc<ToolRegistry>,
    command_registry: Arc<CommandRegistry>,
    api_client: Arc<ApiClient>,          // vercel-ai based
    state: Arc<RwLock<AppState>>,
    messages: Vec<Message>,
    budget_tracker: BudgetTracker,
    total_usage: ModelUsage,             // cumulative API usage
    permission_denials: Vec<SdkPermissionDenial>,  // permission audit trail
    read_file_state: Arc<RwLock<FileStateCache>>,  // shared file read cache
    discovered_skill_names: HashSet<String>,        // cleared each turn
    loaded_nested_memory_paths: HashSet<String>,    // nested CLAUDE.md dedup
}

/// Tracks permission denials for SDK audit trail.
pub struct SdkPermissionDenial {
    pub tool_id: ToolId,
    pub input_summary: String,
    pub reason: String,
    pub timestamp: i64,
}

/// Recovery mechanism when tool fails after permission was already granted.
pub struct OrphanedPermission {
    pub tool_id: ToolId,
    pub tool_use_id: String,
    pub granted_input: Value,
}

/// Snip compaction boundary handler.
/// Called when a snip boundary is encountered during message replay.
pub type SnipReplayFn = Arc<dyn Fn(&Message, &[Message]) -> Option<SnipReplayResult> + Send + Sync>;

pub struct SnipReplayResult {
    pub messages: Vec<Message>,
    pub executed: bool,
}

pub struct QueryEngineConfig {
    pub cwd: PathBuf,
    pub tools: Arc<Vec<Arc<dyn Tool>>>,
    pub commands: Vec<Command>,
    pub mcp_clients: Vec<McpConnection>,
    pub agent_definitions: Vec<AgentDefinition>,
    pub read_file_cache: Arc<RwLock<FileStateCache>>,
    pub custom_system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub model: Option<String>,
    pub fallback_model: Option<String>,
    pub thinking_level: Option<ThinkingLevel>,
    pub max_turns: Option<i32>,
    pub max_budget_usd: Option<f64>,
    pub task_budget: Option<TaskBudget>,    // token budget for task
    pub json_schema: Option<Value>,         // structured output schema
    pub verbose: bool,
    pub replay_user_messages: bool,         // SDK message replay mode
    pub include_partial_messages: bool,     // include partial messages in output
    pub handle_elicitation: Option<Arc<dyn Fn(String, ElicitRequestParams, CancellationToken) -> BoxFuture<ElicitResult> + Send + Sync>>,
    pub abort_controller: Option<CancellationToken>,
    pub orphaned_permission: Option<OrphanedPermission>,
    pub snip_replay: Option<SnipReplayFn>,  // snip compaction boundary handler
    pub set_sdk_status: Option<Arc<dyn Fn(SdkStatus) + Send + Sync>>,
}

// TaskBudget is defined in coco-types (shared with coco-inference).
// Fields: total: i64, remaining: Option<i64>
```

### QueryConfig (from `query/config.ts`)

```rust
pub struct QueryConfig {
    pub session_id: SessionId,
    pub gates: QueryGates,
}

pub struct QueryGates {
    pub streaming_tool_execution: bool,
    pub emit_tool_use_summaries: bool,
    pub fast_mode_enabled: bool,
}
```

### Budget Tracker (from `query/tokenBudget.ts`)

```rust
pub struct BudgetTracker {
    pub continuation_count: i32,
    pub last_delta_tokens: i64,
    pub last_global_turn_tokens: i64,
    pub started_at: Instant,
}

pub enum BudgetDecision {
    Continue,
    Stop { reason: String },
    Nudge { message: String },
}

/// Budget logic:
/// - Max 3 continuations per turn
/// - Stop if < 500 tokens per continuation (diminishing returns)
/// - Stop at 90% budget threshold
pub fn check_budget(
    tracker: &BudgetTracker,
    budget: Option<f64>,
    global_tokens: i64,
) -> BudgetDecision;
```

### QueryEngine public API

QueryEngine emits `CoreEvent` (3-layer: Protocol / Stream / Tui) directly to
consumers via mpsc channel. See `event-system-design.md` for the full event
catalog. There is **no** `QueryEvent` intermediate type — that was an interim
scheme that has been removed in Phase 0 of the event system refactor.

```rust
use coco_types::CoreEvent;

impl QueryEngine {
    pub fn new(config: QueryEngineConfig) -> Self;

    /// Main conversation loop (async generator in TS → emit via mpsc in Rust).
    /// Emits CoreEvent directly; consumers pattern-match on the 3 layers.
    pub async fn run(&mut self, cancel: CancellationToken, event_tx: mpsc::Sender<CoreEvent>) -> Result<(), QueryError>;

    /// Interrupt the current turn. Cancels in-flight API call and tool execution.
    pub fn interrupt(&self);

    /// Get current session ID.
    pub fn session_id(&self) -> &SessionId;

    /// Switch model mid-session.
    pub fn set_model(&mut self, model: String);

    /// Get accumulated messages.
    pub fn messages(&self) -> &[Message];

    /// Get shared file read state cache.
    pub fn read_file_state(&self) -> &Arc<RwLock<FileStateCache>>;

    /// Get accumulated permission denials (for SDK audit trail).
    pub fn permission_denials(&self) -> &[SdkPermissionDenial];
}
```

## Core Logic

### Multi-Turn Loop (from `QueryEngine.ts`)

```rust
impl QueryEngine {
    /// Main conversation loop. Runs until end_turn or budget exhausted.
    pub async fn run(
        &mut self,
        cancel: CancellationToken,
        event_tx: mpsc::Sender<coco_types::CoreEvent>,
    ) -> Result<(), QueryError> {
        loop {
            // 1. Build system prompt
            let system = build_system_prompt(&self.config, &self.state);

            // 2. Normalize messages for API
            let api_messages = normalize_for_api(&self.messages);

            // 3. Call LLM via vercel-ai (streaming)
            let stream = self.api_client.query_streaming(QueryParams {
                messages: api_messages,
                model: self.config.model.clone(),
                system: Some(system),
                tools: Some(self.tool_registry.definitions()),
                thinking_level: self.config.thinking_level.clone(),
                ..Default::default()
            }, cancel.clone()).await?;

            // 4. Process stream events
            let assistant_msg = self.process_stream(stream, &event_tx).await?;
            self.messages.push(Message::Assistant(assistant_msg.clone()));

            // 5. Check stop reason
            match assistant_msg.stop_reason {
                Some(StopReason::EndTurn) => break,
                Some(StopReason::ToolUse) => {
                    // 6. Execute tools
                    let tool_results = run_tools(
                        &assistant_msg.tool_uses,
                        &assistant_msg,
                        &self.build_tool_context(),
                        cancel.clone(),
                    ).await;

                    // 7. Append results as user messages
                    for result in tool_results {
                        self.messages.push(result.into());
                    }
                }
                Some(StopReason::MaxTokens) => {
                    // 8. Retry with escalated budget or compact
                    self.handle_max_tokens().await?;
                }
                _ => break,
            }

            // 9. Auto-compact if needed
            if self.should_auto_compact() {
                self.compact(&event_tx).await?;
            }

            // 10. Check budget
            match check_budget(&self.budget_tracker, self.config.max_budget_usd, self.total_tokens()) {
                BudgetDecision::Stop { reason } => break,
                BudgetDecision::Nudge { message } => { /* inject nudge */ }
                BudgetDecision::Continue => {}
            }
        }
        Ok(())
    }
}
```

### Single-Turn Flow (from `query.ts`)

```rust
/// Execute a single query turn:
/// 1. Validate budget
/// 2. Build prompt (context + tools + memory)
/// 3. Call API (streaming)
/// 4. Collect assistant message
/// 5. Extract & execute tool calls (with permission hooks)
/// 6. Return results for next turn
pub async fn execute_turn(
    messages: &mut Vec<Message>,
    context: &ToolUseContext,
    config: &QueryConfig,
    cancel: CancellationToken,
) -> Result<TurnResult, QueryError>;

pub struct TurnResult {
    pub assistant_message: AssistantMessage,
    pub tool_results: Vec<Message>,
    pub should_continue: bool,
    pub compaction_needed: bool,
}
```

## Steering: Mid-Turn Message Queue & Injection (from `utils/messageQueueManager.ts` 548 LOC, `query.ts:1570-1590`, `utils/attachments.ts` 3760 LOC, `utils/queueProcessor.ts` 96 LOC, `utils/QueryGuard.ts`)

Steering allows users to send messages/guidance to the LLM while it is actively working,
without interrupting the current generation. Messages are queued and injected between
tool calls so the LLM sees new context and adapts direction.

### Command Queue

```rust
/// Priority ordering: Now(0) > Next(1) > Later(2). FIFO within same priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum QueuePriority { Now, Next, Later }

pub struct QueuedCommand {
    pub value: MessageContent,          // String or Vec<ContentBlock>
    pub mode: PromptInputMode,          // Prompt, Bash, TaskNotification
    pub priority: QueuePriority,        // Default: Next
    pub uuid: Option<String>,
    pub pasted_contents: Option<HashMap<i32, PastedContent>>,
    pub agent_id: Option<AgentId>,      // For subagent routing
    pub is_meta: bool,
}

pub enum PromptInputMode { Prompt, Bash, TaskNotification }

/// Module-level singleton queue with frozen snapshot + signal notification.
/// Rust equivalent: Arc<RwLock<CommandQueueState>> + tokio::sync::watch for change signal.
pub struct CommandQueue {
    commands: Vec<QueuedCommand>,       // Priority-sorted
    snapshot: Arc<[QueuedCommand]>,     // Frozen snapshot (updated on each mutation)
}

impl CommandQueue {
    pub fn enqueue(&mut self, cmd: QueuedCommand);
    pub fn enqueue_notification(&mut self, cmd: QueuedCommand);  // Default priority: Later
    pub fn dequeue(&mut self, filter: Option<&dyn Fn(&QueuedCommand) -> bool>) -> Option<QueuedCommand>;
    pub fn dequeue_all_matching(&mut self, predicate: impl Fn(&QueuedCommand) -> bool) -> Vec<QueuedCommand>;
    pub fn peek(&self, filter: Option<&dyn Fn(&QueuedCommand) -> bool>) -> Option<&QueuedCommand>;
    pub fn snapshot(&self) -> Arc<[QueuedCommand]>;
}
```

### Query Guard (3-State Synchronization Primitive)

```rust
/// Ensures queue processor does not fire while an LLM query is active.
/// 3-state machine with generation counter for stale-finally-block detection.
///
/// State machine:
///   idle ←──────────────────────────┐
///     │ reserve()                   │
///     ▼                             │
///   dispatching                     │
///     │ try_start()                 │
///     ▼                             │
///   running ──── end(gen) ──────────┘
///     │                             │
///     └── cancel_reservation() ─────┘ (dispatching → idle)
///
/// Generation counter: incremented on each try_start() and force_end().
/// end(gen) checks gen == current to detect stale finally blocks.
pub struct QueryGuard {
    status: QueryGuardStatus,      // Idle, Dispatching, Running
    generation: i64,               // incremented on start/force-end
    notify: tokio::sync::watch::Sender<bool>,
}

pub enum QueryGuardStatus { Idle, Dispatching, Running }

impl QueryGuard {
    /// Reserve query slot (idle → dispatching). Queue dequeued a command.
    pub fn reserve(&mut self);
    /// Cancel reservation (dispatching → idle). Queue had nothing to run.
    pub fn cancel_reservation(&mut self);
    /// Start query (idle|dispatching → running). Returns generation number.
    pub fn try_start(&mut self) -> i64;
    /// End query (running → idle). Only succeeds if gen matches current.
    pub fn end(&mut self, generation: i64) -> bool;
    /// Force end (any → idle). Increments gen to invalidate stale finally blocks.
    pub fn force_end(&mut self);
    /// True if dispatching or running (prevents re-entry from queue processor).
    pub fn is_active(&self) -> bool;
    /// Wait until status becomes idle.
    pub async fn wait_idle(&self);
}
```

### CommandQueue Priority

```rust
/// 3-level priority: now > next > later.
/// 'now': urgent commands (interrupts), processed immediately
/// 'next': user input (default for enqueue), processed before notifications
/// 'later': system notifications (default for enqueue_notification)
/// FIFO within same priority level.
/// Dequeue selects highest-priority (lowest ordinal) matching command.
pub enum QueuePriority { Now = 0, Next = 1, Later = 2 }
```

### Mid-Turn Attachment Injection

```rust
/// INJECTION POINT: Called AFTER each tool call completes, BEFORE next API request.
/// Located in the query loop between tool execution and the next LLM call.
///
/// Sources of injected messages:
/// 1. CommandQueue snapshot (user typed while LLM was working)
/// 2. AppState.inbox (teammate messages queued mid-turn)
/// 3. Memory prefetches (retrieved context)
/// 4. Skill discovery results
///
/// Deduplication: Uses "from|timestamp|text[..100]" as key.
/// After injection, inbox messages marked 'processed' and cleaned up on turn end.
pub async fn get_attachment_messages(
    queued_commands: &[QueuedCommand],
    context: &ToolUseContext,
    messages: &[Message],
) -> Vec<Message>;
```

### Queue Processing Strategy

```rust
/// Processing rules (from queueProcessor.ts):
/// - Slash commands (start with '/') → processed one-at-a-time individually
/// - Bash-mode commands → processed individually (per-command error isolation)
/// - Regular prompts → batched by mode (all same-mode prompts drained together)
/// - Different modes (Prompt vs TaskNotification) never mixed in a batch
pub fn process_queue(
    queue: &mut CommandQueue,
    execute: impl FnMut(Vec<QueuedCommand>),
);
```

### Inbox System (for Teammate Messages)

```rust
/// Async teammate messages delivered via AppState.inbox.
/// Two-phase delivery:
///   1. If session idle → submit immediately as new turn
///   2. If query active → queue in inbox, deliver via getAttachmentMessages() mid-turn
pub struct InboxMessage {
    pub id: String,
    pub from: String,
    pub text: String,
    pub timestamp: String,
    pub status: InboxStatus,  // Pending, Processing, Processed
    pub color: Option<String>,
    pub summary: Option<String>,
}

pub enum InboxStatus { Pending, Processing, Processed }
```

### Steering Flow

```
User types while LLM working
  → enqueue(QueuedCommand, priority=Next)
  → [LLM completes tool call N]
  → get_attachment_messages(queue_snapshot)   ← INJECTION POINT
  → Convert queued messages to user attachments
  → Inject into tool_results before next API call
  → LLM sees new context in turn N+1
  → LLM adapts direction without restart
  → [Turn ends] → QueryGuard.release()
  → Queue processor fires (idle detected)
```

---

### Events Emitted (CoreEvent from `event-system-design.md`)

**Phase 0 refactor removed the `QueryEvent` intermediate type.** QueryEngine
now emits `coco_types::CoreEvent` directly via mpsc channel, giving consumers
the full 3-layer protocol/stream/TUI dispatch.

During the agent loop, QueryEngine emits the following `CoreEvent` variants:

| Variant | When | Purpose |
|---------|------|---------|
| `Protocol(TurnStarted)` | Start of each turn | Carries turn_id and turn_number |
| `Protocol(TurnCompleted)` | End of each turn | Carries turn_id and TokenUsage |
| `Protocol(TurnFailed)` | On unrecoverable turn error | Emitted by agent driver on QueryEngine error |
| `Protocol(CompactionStarted)` | Reactive compaction begins | Signals retry with compacted context |
| `Protocol(ContextCompacted)` | Auto-compaction completes | Carries removed_messages and summary_tokens |
| `Protocol(Error)` | Budget nudge | category = "budget"; used for BudgetDecision::Nudge |
| `Protocol(QueueStateChanged)` | Command queue drained mid-turn | Reports remaining queue size |
| `Stream(TextDelta)` | LLM text output streaming | Per-turn_id delta, fed into StreamAccumulator |
| `Stream(ThinkingDelta)` | LLM reasoning output streaming | Per-turn_id delta |
| `Stream(ToolUseQueued)` | Tool call parsed from LLM response | Carries complete input; accumulator creates ThreadItem |
| `Stream(ToolUseCompleted)` | Tool execution result ready | Carries tool name + output + is_error |

See `event-system-design.md` Sections 1.4 – 1.6 for the full type catalog.

### StreamAccumulator (AgentStreamEvent → ServerNotification)

Stateful converter that translates streaming deltas into semantic
`ServerNotification::ItemStarted / ItemUpdated / ItemCompleted` events with
ThreadItem tool mapping. Used by SDK output and optionally by TUI for
semantic display.

```rust
use coco_types::{AgentStreamEvent, ServerNotification};

pub struct StreamAccumulator {
    // Private state:
    //   text_item_id, text_buffer
    //   thinking_item_id, thinking_buffer
    //   active_items: HashMap<call_id, ThreadItem>
    //   item_counter: i64
}

impl StreamAccumulator {
    pub fn new(turn_id: impl Into<String>) -> Self;
    /// Process one stream event; returns zero or more notifications.
    pub fn process(&mut self, event: AgentStreamEvent) -> Vec<ServerNotification>;
    /// Flush pending text/thinking items at turn end.
    pub fn flush(&mut self) -> Vec<ServerNotification>;
}
```

**State transitions** (see `event-system-design.md` §6.1):
```
ThinkingDelta* → TextDelta* → ToolUseQueued → ToolUseStarted → ToolUseCompleted
     ↓                ↓              ↓                ↓                ↓
ItemStarted      ItemStarted    ItemStarted     ItemUpdated     ItemCompleted
(Reasoning)      (AgentMsg)     (tool-specific)
```

**Tool mapping** (`build_tool_details()` in `stream_accumulator.rs`):

| Tool name | ThreadItemDetails variant |
|-----------|--------------------------|
| `Bash`, `PowerShell` | `CommandExecution { command, output, exit_code, status }` |
| `Edit`, `Write`, `NotebookEdit` | `FileChange { changes: Vec<FileChangeInfo>, status }` |
| `WebSearch` | `WebSearch { query, status }` |
| `mcp__<server>__<tool>` | `McpToolCall { server, tool, arguments, result, error, status }` |
| `Agent`, `Task` | `Subagent { agent_id, agent_type, description, is_background, result, status }` |
| all others (Read, Glob, Grep, ...) | `ToolCall { tool, input, output, is_error, status }` |

Transition rules:
- Text/thinking items auto-flush when a tool starts or the opposite content type arrives
- Tool errors mark `status = Failed`; success marks `Completed`
- MCP tool events (`McpToolCallBegin/End`) create `McpToolCall` items directly
