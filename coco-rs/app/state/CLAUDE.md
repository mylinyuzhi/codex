# coco-state

Central application state tree (`Arc<RwLock<AppState>>`). Zustand-like shared
state touching ~80 fields; plus the **swarm** subsystem (21 modules) that ports
coordinator / team / multi-pane orchestration.

## TS Source

- `state/{AppState.tsx,AppStateStore.ts,store.ts,onChangeAppState.ts,selectors.ts,teammateViewHelpers.ts}`
- `bootstrap/state.ts` — non-reactive process singleton (session_id, cost accumulators, beta latches, client_type, etc.)
- `projectOnboardingState.ts` — onboarding step tracker
- `coordinator/coordinatorMode.ts` + `utils/swarm/` (backends: iTerm2/tmux/InProcess/Pane, layout, reconnect, mailbox, teammate prompt/model) — swarm/team orchestration

## Key Types

| Type | Purpose |
|------|---------|
| `AppState` | Model, session, agent, token tracking, tasks, MCP, plugins, notifications, speculation, bridge, team/inbox, coordinator, elicitation, sandbox, onboarding, bootstrap_data |
| `McpClientState`, `PluginState`, `NotificationState`, `TaskEntry`, `InboxEntry` | Field substructures |
| `PendingWorkerRequest`, `PendingSandboxRequest`, `WorkerSandboxPermissions` | Leader-side queues for multi-agent permission arbitration |
| `TeamContext`, `StandaloneAgentContext` | Active team membership / agent identity |
| `ElicitationEntry` | MCP elicitation queue |
| `SubAgentState` | Per-subagent status map |

## Swarm Subsystem (21 modules)

Coordinator-mode infrastructure. `swarm_runner` drives the outer loop;
`swarm_runner_loop` handles per-iteration scheduling.

| Module group | Purpose |
|--------------|---------|
| `swarm_runner`, `swarm_runner_loop`, `swarm_agent_handle` | Lifecycle + per-agent handles |
| `swarm_backend` (+ `iterm2`, `pane`, `tmux`) | Terminal layout backends (iTerm2 panes, tmux windows) |
| `swarm_config`, `swarm_constants`, `swarm_discovery`, `swarm_identity` | Config + well-known paths + teammate discovery |
| `swarm_file_io`, `swarm_mailbox`, `swarm_prompt` | Per-agent mailbox files + prompt assembly |
| `swarm_layout`, `swarm_it2_setup` | Terminal layout + iTerm2 setup |
| `swarm_reconnect`, `swarm_spawn_utils`, `swarm_task`, `swarm_teammate` | Reconnect, spawn helpers, task routing, teammate bookkeeping |

See `docs/coco-rs/crate-coco-coordinator.md` for the coordinator design (v2).

## Conventions

- Always use `Arc<RwLock<AppState>>` — never clone the state itself.
- Field access is serde-stable (snake_case); `#[serde(default)]` on all optional fields.
- `onChangeAppState` side-effects (persisting settings, CCR notify) are **not** ported here — handled by the owning crate on mutation.
