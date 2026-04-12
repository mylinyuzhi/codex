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
│  emit(CoreEvent::Stream(StreamEvent))           → needs accum.   │
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

### 1.2 TS Flat Model (Reference)

```
query() yields Message (internal)
  → normalizeMessage() conversion
  → SDKMessage (24 variants, discriminated by type+subtype)
  → all consumers consume uniformly
  → UI side uses convertSDKMessage() to reverse-parse
```

### 1.3 Decision: Keep the cocode-rs Three-Layer Architecture

The cocode-rs design is superior; there is no need to switch to the TS flat model. What needs to be done:
1. Supplement the `ServerNotification` layer with SDK-visible events from TS
2. Supplement the `TuiEvent` layer with UI-only events from TS
3. Supplement the `ClientRequest`/`ServerRequest` layer with the TS control protocol
4. Ensure `StreamAccumulator` covers all streaming conversion scenarios

---

## 2. Event Catalog: ServerNotification (Protocol Layer)

### 2.1 Existing (43 variants) — Already Implemented in cocode-rs

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

| # | Proposed Variant | Wire Method | TS Source | Params | Priority |
|---|-----------------|-------------|-----------|--------|----------|
| 1 | `AuthStatus` | `auth/status` | `SDKAuthStatusMessage` | is_authenticating, output: Vec<String>, error? | P2 |
| 2 | `HookStarted` | `hook/started` | `SDKHookStartedMessage` | hook_id, hook_name, hook_event | P1 |
| 3 | `HookProgress` | `hook/progress` | `SDKHookProgressMessage` | hook_id, hook_name, hook_event, stdout, stderr, output | P1 |
| 4 | `HookResponse` | `hook/response` | `SDKHookResponseMessage` | hook_id, hook_name, hook_event, output, stdout, stderr, exit_code?, outcome | P1 |
| 5 | `LocalCommandOutput` | `localCommand/output` | `SDKLocalCommandOutputMessage` | content | P2 |
| 6 | `SessionStateChanged` | `session/stateChanged` | `SDKSessionStateChangedMessage` | state: idle/running/requires_action | P1 |
| 7 | `FilesPersisted` | `files/persisted` | `SDKFilesPersistedEvent` | files: Vec<{filename, file_id}>, failed: Vec<{filename, error}>, processed_at | P2 |
| 8 | `ElicitationComplete` | `elicitation/complete` | `SDKElicitationCompleteMessage` | mcp_server_name, elicitation_id | P2 |
| 9 | `ToolUseSummary` | `tool/useSummary` | `SDKToolUseSummaryMessage` | summary, preceding_tool_use_ids: Vec | P2 |

**Analysis**:
- **P1 (affects core logic)**: HookStarted/Progress/Response (3 events) + SessionStateChanged — SDK consumers depend on these events to track hook execution and session state
- **P2 (nice-to-have)**: AuthStatus, LocalCommandOutput, FilesPersisted, ElicitationComplete, ToolUseSummary

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

## 3. Event Catalog: StreamEvent (Accumulation Layer)

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

TS `StreamEvent` is an internal type (not exposed to the SDK), converted to SDKMessage via `normalizeMessage()`. The cocode-rs `StreamAccumulator` is an equivalent explicit state machine, and is clearer.

**TS `stream_event` types** (internal to query.ts):
- `content_block_delta` (text, thinking, tool_use input)
- `content_block_start` / `content_block_stop`
- `message_delta` (stop_reason, usage)
- `message_start` / `message_stop`

The 7 cocode-rs StreamEvent variants are already a high-level abstraction of the TS raw SSE events and do not need alignment.

### 3.3 Gap: ToolCallDelta in Stream Layer

TS passes partial tool call JSON (streaming tool input) via `content_block_delta`. cocode-rs currently places `ToolCallDelta` in `TuiEvent`.

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
StreamEvent flow:
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

## 9. TaskStarted/TaskProgress Enhancement

### 9.1 TS Task Events Are Richer

```typescript
// TS SDKTaskStartedMessage
{
  task_id, tool_use_id?, description,
  task_type?,        // "local_workflow", "agent", etc.
  workflow_name?,    // meta.name from workflow script
  prompt?,           // agent prompt
}

// TS SDKTaskProgressMessage
{
  task_id, tool_use_id?, description,
  usage: { total_tokens, tool_uses, duration_ms },
  last_tool_name?, summary?,
  workflow_progress?: SdkWorkflowProgress[],
}
```

### 9.2 Proposed: Enhance Task Params

```rust
pub struct TaskStartedParams {
    pub task_id: String,
    pub task_type: String,
    // new
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

pub struct TaskProgressParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    // new
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TaskUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

pub struct TaskUsage {
    pub total_tokens: i64,
    pub tool_uses: i32,
    pub duration_ms: i64,
}

pub struct TaskCompletedParams {
    pub task_id: String,
    pub result: String,
    #[serde(default)]
    pub is_error: bool,
    // new
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TaskUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}
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
| `ServerNotification` (43+9 = 52) | via handler | direct emit | broadcast to clients | broadcast |
| `StreamEvent` (7) | direct display + accumulator | accumulator → ServerNotification | accumulator → ServerNotification | accumulator |
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
CoreEvent::Stream(evt) → StreamAccumulator.process(evt)
                        → Vec<ServerNotification>
                        → NDJSON for each
CoreEvent::Tui(_) → dropped (log only)
```

---

## 13. Implementation Priority

### Phase 1: P0 — SDK Consumer Parity

| Item | Change | Effort |
|------|--------|--------|
| `SessionStateChanged` notification | Add variant + emission points | S |
| `HookStarted/Progress/Response` notifications | Replace single `HookExecuted` with 3-phase lifecycle | M |
| Enhance `TaskStarted/Progress/Completed` params | Add tool_use_id, usage, summary fields | S |
| Enhance `SessionResultParams` | Add model_usage, permission_denials, errors | S |

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
| `AuthStatus` notification | Add variant | S |
| `LocalCommandOutput` notification | Add variant | S |
| `FilesPersisted` notification | Add variant | S |
| `ElicitationComplete` notification | Add variant | S |
| `ToolUseSummary` notification | Add variant | S |
| Enhance `RateLimitParams` | Add status/type/utilization fields | S |

---

## 14. Summary: cocode-rs vs TS Event Architecture

```
                    cocode-rs (base)              TS (supplement)
                    -----------------             ---------------
Architecture:       3-layer CoreEvent ✓           Flat SDKMessage
                    (superior design)             (need reverse parse)

Protocol events:    43 ServerNotification         24 SDKMessage
                    + 9 proposed additions        (already exceeded)
                    = 52 total

Stream events:      7 StreamEvent                 Raw SSE events
                    + StreamAccumulator ✓          + normalizeMessage()
                    (explicit state machine)      (ad-hoc generator)

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

**Bottom line**: The cocode-rs event system is architecturally superior to TS. What needs to be supplemented:
1. 4 P0 notification enhancements (session state, hook lifecycle, task details, result details)
2. 7 P1 control request additions (MCP management, context usage, plugin reload, flag settings)
3. 6 P2 minor notification additions
