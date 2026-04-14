# coco-rs Event System Design

> Base: cocode-rs `common/protocol` + `app-server-protocol`
> Supplement: Claude Code TS `SDKMessage` + `controlSchemas` capabilities
> Principle: Build on the cocode-rs three-layer CoreEvent architecture, supplementing it with the SDK consumer capabilities from TS

## 1. Architecture Overview

### 1.1 cocode-rs Three-Layer Event Envelope (KEEP)

```
┌─────────────────────────────────────────────────────────────────┐
│                     Agent Loop (core/loop)                       │
│                                                                   │
│  emit(CoreEvent::Protocol(ServerNotification))  → all consumers  │
│  emit(CoreEvent::Stream(AgentStreamEvent))       → needs accum.   │
│  emit(CoreEvent::Tui(TuiEvent))                 → TUI exclusive  │
└─────────────────────────────┬───────────────────────────────────┘
                              │ mpsc::channel<CoreEvent>
              ┌───────────────┼───────────────┐
              │               │               │
         ┌────▼─────┐   ┌────▼──────┐   ┌────▼──────┐
         │   TUI    │   │  SDK/CLI  │   │App-Server │
         │ all 3    │   │ Protocol  │   │ Protocol  │
         │ layers   │   │ + Stream  │   │ + Stream  │
         └────┬─────┘   └────┬──────┘   └────┬──────┘
              │              │               │
         in-process     NDJSON stdio    WebSocket/stdio
         channels       (Python SDK)     (IDE plugins)
```

**Core design advantages over TS**:
- TS uses a flat `SDKMessage` union (24 variants) with no consumer differentiation
- cocode-rs uses a three-layer `CoreEvent` with explicit separation; TuiEvent does not leak to SDK
- TS requires `convertSDKMessage()` to reverse-parse for UI; cocode-rs dispatches directly

### 1.2 TS Actual Model (Reference)

TS `query()` is an async generator that directly yields **core data types**, not SDK-specific wrappers:

```typescript
// query.ts:219-228 — actual signature
async function* query(params: QueryParams): AsyncGenerator<
  | StreamEvent          // raw Anthropic SDK stream event (message_start, content_block_delta, ...)
  | RequestStartEvent    // { type: 'stream_request_start' }
  | Message              // UserMessage | AssistantMessage | SystemMessage | ...
  | TombstoneMessage     // { type: 'tombstone', message: Message }
  | ToolUseSummaryMessage // { type: 'tool_use_summary', summary, preceding_tool_use_ids }
, Terminal>
```

**Consumer-side adaptation** (not producer-side conversion):
- **TUI (REPL.tsx)**: `for await (event of query(...))` → `handleMessageFromStream(event, callbacks...)` dispatches to 8 callbacks (onMessage, onStreamingText, onStreamingToolUses, onSetStreamMode, onTombstone, onStreamingThinking, onApiMetrics, onUpdateLength)
- **SDK (print.ts)**: `normalizeMessage()` converts internal Message → SDKMessage (24 variants) → `structuredIO.write()` → NDJSON stdout
- **Background tasks**: `sdkEventQueue` collects task_started/task_progress/task_notification events, drained via `drainSdkEvents()` before result emission

Key insight: TS yields **the same core types** to both TUI and SDK consumers. The conversion to SDKMessage happens only at the SDK serialization boundary (`normalizeMessage()` in `queryHelpers.ts`), not in the agent loop.

### 1.3 Decision: Keep the cocode-rs Three-Layer Architecture

The cocode-rs design is superior because:
- TS `handleMessageFromStream()` must dispatch on 5 unrelated types with type-checking workarounds; CoreEvent provides type-safe 3-way dispatch
- TS `normalizeMessage()` converts at SDK boundary, losing the streaming position information; CoreEvent::Stream preserves it
- TS mixes UI-only events (SpinnerMode) into the same stream; CoreEvent::Tui isolates them

What needs to be done:
1. Supplement the `ServerNotification` layer with SDK-visible events from TS
2. Supplement the `TuiEvent` layer with UI-only events from TS
3. Supplement the `ClientRequest`/`ServerRequest` layer with the TS control protocol
4. Ensure `StreamAccumulator` covers all streaming conversion scenarios

### 1.4 CoreEvent Envelope Definition

> **Naming**: The event-system's stream layer is called `AgentStreamEvent` (not `StreamEvent`) to avoid collision with `coco_types::StreamEvent` which represents raw inference-layer LLM stream events. `AgentStreamEvent` is the agent-loop-processed version with tool lifecycle semantics and MCP events.

```rust
/// Three-layer event envelope. All consumers receive CoreEvent via mpsc channel.
/// Defined in coco-types (shared across 3+ crates: coco-query, coco-tui, coco-cli).
#[derive(Debug, Clone)]
pub enum CoreEvent {
    /// Protocol-level notifications visible to ALL consumers (TUI, SDK, IDE, App-Server).
    /// 56 base variants + 9 TS gap additions = 65 total (see §2 for the catalog).
    /// Covers session/turn/item/content/subagent/MCP/context/task/model/permission/
    /// system/IDE/plan/queue/rewind/cost/sandbox/agent/hook/worktree/summarize/stream lifecycle.
    Protocol(ServerNotification),

    /// Agent-loop stream events requiring accumulation before SDK consumption.
    /// TUI consumes directly for real-time display; SDK passes through StreamAccumulator
    /// which converts them to Protocol(ItemStarted/Updated/Completed) notifications.
    Stream(AgentStreamEvent),

    /// TUI-exclusive events (overlays, toasts, streaming deltas for display).
    /// SDK and App-Server consumers DROP these events.
    Tui(TuiEvent),
}
```

### 1.5 AgentStreamEvent Definition

```rust
/// Agent-loop stream events. These are higher-level than coco_types::StreamEvent
/// (which represents raw LLM inference deltas). AgentStreamEvent adds:
/// - Tool lifecycle states (Queued → Started → Completed)
/// - MCP tool call tracking
/// - Turn-scoped item IDs
///
/// Defined in coco-types. Input to StreamAccumulator.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentStreamEvent {
    /// Text content delta from assistant response.
    TextDelta { turn_id: String, delta: String },
    /// Thinking/reasoning delta from extended thinking.
    ThinkingDelta { turn_id: String, delta: String },
    /// Tool use block received from API (input complete). Creates a ThreadItem.
    ToolUseQueued { call_id: String, name: String, input: serde_json::Value },
    /// Tool execution has begun (after permission check).
    ToolUseStarted { call_id: String, name: String, batch_id: Option<String> },
    /// Tool execution completed with result.
    ToolUseCompleted { call_id: String, output: String, is_error: bool },
    /// MCP tool call initiated (separate from builtin tools).
    McpToolCallBegin { server: String, tool: String, call_id: String },
    /// MCP tool call completed.
    McpToolCallEnd { server: String, tool: String, call_id: String, is_error: bool },
}
```

### 1.6 ThreadItem and ItemStatus Definitions

```rust
/// Semantic representation of a conversation thread item.
/// Produced by StreamAccumulator from AgentStreamEvent sequences.
/// Used in ServerNotification::ItemStarted/ItemUpdated/ItemCompleted.
///
/// Defined in coco-types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadItem {
    pub item_id: String,
    pub turn_id: String,
    pub details: ThreadItemDetails,
}

/// Tool-specific semantic mapping (see Section 6.2 for mapping rules).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadItemDetails {
    /// Bash tool → command execution with output.
    CommandExecution {
        command: String,
        output: String,
        exit_code: Option<i32>,
        status: ItemStatus,
    },
    /// Edit/Write tools → file change with diff info.
    FileChange {
        changes: Vec<FileChangeInfo>,
        status: ItemStatus,
    },
    /// WebSearch tool.
    WebSearch {
        query: String,
        status: ItemStatus,
    },
    /// MCP server tool call.
    McpToolCall {
        server: String,
        tool: String,
        arguments: serde_json::Value,
        result: Option<String>,
        error: Option<String>,
        status: ItemStatus,
    },
    /// Agent/Task tool → subagent lifecycle.
    Subagent {
        agent_id: String,
        agent_type: String,
        description: String,
        is_background: bool,
        result: Option<String>,
        status: ItemStatus,
    },
    /// All other tools (Read, Glob, Grep, etc.).
    ToolCall {
        tool: String,
        input: serde_json::Value,
        output: Option<String>,
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

pub struct FileChangeInfo {
    pub path: String,
    pub kind: String, // "create", "modify", "delete"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    InProgress,
    Completed,
    Failed,
    Declined,
}
```

### 1.7 Type Ownership

| Type | Owner Crate | Rationale |
|------|-------------|-----------|
| `CoreEvent` | `coco-types` | Shared across coco-query (producer), coco-tui, coco-cli, coco-bridge (consumers) |
| `ServerNotification` (56 base + 9 TS gaps = 65 target) | `coco-types` | Protocol-level, shared across all consumers and serialized to SDK. Phase 0 implemented 56 base + 4 P1 gaps = 60. |
| `AgentStreamEvent` (7 variants) | `coco-types` | Shared across coco-query (producer) and coco-tui/StreamAccumulator (consumers) |
| `TuiOnlyEvent` (20 variants) | `coco-types` | Although UI-exclusive in semantics, the type must live in `coco-types` because `CoreEvent::Tui(TuiOnlyEvent)` is part of the envelope enum defined in `coco-types`. Moving it to `coco-tui` would create a cyclic dependency (coco-types → coco-tui → coco-types). The TUI-only semantic contract is preserved via consumer dispatch rules in `StreamAccumulator` and `handle_core_event()` — SDK and App-Server consumers drop these events. |
| `ThreadItem`, `ThreadItemDetails`, `ItemStatus` | `coco-types` | Used in ServerNotification params and StreamAccumulator output |
| `StreamAccumulator` | `coco-query` | Stateful converter, only used inside the query/SDK output path |
| `ClientRequest`, `ServerRequest` | `coco-types` | Protocol-level, shared across SDK server and transport layers |
| `TuiNotification` (17 variants) | `coco-tui` | Internal TUI state-update type. Not part of the protocol. The translator `core_event_to_tui_notifications()` converts `CoreEvent` to zero or more `TuiNotification`s so the existing TUI state mutation logic remains stable. |
| `TuiEvent` (terminal input) | `coco-tui` | Crossterm input events (Key, Mouse, Resize, Tick, SpinnerTick, Paste). Completely distinct from `TuiOnlyEvent`; not part of the CoreEvent envelope. |

> **Naming collision note**: `coco-tui` has two "event" types with different purposes:
> - `TuiEvent` (in `tui/src/events.rs`): low-level terminal input (Crossterm events + timers).
> - `TuiOnlyEvent` (in `coco-types/src/event.rs`): high-level UI overlay/toast events that flow through `CoreEvent::Tui(...)`.
>
> Do not conflate these. The former is produced by the terminal event stream; the latter is produced by the agent loop or CLI handlers and consumed by the TUI as part of `CoreEvent` dispatch.

> **Note on coco_types::StreamEvent**: The existing `StreamEvent` in `crate-coco-types.md` (7 variants: TextDelta, ThinkingDelta, ToolUseStart, ToolUseInput, ToolUseEnd, RequestStart, MessageComplete) represents **raw inference-layer** LLM stream output. It is consumed by QueryEngine internally and converted to `AgentStreamEvent` for the CoreEvent channel. These are two distinct types at different abstraction layers; both live in coco-types.

### 1.8 Migration from Current Implementation

The current coco-rs implementation uses `QueryEvent` (13 variants) as an interim event type, mapped to `ServerNotification` (17 variants) via `map_query_event()` in `tui_runner.rs`. This is **not in the target design** and should be replaced:

1. **Delete `QueryEvent`** — Replace with `CoreEvent` emission in QueryEngine
2. **Move `ServerNotification`** — From TUI crate to coco-types, expand from 17 → 60 variants (56 base + 4 P1 gaps)
3. **Delete `map_query_event()`** — No intermediate mapping needed; QueryEngine emits CoreEvent directly
4. **TUI consumes CoreEvent** — Replace `notification_rx: Receiver<ServerNotification>` with `Receiver<CoreEvent>`
5. **Add AgentStreamEvent emission** — QueryEngine emits `CoreEvent::Stream(...)` for streaming deltas instead of `CoreEvent::Protocol(TextDelta)`

---

## 2. Event Catalog: ServerNotification (Protocol Layer)

### 2.1 Existing (56 variants) — From cocode-rs Base

These are the `ServerNotification` variants available in the cocode-rs reference
implementation, which form the foundation for coco-rs. Phase 0 of the refactor
has **implemented all 56 in `coco-types::ServerNotification`** plus 4 of the 9
TS gap additions from §2.2 (all P1 priority). See `audit-gaps.md` Round 8.

> **Counting note**: Earlier revisions of this doc claimed "43 variants"; a manual
> row count of the table below yields **56**. The error did not affect the design
> itself — only the summary arithmetic. Corrected in the Phase 0 review.

| Category | Variant | Wire Method | Params |
|----------|---------|-------------|--------|
| **Session** | `SessionStarted` | `session/started` | session_id, protocol_version, models?, commands? |
| | `SessionResult` | `session/result` | session_id, total_turns, total_cost_cents?, duration_ms, duration_api_ms?, usage, stop_reason, structured_output? |
| | `SessionEnded` | `session/ended` | reason: SessionEndedReason |
| **Turn** | `TurnStarted` | `turn/started` | turn_id, turn_number |
| | `TurnCompleted` | `turn/completed` | turn_id, usage |
| | `TurnFailed` | `turn/failed` | error |
| | `TurnInterrupted` | `turn/interrupted` | turn_id? |
| | `MaxTurnsReached` | `turn/maxReached` | max_turns? |
| **Item** | `ItemStarted` | `item/started` | item: ThreadItem |
| | `ItemUpdated` | `item/updated` | item: ThreadItem |
| | `ItemCompleted` | `item/completed` | item: ThreadItem |
| **Content** | `AgentMessageDelta` | `agentMessage/delta` | item_id, turn_id, delta |
| | `ReasoningDelta` | `reasoning/delta` | item_id, turn_id, delta |
| **Subagent** | `SubagentSpawned` | `subagent/spawned` | agent_id, agent_type, description, color? |
| | `SubagentCompleted` | `subagent/completed` | agent_id, result, is_error |
| | `SubagentBackgrounded` | `subagent/backgrounded` | agent_id, output_file |
| | `SubagentProgress` | `subagent/progress` | agent_id, message?, current_step?, total_steps?, summary? |
| **MCP** | `McpStartupStatus` | `mcp/startupStatus` | server, status |
| | `McpStartupComplete` | `mcp/startupComplete` | servers, failed |
| **Context** | `ContextCompacted` | `context/compacted` | removed_messages, summary_tokens |
| | `ContextUsageWarning` | `context/usageWarning` | estimated_tokens, warning_threshold, percent_left |
| | `CompactionStarted` | `context/compactionStarted` | (empty) |
| | `CompactionFailed` | `context/compactionFailed` | error, attempts |
| | `ContextCleared` | `context/cleared` | new_mode |
| **Task** | `TaskStarted` | `task/started` | task_id, task_type |
| | `TaskCompleted` | `task/completed` | task_id, result, is_error |
| | `TaskProgress` | `task/progress` | task_id, message? |
| | `AgentsKilled` | `agents/killed` | count, agent_ids |
| **Model** | `ModelFallbackStarted` | `model/fallbackStarted` | from_model, to_model, reason |
| | `ModelFallbackCompleted` | `model/fallbackCompleted` | (empty) |
| | `FastModeChanged` | `model/fastModeChanged` | active |
| **Permission** | `PermissionModeChanged` | `permission/modeChanged` | mode, bypass_available |
| **Prompt** | `PromptSuggestion` | `prompt/suggestion` | suggestions: Vec |
| **System** | `Error` | `error` | message, category?, retryable, error_info? |
| | `RateLimit` | `rateLimit` | remaining?, reset_at?, limit?, provider? |
| | `KeepAlive` | `keepAlive` | timestamp |
| **IDE** | `IdeSelectionChanged` | `ide/selectionChanged` | file_path, selected_text, start_line, end_line |
| | `IdeDiagnosticsUpdated` | `ide/diagnosticsUpdated` | file_path, new_count, diagnostics |
| **Plan** | `PlanModeChanged` | `plan/modeChanged` | entered, plan_file?, approved? |
| **Queue** | `QueueStateChanged` | `queue/stateChanged` | queued |
| | `CommandQueued` | `queue/commandQueued` | id, preview |
| | `CommandDequeued` | `queue/commandDequeued` | id |
| **Rewind** | `RewindCompleted` | `rewind/completed` | rewound_turn, restored_files, messages_removed |
| | `RewindFailed` | `rewind/failed` | error |
| **Cost** | `CostWarning` | `cost/warning` | current_cost_cents, threshold_cents, budget_cents? |
| **Sandbox** | `SandboxStateChanged` | `sandbox/stateChanged` | active, enforcement |
| | `SandboxViolationsDetected` | `sandbox/violationsDetected` | count |
| **Agent** | `AgentsRegistered` | `agents/registered` | agents: Vec<AgentInfo> |
| **Hook** | `HookExecuted` | `hook/executed` | hook_type, hook_name |
| **Worktree** | `WorktreeEntered` | `worktree/entered` | worktree_path, branch |
| | `WorktreeExited` | `worktree/exited` | worktree_path, action |
| **Summarize** | `SummarizeCompleted` | `summarize/completed` | from_turn, summary_tokens |
| | `SummarizeFailed` | `summarize/failed` | error |
| **Stream** | `StreamStallDetected` | `stream/stallDetected` | turn_id? |
| | `StreamWatchdogWarning` | `stream/watchdogWarning` | elapsed_secs |
| | `StreamRequestEnd` | `stream/requestEnd` | usage |

### 2.2 Gaps: Protocol Events Present in TS but Missing from cocode-rs

The following events exist in TS `SDKMessage` and need to be added to `ServerNotification`:

| # | Proposed Variant | Wire Method | TS Source | Params | Priority | Status |
|---|-----------------|-------------|-----------|--------|----------|--------|
| 1 | `HookStarted` | `hook/started` | `SDKHookStartedMessage` | hook_id, hook_name, hook_event | P1 | ✅ implemented |
| 2 | `HookProgress` | `hook/progress` | `SDKHookProgressMessage` | hook_id, hook_name, hook_event, stdout, stderr, output | P1 | ✅ implemented |
| 3 | `HookResponse` | `hook/response` | `SDKHookResponseMessage` | hook_id, hook_name, hook_event, output, stdout, stderr, exit_code?, outcome | P1 | ✅ implemented |
| 4 | `SessionStateChanged` | `session/stateChanged` | `SDKSessionStateChangedMessage` | state: idle/running/requires_action | P1 | ✅ implemented |
| 5 | `LocalCommandOutput` | `localCommand/output` | `SDKLocalCommandOutputMessage` | content | P2 | ✅ implemented |
| 6 | `FilesPersisted` | `files/persisted` | `SDKFilesPersistedEvent` | files: Vec<{filename, file_id}>, failed: Vec<{filename, error}>, processed_at | P2 | ✅ implemented |
| 7 | `ElicitationComplete` | `elicitation/complete` | `SDKElicitationCompleteMessage` | mcp_server_name, elicitation_id | P2 | ✅ implemented |
| 8 | `ToolUseSummary` | `tool/useSummary` | `SDKToolUseSummaryMessage` | summary, preceding_tool_use_ids: Vec | P2 | ✅ implemented |
| 9 | `ToolProgress` | `tool/progress` | `SDKToolProgressMessage` | tool_use_id, tool_name, parent_tool_use_id, elapsed_time_seconds, task_id? | P1 | ✅ implemented |

> **AuthStatus removed from the gap list** (April 2026): The TS `SDKAuthStatusMessage` is bespoke to Claude Code's OAuth flow and does not apply to coco-rs's multi-provider auth model. coco-rs tracks auth via `coco-inference` retry events and MCP auth status (`McpAuthStatus`) independently. No ServerNotification equivalent needed.

**Priority summary**:
- **P1 (affects core logic)**: 5 events (Hook lifecycle ×3, SessionStateChanged, ToolProgress)
- **P2 (nice-to-have)**: 4 events (LocalCommandOutput, FilesPersisted, ElicitationComplete, ToolUseSummary)

### 2.3 cocode-rs Exclusive Events (Not in TS, KEEP)

The following events are design enhancements in cocode-rs with no TS counterpart:

| Variant | Value |
|---------|-------|
| `StreamStallDetected` / `StreamWatchdogWarning` | Stream health monitoring — TS has internal stall detection but does not expose it to the SDK |
| `SummarizeCompleted` / `SummarizeFailed` | Partial compaction observability |
| `SandboxStateChanged` / `SandboxViolationsDetected` | Sandbox lifecycle — TS sandbox does not expose state events |
| `CostWarning` | Cost alerting — TS passes cost implicitly via result.total_cost_usd |
| `QueueStateChanged` / `CommandQueued` / `CommandDequeued` | Command queue observability — TS QueryGuard does not expose this |
| `IdeSelectionChanged` / `IdeDiagnosticsUpdated` | IDE integration events — TS uses a private CCR bridge protocol |
| `AgentsRegistered` / `AgentsKilled` | Agent registration lifecycle |
| `WorktreeEntered` / `WorktreeExited` | Worktree lifecycle events |

---

## 3. Event Catalog: AgentStreamEvent (Accumulation Layer)

> **Not to be confused with** `coco_types::StreamEvent` (inference-layer raw LLM stream). See Section 1.5 for the distinction.

### 3.1 Existing (7 variants) — KEEP

| Variant | Fields | Accumulates To |
|---------|--------|---------------|
| `TextDelta` | turn_id, delta | → ItemStarted(AgentMessage) + AgentMessageDelta + ItemCompleted |
| `ThinkingDelta` | turn_id, delta | → ItemStarted(Reasoning) + ReasoningDelta + ItemCompleted |
| `ToolUseQueued` | call_id, name, input | → ItemStarted(tool-specific ThreadItem) |
| `ToolUseStarted` | call_id, name, batch_id? | → ItemUpdated |
| `ToolUseCompleted` | call_id, output, is_error | → ItemCompleted |
| `McpToolCallBegin` | server, tool, call_id | → ItemStarted(McpToolCall) |
| `McpToolCallEnd` | server, tool, call_id, is_error | → ItemCompleted(McpToolCall) |

### 3.2 TS Comparison

TS raw `StreamEvent` (from `@anthropic-ai/sdk`) is internal to `queryModelWithStreaming()` in `claude.ts`. It is NOT exposed to SDK consumers — TS converts it to `SDKPartialAssistantMessage` (type: `'stream_event'`, wrapping the raw event) for SDK output, or passes it through `handleMessageFromStream()` for TUI consumption.

**TS raw stream event types** (from Anthropic SDK, consumed inside `queryModelWithStreaming()`):
- `message_start` / `message_stop`
- `content_block_start` / `content_block_stop` (with `content_block.type`: `text`, `thinking`, `tool_use`)
- `content_block_delta` (with `delta.type`: `text_delta`, `thinking_delta`, `input_json_delta`)
- `message_delta` (stop_reason, usage)

The 7 `AgentStreamEvent` variants are a high-level abstraction of these raw SSE events, adding tool lifecycle semantics (Queued → Started → Completed) and MCP tracking that TS handles implicitly inside `query()`. No alignment needed.

### 3.3 Gap: ToolCallDelta in Stream Layer

TS passes partial tool call JSON (streaming tool input) via `content_block_delta` with `delta.type === 'input_json_delta'`. In TS TUI, `handleMessageFromStream()` dispatches this to `onStreamingToolUses()` which accumulates the JSON string in `StreamingToolUse.unparsedToolInput`. cocode-rs places the equivalent `ToolCallDelta` in `TuiEvent`.

**Decision**: Keep the status quo — `ToolCallDelta` serves a purely UI display purpose (showing a typing effect for tool input) and the SDK does not need partial JSON. `ToolUseQueued` already contains the complete input.

---

## 4. Event Catalog: TuiEvent (UI-Only Layer)

### 4.1 Existing (20 variants) — KEEP

| Variant | Purpose | TS Equivalent |
|---------|---------|--------------|
| `ApprovalRequired` | Permission prompt overlay | SDKControlPermissionRequest |
| `QuestionAsked` | AskUserQuestion overlay | - (TS uses control flow) |
| `ElicitationRequested` | MCP elicitation overlay | SDKControlElicitationRequest |
| `SandboxApprovalRequired` | Sandbox permission overlay | - (TS: inline in permission) |
| `PluginDataReady` | Plugin picker data | - (TS: in-process) |
| `OutputStylesReady` | Output style picker | - (TS: in-process) |
| `RewindCheckpointsReady` | Rewind selector | - (TS: in-process) |
| `DiffStatsReady` | Rewind diff preview | - (TS: in-process) |
| `CompactionCircuitBreakerOpen` | Circuit breaker toast | - (TS: console.warn) |
| `MicroCompactionApplied` | Compaction toast | - (TS: internal) |
| `SessionMemoryCompactApplied` | Memory compaction toast | - (TS: internal) |
| `SessionMemoryExtractionStarted` | Memory extraction status | - (TS: internal) |
| `SessionMemoryExtractionCompleted` | Memory extraction done | - (TS: internal) |
| `SessionMemoryExtractionFailed` | Memory extraction error | - (TS: internal) |
| `SpeculativeRolledBack` | Speculation rollback toast | - (TS: internal) |
| `CronJobDisabled` | Cron circuit breaker toast | - (TS: N/A) |
| `CronJobsMissed` | Missed cron toast | - (TS: N/A) |
| `ToolCallDelta` | Streaming tool input display | - (TS: stream_event internal) |
| `ToolProgress` | Tool progress bar | SDKToolProgressMessage (partial) |
| `ToolExecutionAborted` | Abort toast | - (TS: internal) |

### 4.2 No Gaps

TS UI-facing events are either already covered in `TuiEvent` or covered via `ServerNotification`. The cocode-rs `TuiEvent` is richer than TS UI events (compaction details, speculation, cron).

---

## 5. Bidirectional Protocol: ClientRequest / ServerRequest

### 5.1 Existing ClientRequest (22 variants) — Already Implemented in cocode-rs

| Method | Variant | Purpose |
|--------|---------|---------|
| `initialize` | `Initialize` | Version/capability negotiation |
| `session/start` | `SessionStart` | New session with full config |
| `session/resume` | `SessionResume` | Resume saved session |
| `session/list` | `SessionList` | List saved sessions |
| `session/read` | `SessionRead` | Read session items |
| `session/archive` | `SessionArchive` | Archive session |
| `turn/start` | `TurnStart` | New turn with user input |
| `turn/interrupt` | `TurnInterrupt` | Interrupt current turn |
| `approval/resolve` | `ApprovalResolve` | Resolve permission prompt |
| `input/resolveUserInput` | `UserInputResolve` | Answer model question |
| `control/setModel` | `SetModel` | Change model mid-session |
| `control/setPermissionMode` | `SetPermissionMode` | Change permission mode |
| `control/stopTask` | `StopTask` | Stop background task |
| `control/setThinking` | `SetThinking` | Change thinking config |
| `control/rewindFiles` | `RewindFiles` | Rewind to previous turn |
| `control/updateEnv` | `UpdateEnv` | Update env vars |
| `control/keepAlive` | `KeepAlive` | Prevent idle timeout |
| `control/cancelRequest` | `CancelRequest` | Cancel pending server request |
| `config/read` | `ConfigRead` | Read effective config |
| `config/value/write` | `ConfigWrite` | Write config value |
| `hook/callbackResponse` | `HookCallbackResponse` | Respond to hook callback |
| `mcp/routeMessageResponse` | `McpRouteMessageResponse` | MCP message routing response |

### 5.2 Existing ServerRequest (5 variants) — Already Implemented in cocode-rs

| Method | Variant | Purpose |
|--------|---------|---------|
| `approval/askForApproval` | `AskForApproval` | Request tool approval |
| `input/requestUserInput` | `RequestUserInput` | Request user input |
| `mcp/routeMessage` | `McpRouteMessage` | Route MCP message to client |
| `hook/callback` | `HookCallback` | Invoke SDK hook callback |
| `control/cancelRequest` | `CancelRequest` | Cancel pending request |

### 5.3 Gaps: TS Control Protocol → ClientRequest

TS `controlSchemas.ts` defines 21 control request types. Comparison with cocode-rs's 22 ClientRequest variants:

| # | TS Control Request | cocode-rs Status | Action |
|---|-------------------|------------------|--------|
| 1 | `SDKControlInitializeRequest` | Covered by combined `Initialize` + `SessionStart` | DONE — cocode-rs merges TS initialize (hooks, agents, mcp) into SessionStart |
| 2 | `SDKControlInterruptRequest` | `TurnInterrupt` | DONE |
| 3 | `SDKControlPermissionRequest` | `ServerRequest::AskForApproval` (direction reversed) | DONE — cocode-rs design is more correct |
| 4 | `SDKControlSetPermissionModeRequest` | `SetPermissionMode` | DONE |
| 5 | `SDKControlSetModelRequest` | `SetModel` | DONE |
| 6 | `SDKControlSetMaxThinkingTokensRequest` | `SetThinking` | DONE — cocode-rs uses ThinkingConfig which is more complete |
| 7 | `SDKControlMcpStatusRequest` | Missing | **ADD**: `mcp/status` |
| 8 | `SDKControlGetContextUsageRequest` | Missing | **ADD**: `context/usage` |
| 9 | `SDKControlRewindFilesRequest` | `RewindFiles` | DONE |
| 10 | `SDKControlCancelAsyncMessageRequest` | Missing | **EVALUATE**: May not be needed — async message cancellation is TS-specific |
| 11 | `SDKControlSeedReadStateRequest` | Missing | **SKIP**: TS internal optimization (read file cache seeding) |
| 12 | `SDKHookCallbackRequest` | `HookCallbackResponse` | DONE (different direction but semantically equivalent) |
| 13 | `SDKControlMcpMessageRequest` | `McpRouteMessageResponse` | DONE |
| 14 | `SDKControlMcpSetServersRequest` | Missing | **ADD**: `mcp/setServers` |
| 15 | `SDKControlReloadPluginsRequest` | Missing | **ADD**: `plugin/reload` |
| 16 | `SDKControlMcpReconnectRequest` | Missing | **ADD**: `mcp/reconnect` |
| 17 | `SDKControlMcpToggleRequest` | Missing | **ADD**: `mcp/toggle` |
| 18 | `SDKControlStopTaskRequest` | `StopTask` | DONE |
| 19 | `SDKControlApplyFlagSettingsRequest` | Missing | **ADD**: `config/applyFlags` |
| 20 | `SDKControlGetSettingsRequest` | Partially covered by `ConfigRead` | ENHANCE: add sources field |
| 21 | `SDKControlElicitationRequest` | `TuiEvent::ElicitationRequested` (different direction) | **ADD**: `elicitation/resolve` |

### 5.4 Proposed ClientRequest Additions (7 variants)

```rust
// New ClientRequest variants to add
pub enum ClientRequest {
    // ... existing 22 variants ...

    /// Query MCP server connection status.
    #[serde(rename = "mcp/status")]
    McpStatus(McpStatusRequestParams),

    /// Get context window usage breakdown.
    #[serde(rename = "context/usage")]
    ContextUsage(ContextUsageRequestParams),

    /// Hot-reload MCP server configurations.
    #[serde(rename = "mcp/setServers")]
    McpSetServers(McpSetServersRequestParams),

    /// Reconnect a specific MCP server.
    #[serde(rename = "mcp/reconnect")]
    McpReconnect(McpReconnectRequestParams),

    /// Enable/disable a specific MCP server.
    #[serde(rename = "mcp/toggle")]
    McpToggle(McpToggleRequestParams),

    /// Reload all plugins from disk.
    #[serde(rename = "plugin/reload")]
    PluginReload(PluginReloadRequestParams),

    /// Apply feature flag settings at runtime.
    #[serde(rename = "config/applyFlags")]
    ConfigApplyFlags(ConfigApplyFlagsRequestParams),
}
```

### 5.5 Proposed Response Types

```rust
/// Response for `mcp/status`.
pub struct McpStatusResult {
    pub servers: Vec<McpServerStatusInfo>,
}

pub struct McpServerStatusInfo {
    pub name: String,
    pub status: String, // "connected" | "connecting" | "failed" | "disabled"
    pub tool_count: i32,
    pub error: Option<String>,
}

/// Response for `context/usage`.
pub struct ContextUsageResult {
    pub total_tokens: i32,
    pub max_tokens: i32,
    pub percentage: f64,
    pub categories: Vec<ContextUsageCategory>,
    pub model: String,
    pub is_auto_compact_enabled: bool,
    pub auto_compact_threshold: Option<i32>,
    pub message_breakdown: Option<MessageBreakdown>,
}

pub struct ContextUsageCategory {
    pub name: String,
    pub tokens: i32,
}

pub struct MessageBreakdown {
    pub tool_call_tokens: i32,
    pub tool_result_tokens: i32,
    pub attachment_tokens: i32,
    pub assistant_message_tokens: i32,
    pub user_message_tokens: i32,
}

/// Response for `mcp/setServers`.
pub struct McpSetServersResult {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub errors: HashMap<String, String>,
}

/// Response for `plugin/reload`.
pub struct PluginReloadResult {
    pub commands: Vec<CommandInfo>,
    pub agents: Vec<AgentInfo>,
    pub error_count: i32,
}
```

---

## 6. StreamAccumulator Design (KEEP + ENHANCE)

### 6.1 Existing State Machine

```
AgentStreamEvent flow:
  ThinkingDelta* → TextDelta* → ToolUseQueued → ToolUseStarted → ToolUseCompleted
       ↓                ↓              ↓                ↓                ↓
  ItemStarted      ItemStarted    ItemStarted     ItemUpdated     ItemCompleted
  (Reasoning)      (AgentMsg)     (Tool-specific)
  ReasoningDelta   AgentMsgDelta
  ...
  ItemCompleted    ItemCompleted
  (on text start   (on flush)
   or flush)
```

**State**: `text_buffer`, `text_item_id`, `thinking_buffer`, `thinking_item_id`, `active_items: HashMap<call_id, ThreadItem>`

### 6.2 ThreadItem Tool Mapping (KEEP)

```rust
match tool_name {
    Bash        → CommandExecution { command, output, exit_code, status }
    Edit/Write  → FileChange { changes: [{path, kind}], status }
    WebSearch   → WebSearch { query, status }
    mcp__*      → McpToolCall { server, tool, arguments, result?, error?, status }
    Task/Agent  → Subagent { agent_id, agent_type, description, is_background, result?, status }
    _           → ToolCall { tool, input, output?, is_error, status }  // Read, Glob, Grep, etc.
}
```

### 6.3 Enhancement: Hook Events in Stream

TS hook execution produces 3 SDK events (started/progress/response). Currently cocode-rs only has `HookExecuted` (a post-completion notification).

**Proposal**: Promote hook lifecycle events from TuiEvent level to ServerNotification:

```rust
// Add to ServerNotification
HookStarted => "hook/started" (HookStartedParams),
HookProgress => "hook/progress" (HookProgressParams),
HookResponse => "hook/response" (HookResponseParams),

pub struct HookStartedParams {
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String, // HookEventType as_str()
}

pub struct HookProgressParams {
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String,
    pub stdout: String,
    pub stderr: String,
}

pub struct HookResponseParams {
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String,
    pub output: String,
    pub exit_code: Option<i32>,
    pub outcome: HookOutcome, // Success | Error | Cancelled
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutcome {
    Success,
    Error,
    Cancelled,
}
```

---

## 7. Session State Machine

### 7.1 TS SessionState

TS has a `session_state_changed` event with 3 states:
- `idle` — turn completed, waiting for user input
- `running` — turn is executing
- `requires_action` — waiting for user approval

This state is critical for SDK consumers (e.g., VS Code needs to know when it can send a new turn).

### 7.2 Proposed: SessionStateChanged Notification

```rust
// Add to ServerNotification
SessionStateChanged => "session/stateChanged" (SessionStateChangedParams),

pub struct SessionStateChangedParams {
    pub state: SessionState,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// Turn completed, waiting for user input.
    Idle,
    /// Agent is actively processing.
    Running,
    /// Waiting for user action (approval, question, elicitation).
    RequiresAction,
}
```

**Emission points in agent loop**:
- `Idle` → after `TurnCompleted` + all background draining done
- `Running` → on `TurnStarted`
- `RequiresAction` → on `TuiEvent::ApprovalRequired` | `QuestionAsked` | `ElicitationRequested`

---

## 8. Rate Limit Enhancement

### 8.1 TS Rate Limit Detail

TS `SDKRateLimitEventMessage` is richer than cocode-rs `RateLimitParams`:

```typescript
// TS SDKRateLimitInfo
{
  status: 'allowed' | 'allowed_warning' | 'rejected',
  resetsAt?: number,
  rateLimitType?: 'five_hour' | 'seven_day' | 'seven_day_opus' | ...,
  utilization?: number,
  overageStatus?: ...,
  overageDisabledReason?: ...,
  isUsingOverage?: boolean,
  surpassedThreshold?: number,
}
```

### 8.2 Proposed: Enhance RateLimitParams

```rust
pub struct RateLimitParams {
    // existing
    pub remaining: Option<i64>,
    pub reset_at: Option<i64>,
    pub limit: Option<i64>,
    pub provider: Option<String>,
    // new fields from TS
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<RateLimitStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization: Option<f64>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitStatus {
    Allowed,
    AllowedWarning,
    Rejected,
}
```

---

## 9. Task Events (Aligned with TS sdkEventQueue)

### 9.1 TS Pattern: sdkEventQueue

TS uses a dedicated `sdkEventQueue` (utils/sdkEventQueue.ts) for background task
events. Events are **queued** via `enqueueSdkEvent()` during task execution and
**drained** via `drainSdkEvents()` before each `result` message is emitted, plus
mid-turn for real-time streaming.

Queue characteristics:
- `MAX_QUEUE_SIZE = 1000` (FIFO with shift on overflow)
- Only active in non-interactive/headless mode (`getIsNonInteractiveSession()`)
- Each drained event is enriched with `uuid` and `session_id` at drain time
- Four event types: `task_started`, `task_progress`, `task_notification`, `session_state_changed`

### 9.2 TS Task Event Fields (from coreSchemas.ts:1694-1767)

```typescript
// TS SDKTaskStartedMessage
{
  type: 'system', subtype: 'task_started',
  task_id, tool_use_id?, description,   // description REQUIRED
  task_type?, workflow_name?, prompt?,
  uuid, session_id,
}

// TS SDKTaskProgressMessage
{
  type: 'system', subtype: 'task_progress',
  task_id, tool_use_id?, description,   // description REQUIRED
  usage: { total_tokens, tool_uses, duration_ms },  // usage REQUIRED
  last_tool_name?, summary?, workflow_progress?,
  uuid, session_id,
}

// TS SDKTaskNotificationMessage (maps to coco-rs TaskCompletedParams)
{
  type: 'system', subtype: 'task_notification',
  task_id, tool_use_id?,
  status: 'completed' | 'failed' | 'stopped',
  output_file, summary,                 // both REQUIRED
  usage?,
  uuid, session_id,
}
```

### 9.3 coco-rs Implementation (Phase 0 — structural, not wired)

`TaskStartedParams`, `TaskProgressParams`, `TaskCompletedParams` are defined in
`coco-types` and field-aligned with TS exactly. `TaskCompletedParams` uses the
TS `SDKTaskNotificationMessage` shape (status/output_file/summary) — coco-rs
uses the wire method `task/completed` but the fields match TS's task_notification.

**Not yet wired** (Phase 1+):
- An `sdkEventQueue` equivalent in coco-query that accumulates task events
- Emit points in `coco-tasks` / `coco-subagent` crates
- Drain logic before each session result

Structural types are in place so the queue/drain infrastructure can be added
without re-shaping the protocol.

```rust
// Defined in coco-types/src/event.rs — matching TS exactly
pub struct TaskStartedParams { ... }
pub struct TaskProgressParams { ... }
pub struct TaskCompletedParams { ... }  // TS: task_notification
pub struct TaskUsage { total_tokens, tool_uses, duration_ms }
pub enum TaskCompletionStatus { Completed, Failed, Stopped }
```

---

## 10. SessionResult Enhancement

### 10.1 TS Result Message

TS `SDKResultMessage` is richer than cocode-rs `SessionResultParams`:

```typescript
// TS SDKResultSuccessMessage
{
  duration_ms, duration_api_ms, is_error, num_turns, result,
  num_api_calls,
  modelUsage: Record<string, ModelUsage>,  // per-model breakdown
  permission_denials: SDKPermissionDenial[],
  structured_output?,
  fast_mode_state?,
}
```

### 10.2 Proposed: Enhance SessionResultParams

```rust
pub struct SessionResultParams {
    pub session_id: String,
    pub total_turns: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_cost_cents: Option<i64>,
    pub duration_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_api_ms: Option<i64>,
    pub usage: Usage,
    pub stop_reason: SessionEndedReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<Value>,
    // new
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_api_calls: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_usage: Option<HashMap<String, Usage>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_denials: Vec<PermissionDenialInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_mode_state: Option<FastModeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

pub struct PermissionDenialInfo {
    pub tool_name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FastModeState {
    Off,
    Cooldown,
    On,
}
```

---

## 11. Schema Generation Pipeline

### 11.1 cocode-rs Approach (KEEP)

```
Rust types + #[derive(JsonSchema)]
  → schemars → JSON Schema
  → datamodel-code-generator → Python SDK types
  → json-schema-to-typescript → TypeScript SDK types (future)
```

### 11.2 TS Approach (Reference)

```
Zod schemas (coreSchemas.ts)
  → generate-sdk-types.ts → TypeScript .d.ts
  → Runtime validation (z.parse)
```

### 11.3 Comparison

| Aspect | cocode-rs | TS |
|--------|-----------|-----|
| Source of truth | Rust types | Zod schemas |
| Schema format | JSON Schema (standard) | Zod (proprietary) |
| Multi-language | Yes (any JSON Schema consumer) | No (TypeScript only) |
| Runtime validation | serde deserialization | z.parse() |
| Code generation | Automated via `schemars` derive | Custom script |

The cocode-rs approach is more standardized and extensible.

---

## 12. Consumer Routing Matrix

### 12.1 Event → Consumer Delivery

| Event Layer | TUI | SDK/CLI (NDJSON) | App-Server (WebSocket) | IDE Extension |
|-------------|-----|------------------|----------------------|---------------|
| `ServerNotification` (56 base + 9 gaps = 65 target; 60 implemented) | via handler | direct emit | broadcast to clients | broadcast |
| `AgentStreamEvent` (7) | direct display + accumulator | accumulator → ServerNotification | accumulator → ServerNotification | accumulator |
| `TuiEvent` (20) | via overlay/toast handler | **dropped** | **dropped** | partial (approval only) |

### 12.2 Channel Architecture

```rust
// Core → TUI (full set)
let (core_tx, core_rx) = mpsc::channel::<CoreEvent>(32);

// Core → App-Server (protocol + stream)
let (server_tx, server_rx) = mpsc::channel::<CoreEvent>(256);
// App-Server internally filters: Tui events dropped

// App-Server → Client (outbound)
let (client_tx, client_rx) = mpsc::channel::<OutboundMessage>(64);
// OutboundMessage = JsonRpcNotification | JsonRpcResponse

// Client → App-Server (inbound)
let (inbound_tx, inbound_rx) = mpsc::channel::<TransportEvent>(64);
// TransportEvent = ConnectionOpened | ConnectionClosed | IncomingMessage
```

### 12.3 SDK/CLI Mode Event Flow

```
CoreEvent::Protocol(notif) → NDJSON: {"method": "turn/started", "params": {...}}
CoreEvent::Stream(agent_evt) → StreamAccumulator.process(agent_evt)
                             → Vec<ServerNotification>
                        → NDJSON for each
CoreEvent::Tui(_) → dropped (log only)
```

---

## 13. Implementation Priority

### Phase 0: Foundation — CoreEvent Infrastructure

| Item | Change | Effort |
|------|--------|--------|
| Define `CoreEvent`, `AgentStreamEvent`, `ThreadItem`, `ItemStatus` in coco-types | New types per Section 1.4-1.6 | M |
| Move `ServerNotification` from coco-tui to coco-types | Expand from 17 → 43 variants (existing cocode-rs base) | L |
| Replace `QueryEvent` with `CoreEvent` emission in QueryEngine | Delete QueryEvent enum, emit CoreEvent directly | M |
| Delete `map_query_event()` in tui_runner.rs | TUI consumes CoreEvent directly | S |
| Implement `StreamAccumulator` in coco-query | AgentStreamEvent → ServerNotification (ItemStarted/Updated/Completed) | L |
| TUI: consume `CoreEvent` instead of `ServerNotification` | Update `handle_server_notification()` to 3-way dispatch | M |

> **TS reference**: This phase aligns with TS's pattern where `query()` yields core types directly. The key difference: TS yields raw `Message` objects, coco-rs emits `CoreEvent` with 3-layer dispatch. Both avoid intermediate summary types (like the current `QueryEvent`).

### Phase 1: P0 — SDK Consumer Parity

| Item | Change | Effort | Status |
|------|--------|--------|--------|
| `SessionStarted` emission | New `SessionBootstrap` struct + emit at session entry | S | ✅ done |
| `SessionStateChanged` Running/Idle | Emit at session entry/exit via split `run_session_loop` | S | ✅ done |
| `SessionStateChanged` RequiresAction | Emit when permission prompt fires | S | ⏭️ deferred to Phase 2 (needs SDK control protocol) |
| `HookStarted/Progress/Response` wiring | Extend `execute_pre/post_tool_use` to accept `event_tx`; spawn `forward_hook_events` forwarder in engine | M | ✅ done |
| `SessionResult` emission | `build_session_result_params` from `QueryResult` + `CostTracker.per_model` | S | ✅ done |
| `permission_denials` accumulation | Track denials across session, populate in SessionResult | S | ⏭️ deferred to Phase 2 |
| `Task` lifecycle emission | Needs `TaskHandle` trait event sink + subagent wiring | L | ⏭️ deferred to Phase 2 |

### Phase 2: P1 — Control Protocol Completeness

| Item | Change | Effort |
|------|--------|--------|
| `mcp/status` request | Add ClientRequest variant + handler | S |
| `context/usage` request + response | Add variant + ContextUsageResult | M |
| `mcp/setServers` request | Add variant + hot-reload logic | M |
| `mcp/reconnect` request | Add variant + reconnect logic | S |
| `mcp/toggle` request | Add variant + enable/disable logic | S |
| `plugin/reload` request | Add variant + reload logic | M |
| `config/applyFlags` request | Add variant + flag application | S |

### Phase 3: P2 — Nice-to-Have

| Item | Change | Effort |
|------|--------|--------|
| `LocalCommandOutput` notification | Add variant (content field) | S — ✅ done |
| `FilesPersisted` notification | Add variant (files, failed, processed_at) | S — ✅ done |
| `ElicitationComplete` notification | Add variant (mcp_server_name, elicitation_id) | S — ✅ done |
| `ToolUseSummary` notification | Add variant (summary, preceding_tool_use_ids) | S — ✅ done |
| `ToolProgress` notification | Add variant (tool_use_id, tool_name, elapsed_time, task_id) | S — ✅ done |
| Enhance `RateLimitParams` | Add status/type/utilization fields per TS SDKRateLimitInfoSchema | S — ✅ done |

---

## 14. Summary: cocode-rs vs TS Event Architecture

```
                    cocode-rs (base)              TS (supplement)
                    -----------------             ---------------
Architecture:       3-layer CoreEvent ✓           Flat SDKMessage
                    (superior design)             (need reverse parse)

Protocol events:    56 ServerNotification         24 SDKMessage
                    + 9 proposed additions        (already exceeded)
                    = 65 target, 60 implemented

Stream events:      7 AgentStreamEvent             Raw SSE events (content_block_delta, etc.)
                    + StreamAccumulator ✓          + normalizeMessage() in queryHelpers.ts
                    (explicit state machine)      (sync conversion at SDK boundary)

TUI events:         20 TuiEvent                   Mixed in SDKMessage
                    (clean separation) ✓          (needs filtering)

Bidirectional:      22 ClientRequest              ~21 control requests
                    + 7 proposed additions        (partially structured)
                    = 29 total
                    5 ServerRequest               ~3 control responses
                    (JSON-RPC structured) ✓       (custom format)

Schema:             schemars → JSON Schema ✓      Zod → TS-only
                    (multi-language codegen)       (single language)

Transport:          channel / NDJSON / WS ✓       NDJSON only
```

**Bottom line**: The cocode-rs event system is architecturally superior to TS. What needs to be done:
1. Phase 0: CoreEvent infrastructure (define types, replace QueryEvent, implement StreamAccumulator)
2. Phase 1: 4 P0 notification enhancements (session state, hook lifecycle, task details, result details)
3. Phase 2: 7 P1 control request additions (MCP management, context usage, plugin reload, flag settings)
4. Phase 3: 6 P2 minor notification additions
