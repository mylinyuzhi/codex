# coco-query

Multi-turn agent loop driver. Orchestrates the full turn cycle: prompt build,
LLM call, tool execution, compaction, command-queue drain, budget/continue
decisions. Emits `coco_types::CoreEvent` directly (no intermediate event enum).

## TS Source

- `QueryEngine.ts` — multi-turn loop
- `query.ts` — single-turn execution
- `query/{config,deps,stopHooks,tokenBudget}.ts` — gates + budget + stop-hook handling
- `utils/messageQueueManager.ts` — mid-turn command queue (module-level singleton in TS)
- `utils/queueProcessor.ts` — queue draining strategy
- `utils/attachments.ts` — `getQueuedCommandAttachments` (origin framing + system-reminder wrap)
- `utils/processUserInput/{processBashCommand.tsx,processSlashCommand.tsx,processTextPrompt.ts,processUserInput.ts}`
- `tasks/LocalMainSessionTask.ts` — task adapter for main loop

## Key Types

| Type | Purpose |
|------|---------|
| `QueryEngine` | Orchestrator: owns tool/command registries, `ApiClient`, state |
| `QueryEngineConfig` | max_turns, max_tokens, permission_mode, context_window, streaming_tool_execution, bypass_permissions_available, fallback_model, plan_mode_settings |
| `QueryResult`, `ContinueReason` | Loop control: `NextTurn`, `ReactiveCompactRetry`, `MaxOutputTokensEscalate`, `MaxOutputTokensRecovery`, `StopHookBlocking`, `TokenBudgetContinuation`, `CollapseDrainRetry` |
| `SessionBootstrap` | Initial system prompt, messages, cost tracker |
| `BudgetTracker`, `BudgetDecision` | Token budget; 3-continuation cap, 90% threshold, diminishing-returns stop |
| `CommandQueue`, `QueuedCommand`, `QueuePriority`, `QueueOrigin` | `Now`/`Next`/`Later`; FIFO within priority; per-item `Uuid` for id-based removal; `Human`/`Coordinator`/`TaskNotification`/`Channel` origin drives framing prose |
| `StreamAccumulator` | `AgentStreamEvent` → `ServerNotification::ItemStarted/Updated/Completed` with `ThreadItem` tool mapping |
| `agent_adapter::*` | Bridges `QueryEngine` to tool invocations and subagent spawn callbacks |
| `plan_mode_reminder::*` | Plan-mode steady-state reminder cadence (Full/Sparse/Reentry) |
| `single_turn::*` | One-shot turn execution (no loop) |
| `emit::*` | `CoreEvent` emission helpers |

## Turn Lifecycle

```
1.  Build system prompt (context)                  [coco-context]
2.  Normalize messages for API                     [coco-messages]
3.  ApiClient.query_streaming(QueryParams)         [coco-inference]
4.  Parse response; extract tool calls             [engine.rs]
5.  StreamingToolExecutor: safe concurrent / unsafe queued  [coco-tool-runtime]
6.  HookRegistry PreToolUse / PostToolUse          [coco-hooks]
7.  Tool results → MessageHistory                  [coco-messages]
8.  Check ContinueReason
       - NextTurn / TokenBudgetContinuation → loop
       - ReactiveCompactRetry → compact then retry [coco-compact]
       - MaxOutputTokensEscalate (→64k) / Recovery / StopHookBlocking / CollapseDrainRetry
9.  Drain CommandQueue → attachment messages (User w/ system-reminder wrap)
10. Goto 1 if tools remain; else emit TurnCompleted
```

## Emitted CoreEvent Variants

Protocol: `TurnStarted`, `TurnCompleted`, `TurnFailed`, `CompactionStarted`,
`ContextCompacted`, `Error` (budget nudge), `QueueStateChanged`,
`CommandQueued`, `CommandDequeued`.
Stream: `TextDelta`, `ThinkingDelta`, `ToolUseQueued`, `ToolUseStarted`,
`ToolUseCompleted`.
(See `docs/coco-rs/event-system-design.md` for full catalog.)

## Steering (Mid-Turn Injection)

Users can type while the LLM is working. The TS pattern uses a module-level
singleton; in coco-rs the queue is **`SessionRuntime`-scoped** (`runtime.command_queue`)
because `QueryEngine` is rebuilt per turn — `SessionRuntime::wire_engine` calls
`engine.with_command_queue(self.command_queue.clone())` so every turn observes
the same `Arc`-shared queue.

**Enqueue path.** While streaming, the TUI dispatch (`tui_runner`) routes typed
input through `UserCommand::QueueCommand`, which constructs a `QueuedCommand`
(default priority `Next`, origin `Human`), pushes it onto the runtime queue,
and emits `ServerNotification::CommandQueued { id, preview }`. Each item has
a `Uuid` for id-based removal — there is no `from|timestamp|text[..100]`
dedup string (TS-style).

**Drain path.** At turn boundaries (after a turn finishes, before the next API
request), `engine_finalize_turn` calls `drain_command_queue_into_history`. Each
queued item becomes one `Message::Attachment(AttachmentKind::QueuedCommand)`
carrying a User-role LLM message. The body is **double-wrapped**, mirroring
TS `getQueuedCommandAttachments`:

1. `wrap_command_text(prompt, origin)` — origin-specific framing prose
   (e.g. "The user sent the following message while you were working:").
2. `wrap_in_system_reminder(...)` — outer `<system-reminder>` XML tags.

Attachments are API-visible (`AttachmentMessage::api`) so they render in the
transcript and reach the model on the next turn. The `messages::normalize`
pass `smoosh_system_reminder_into_tool_result` then folds the wrapped User
message into the preceding Tool message when present, preserving Anthropic's
strict tool_use/tool_result adjacency.

**No mid-turn `Now` drain.** TS supports interleaving `Now`-priority items
mid-turn; coco-rs intentionally does not — it would break tool_use/tool_result
pairing on non-streaming providers. All priorities are honored at the
turn-boundary drain in FIFO-within-priority order.

**Clear semantics.** `SessionRuntime::clear_conversation` wipes the queue
*before* the `is_history_only` early return, so every clear scope (history /
full / partial) drops in-flight queued commands.

**E2E coverage.** `app/query/tests/steering.rs` runs a real `QueryEngine`
against a mock model: a producer task enqueues during turn 1, the test asserts
the wrapped attachment lands in history, the second turn's prompt contains the
steering marker, lifecycle events fire (`CommandQueued` → `CommandDequeued`),
and the final response references the marker (proving the model acted on it).
