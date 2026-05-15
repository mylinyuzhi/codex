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
