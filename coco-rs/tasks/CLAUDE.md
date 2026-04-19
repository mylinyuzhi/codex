# coco-tasks

Background task system: LocalBash / LocalAgent / LocalWorkflow / RemoteAgent / InProcessTeammate / MonitorMcp / Dream. Lifecycle tracking, output persistence, dependency graph, optional event-stream emission for SDK clients, TodoV2 checklists.

## TS Source
- `Task.ts` — canonical TaskStateBase + manager logic
- `tasks/types.ts` — task variant types
- `tasks/LocalShellTask/` — background bash
- `tasks/LocalAgentTask/` — foreground/background sub-agents
- `tasks/RemoteAgentTask/` — CCR remote agents
- `tasks/DreamTask/` — auto-dream consolidation
- `tasks/InProcessTeammateTask/` — v2 team-mate tasks
- `tasks/LocalMainSessionTask.ts`, `stopTask.ts`, `pillLabel.ts`
- `utils/task/TaskOutput.ts`, `diskOutput.ts`, `framework.ts`, `outputFormatting.ts`, `sdkProgress.ts`
- `utils/todo/types.ts` — TodoV2

Paths relative to `/lyz/codespace/3rd/claude-code/src/`.

## Key Types
- `TaskManager` — `Arc<RwLock<HashMap<id, TaskStateBase>>>` + outputs map, optional `mpsc::Sender<CoreEvent>` sink (SDK NDJSON parity); `create` / `get` / `update_status` / `stop` / `set_output` / `get_output` / `list` / `remove_completed`
- `TaskOutput` — `{stdout, stderr, exit_code}`
- `TaskDependencies` — `blocks` / `blocked_by` ID arrays
- `PersistentTaskManager` — wraps `TaskManager` with JSON persistence + `add_blocks` / `get_deps` / `is_blocked` / `save` / `load`

Re-exports from `coco_types`: `TaskStateBase`, `TaskStatus` (6 variants), `TaskType` (7 variants: `LocalBash`, `LocalAgent`, `LocalWorkflow`, `RemoteAgent`, `InProcessTeammate`, `MonitorMcp`, `Dream`), `TaskUsage`, `TaskCompletionStatus`, `generate_task_id`.

## Event Emission (WS-6)
When `TaskManager` is constructed with `with_event_sink(tx)`, every lifecycle transition emits:
- `CoreEvent::Protocol(ServerNotification::TaskStarted)` on `create`
- `CoreEvent::Protocol(ServerNotification::TaskProgress)` on non-terminal status updates
- `CoreEvent::Protocol(ServerNotification::TaskCompleted)` on terminal status (with `TaskCompletionStatus` mapping: Completed→Completed, Failed→Failed, Killed|Cancelled→Stopped)

TS parity: `utils/sdkEventQueue.ts` drain pattern.

## Modules
- `output` — task output file handling
- `todo` — TodoV2 checklist item types
