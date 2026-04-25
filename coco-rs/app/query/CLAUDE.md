# coco-query

Multi-turn agent loop driver. Orchestrates the full turn cycle: prompt build,
LLM call, tool execution, compaction, command-queue drain, budget/continue
decisions. Emits `coco_types::CoreEvent` directly (no intermediate event enum).

## TS Source

- `QueryEngine.ts` — multi-turn loop
- `query.ts` — single-turn execution
- `query/{config,deps,stopHooks,tokenBudget}.ts` — gates + budget + stop-hook handling
- `utils/messageQueueManager.ts` — mid-turn command queue
- `utils/QueryGuard.ts` — 3-state query synchronization primitive
- `utils/queueProcessor.ts` — queue draining strategy
- `utils/attachments.ts` — attachment injection between turns
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
| `CommandQueue`, `QueuedCommand`, `QueuePriority` | `Now`/`Next`/`Later`; FIFO within priority; snapshot + signal notify |
| `Inbox`, `InboxMessage` | Teammate async messages; 2-phase delivery (submit if idle, queue mid-turn otherwise) |
| `QueryGuard`, `QueryGuardStatus` | 3-state FSM (`Idle`/`Dispatching`/`Running`) + generation counter for stale-finally detection |
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
9.  Drain CommandQueue + Inbox + attachment injection → user messages
10. Goto 1 if tools remain; else emit TurnCompleted
```

## Emitted CoreEvent Variants

Protocol: `TurnStarted`, `TurnCompleted`, `TurnFailed`, `CompactionStarted`,
`ContextCompacted`, `Error` (budget nudge), `QueueStateChanged`.
Stream: `TextDelta`, `ThinkingDelta`, `ToolUseQueued`, `ToolUseStarted`,
`ToolUseCompleted`.
(See `docs/coco-rs/event-system-design.md` for full catalog.)

## Steering (Mid-Turn Injection)

Users can type while the LLM is working. Commands enqueue with priority; at the
gap between tool call N and the next API request, `get_attachment_messages` pulls
the queue snapshot + inbox + memory prefetches and injects them as user
attachments. `QueryGuard` prevents the queue processor from re-entering an
active turn. Dedup key: `from|timestamp|text[..100]`.
