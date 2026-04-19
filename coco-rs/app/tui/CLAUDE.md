# coco-tui

Terminal UI using ratatui with The Elm Architecture (TEA). Consumes `CoreEvent`
from `coco-query`; emits `UserCommand` back to core. Replaces TS Ink/React.

## TS Source

- `components/` (~389 files) — chat, permissions, tool displays, overlays, design system
- `screens/{Doctor,REPL,ResumeConversation}.tsx` — top-level screens
- `ink/` (~96 files) — custom React terminal renderer (replaced by ratatui)
- `outputStyles/` — output formatting
- `services/notifier.ts` — terminal bell/OSC notifications
- `utils/{theme,terminal}.ts`, `costHook.ts`, `dialogLaunchers.tsx`, `replLauncher.tsx`

## Architecture (TEA)

```
Model (AppState) ← Update (handle_event) ← Events (TuiEvent) ← View (render)
                                              ↑
                                         CoreEvent (from coco-query)
                                         UserCommand (TUI → core)
```

`App::run` is a `tokio::select!` loop multiplexing: terminal input (crossterm),
`CoreEvent` from agent, file-search/symbol-search results, animation ticks.

## Key Types

| Type | Purpose |
|------|---------|
| `App`, `create_channels` | Run loop + channel constructor |
| `AppState` | Model: `SessionState` + `UiState` + `RunningState` |
| `state::SessionState` | Messages, tool executions, subagents, token usage, plan_mode, permission_mode, MCP servers, team |
| `state::UiState` | Input, overlay stack, scroll, streaming, suggestions, theme |
| `state::Overlay` | 30+ variants: ModelPicker, Permission, Doctor, Export, Feedback, CommandPalette, McpServerApproval, PlanEntry/Exit, Rewind, TrustOverlay, etc. |
| `TuiEvent`, `TuiCommand` | Message enum driving update; chord-aware command dispatch |
| `UserCommand` | TUI → core: SubmitInput, Interrupt, ApprovalResponse, ModelChange, etc. |
| `server_notification_handler::handle_core_event` | Fold `CoreEvent` into `AppState` |
| `render` | Pure `(&AppState, &mut Frame) → ()` — ratatui view |
| `StreamAccumulator` (re-export) | Stream deltas → `ServerNotification` for semantic display |
| `Theme`, `ThemeName` | 5 named themes; crossterm color fallback |
| `PasteManager`, `ImageData`, `ResolvedInput` | Paste buffering + image attachment |
| `Animation` | Spinner/streaming pacing |

## Event Flow

```
Terminal → crossterm::Event → TuiEvent::Key/Resize/Paste/Mouse
                                    ↓
                            keybinding_bridge → TuiCommand
                                    ↓
coco-query → CoreEvent ─────→ handle_core_event (fold into AppState)
                                    ↓
                            update → AppState mutation + optional UserCommand
                                    ↓
                            render (ratatui)
                                    ↓
                            UserCommand → coco-query (via mpsc)
```

See `docs/coco-rs/crate-coco-tui.md` for widget taxonomy, overlay catalog, and
snapshot-testing conventions (`insta`).
