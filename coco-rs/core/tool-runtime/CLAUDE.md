# coco-tool-runtime

Tool trait, streaming executor, tool registry, callback handles. Defines the interface; `coco-tools` provides implementations.

## TS Source
- `Tool.ts` — Tool interface + ToolUseContext (Rust: `traits.rs`, `context.rs`)
- `services/tools/StreamingToolExecutor.ts` — concurrent safe / queued unsafe executor
- `services/tools/toolOrchestration.ts` — orchestration
- `services/tools/toolExecution.ts` — per-tool execution pipeline
- `services/tools/toolHooks.ts` — pre/post tool hook integration
- `tools.ts` — registry + feature-gated tool loading

## Key Types

- **Trait + context**: `Tool`, `ToolUseContext`, `DescriptionOptions`, `InterruptBehavior`, `ProgressSender`/`ProgressReceiver`/`ToolProgress`, `PromptOptions`, `SearchReadInfo`, `McpToolInfo`
- **Executor**: `StreamingToolExecutor`, `ToolBatch`, `BatchResult`, `PendingToolCall`, `ToolCallResult`, `ToolStatus`, `StreamingToolUpdate`
- **Registry**: `ToolRegistry`
- **Errors**: `ToolError`, `SyntheticToolError`, `ToolUseEvent`, `classify_tool_error`, `format_tool_error`
- **Validation**: `ValidationResult`
- **Callback handles** (decouple tool → subsystem circular deps; every handle has a `NoOp*` impl for tests):
  - `AgentHandle`/`AgentHandleRef` + `AgentSpawnRequest`/`AgentSpawnResponse`/`AgentSpawnStatus` — subagent spawning
  - `AgentQueryEngine`/`AgentQueryEngineRef` + `AgentQueryConfig`/`AgentQueryResult` — side-agent queries
  - `HookHandle`/`HookHandleRef` + `HookPermission`/`PreToolUseOutcome`/`PostToolUseOutcome`
  - `McpHandle`/`McpHandleRef` + `McpToolAnnotations`/`McpToolSchema`
  - `TaskHandle`/`TaskHandleRef` + `BackgroundShellRequest`/`BackgroundTaskInfo`/`BackgroundTaskStatus`/`StallInfo`/`TaskOutputDelta` — running background tasks (shell/agent)
  - `TaskListHandle`/`TaskListHandleRef` — persistent V2 plan-item store (`TaskCreate`/`Update`/`Get`/`List`/`Stop`/`Output`). DTOs live in `coco-types` (`TaskRecord`, `TaskRecordUpdate`, `TaskListStatus`, `TaskClaimOutcome`, `ExpandedView`); `coco-tool-runtime` re-exports them. `InMemoryTaskListHandle` for tests; `NoOpTaskListHandle` for sessions without a store.
  - `TodoListHandle`/`TodoListHandleRef` + `TodoRecord` (re-export) — per-agent V1 TodoWrite checklist. `InMemoryTodoListHandle` is the default.
  - `check_verification_nudge(&[&str])` — shared pure helper used by both V1 `TodoWrite` and V2 `TaskUpdate` (TS parity: `/verif/i` gate, ≥3 items).
  - `MailboxHandle`/`MailboxHandleRef` + `InboxMessage`/`MailboxEnvelope`
  - `ScheduleStore`/`ScheduleStoreRef` — cron store
  - `SideQuery`/`SideQueryHandle` + `SideQueryRequest`/`SideQueryResponse` + `side_query_to_text_callback`
  - `ToolPermissionBridge`/`ToolPermissionBridgeRef` + `ToolPermissionRequest`/`ToolPermissionDecision`/`ToolPermissionResolution`
  - `CanUseToolHandle`/`CanUseToolHandleRef` + `CanUseToolDecision` (`Allow{updated_input}` / `Deny{message}` / `Ask`) + `DecisionReason` + `CanUseToolCallContext` + `NoOpCanUseToolHandle` + `deny_all_handle(reason)` — per-fork tool-execution gate dispatched at `execution::execute_tool_call` step 3.5 BEFORE the tool's built-in `check_permissions`. TS: `Tool.ts::CanUseToolFn`. The `Allow{updated_input}` variant is the path-rewrite hook speculation overlay needs.
  - `PlanApprovalMessage`/`PlanApprovalRequest`/`PlanApprovalResponse`
- **Stall detection**: `STALL_CHECK_INTERVAL_MS`, `STALL_TAIL_BYTES`, `STALL_THRESHOLD_MS`, `format_stall_notification`, `format_task_notification`, `matches_interactive_prompt`

## Architecture

- **Safe tools** (read-only, idempotent) execute concurrently; **unsafe tools** queue and execute after streaming stop. `StreamingToolExecutor` orchestrates this.
- All cross-subsystem interaction (tasks, agents, hooks, MCP, mailbox) goes through callback handle traits — `coco-tool-runtime` does NOT depend on `coco-tools`, `coco-tasks`, `coco-commands`, etc. Implementations are injected via `ToolUseContext` at runtime.
- `ToolUseContext` is the typed payload carried across tool invocations (see main CLAUDE.md "Typed Structs over JSON Values" for the `ToolAppState` migration story).

## Tool Result Budget (Level 1 + state types for Level 2)

Owner of the `tool_result_storage` module (planned: `src/tool_result_storage/`).
TS source: `utils/toolResultStorage.ts`, `constants/toolLimits.ts`,
`utils/mcpOutputStorage.ts`. Plan: [`docs/coco-rs/tool-result-budget-plan.md`](../../../docs/coco-rs/tool-result-budget-plan.md).

- **Level 1** — per-tool persistence helpers: `persist_to_disk` and
  `render_persisted_reference` live in `tool_result_storage.rs`; `coco-query`
  invokes them after `Tool::execute()` for singleton text results when
  `Tool::max_result_size_chars()` opts in. Current gaps vs TS: overwrite rather
  than `create_new`, no empty-content guard here, and Bash still has a
  tool-local `temp_dir()` persistence path for shell stdout.
- **Level 2** — aggregate budget state and decision logic:
  `ContentReplacementState` + `apply_tool_result_budget`. `coco-query` owns the
  message projection/wiring. Current gap vs TS: Rust currently replaces selected
  IDs with `[Old tool result content cleared]`; TS persists selected fresh
  candidates and replays the exact `<persisted-output>` preview string from
  replacement state/transcript records.

`Tool::max_result_size_chars()` uses `i64::MAX` as the Rust sentinel for TS
`Infinity` opt-out.

## Deferred refactors (no TS-parity impact, pure code-quality)

Tracked here for future contributors — none of these changes behavior;
they're all structural cleanups identified in the May 2026 audit.

### Split `AgentSpawnRequest` into 4 sub-structs

`AgentSpawnRequest` carries 27 fields covering four distinct concerns:
model-visible input (prompt / description / subagent_type / model /
run_in_background / isolation / cwd / effort / name / team_name / mode /
auto_background_ms), permission policy (mcp_servers / disallowed_tools /
max_turns / can_use_tool / require_can_use_tool / features /
tool_overrides / parent_tool_filter), routing (tool_use_id /
invoking_agent_id / session_id / fork_label), and spawn-mode discriminant
(spawn_mode / definition / constraints / fork_context_messages /
skip_transcript / use_exact_tools / initial_prompt / model_role /
enable_summarization / is_non_interactive).

Mechanical split:

```rust
pub struct AgentSpawnRequest {
    pub input: AgentSpawnInput,
    pub policy: AgentSpawnPolicy,
    pub routing: AgentSpawnRouting,
    pub spawn_mode: SpawnMode,
    pub definition: Option<Arc<AgentDefinition>>,
    pub constraints: Option<AgentSpawnConstraints>,
    pub fork_context_messages: Vec<Arc<Message>>,
}
```

Touches 5 callers (`AgentTool`, `memory::{extract, dream, session}`,
`coordinator::resume`). Pure refactor, no semantics change.

### `TaskExtras` enum on `TaskStateBase`

`TaskStateBase` currently carries 5 LocalAgent-specific Option fields
(`progress` / `retrieved` / `retain` / `evict_after` / `is_backgrounded`)
that default to None / false for shell / dream / teammate tasks. Pollutes
the type with always-None fields and the per-task-type match logic ends
up scattered across `running.rs` / `reminder_source.rs` / TUI panels.

```rust
pub enum TaskExtras {
    LocalAgent(LocalAgentExtras),
    LocalShell(LocalShellExtras),
    Dream,
    None,
}

pub struct TaskStateBase {
    // core fields only ...
    pub extras: TaskExtras,
}
```

Eliminates sparse LocalAgent sidecar shims entirely; per-task-type accessors
return concrete types via match. Touches 20+ files (every consumer of
`progress` / `retrieved` / `retain` / `evict_after` / `is_backgrounded`).

### Schemars-derived `ToolInputSchema`

`ToolInputSchema` is currently a hand-built `HashMap<String, Value>` +
`Vec<String>` `required` list. Every tool implements `input_schema()`
with 40-200 lines of `serde_json::json!` macro construction. Workspace
upgraded `schemars` to 1.2 (commit `ba50b1364`) — the infrastructure is
in place to switch each tool to a `#[derive(JsonSchema, Deserialize)]`
input struct.

Migration pattern (per tool):

```rust
#[derive(Deserialize, JsonSchema)]
pub struct AgentToolInput {
    /// The task for the agent to perform
    pub prompt: String,
    /// A short (3-5 word) description of the task
    pub description: String,
    pub subagent_type: Option<String>,
    // ...
}

impl Tool for AgentTool {
    fn input_schema(&self) -> ToolInputSchema {
        schema_from::<AgentToolInput>()
    }
}
```

Required fields fall out of `Option<T>` automatically. Hand-built
schemas become validators-for-free. Touches all 43 built-in tools but
each conversion is mechanical.
