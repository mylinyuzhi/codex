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
| `Theme`, `ThemeName` | 9 built-in themes plus custom `~/.coco/theme.json` palettes; hot-reloaded |
| `DisplaySettings` | TUI display prefs from `settings.json` such as syntax-highlighting enablement |
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
                            native surface draw (ratatui)
                                    ↓
                            UserCommand → coco-query (via mpsc)
```

See `docs/coco-rs/crate-coco-tui.md` for widget taxonomy, overlay catalog, and
snapshot-testing conventions (`insta`).

## Transcript Reader

`Ctrl+O` opens the transcript overlay as a cell-level reader. The projection
keeps lightweight `TranscriptCell` metadata for the full message list, while
the overlay renderer locates the visible cells from `TranscriptOverlay.scroll`
and only renders those cells into the buffer.

Expansion is selected-cell UI state only:

- `Tab` / `Shift+Tab` select expandable cells.
- `Enter` expands/collapses the selected cell.
- Expanded cells are capped internally by a fixed per-cell line cap.

Do not reintroduce a user-facing transcript expansion budget, `Ctrl+E` show-all
mode, or a full transcript `Vec<Line>`/`String` path for overlay rendering.

## Transcript Invariants

The unified transcript refactor
(`docs/coco-rs/engine-tui-unified-transcript-plan.md`) pins three rules:

- **I-1 Authority** — `coco_messages::MessageHistory` is the single source
  of truth. Every transcript mutation emits one of:
  `MessageAppended` / `MessageTruncated` / `SessionResetForResume`.
  Helpers: `coco_query::history_sync::{history_push_and_emit,
  history_clear_and_emit, history_clear_and_emit_session_reset,
  history_replace_and_emit}`. Direct `history.clear()` / `history.messages = ...`
  in production code is a bug — observers desync.
- **I-2 Derived view** — `TranscriptView.cells` is a pure derivation
  from `&Message` via `derive::message_to_cells`. Renderers read
  cells; never mutate cells in place.
- **I-3 UI-only state stays UI-only** — `ui.streaming`,
  `session.tool_executions`, modals, toasts. Not part of transcript.

### Tolerated I-2 exception: `TranscriptView::record_reasoning_tokens`

The `TurnCompleted` handler walks the most recent `AssistantThinking`
cell in `TranscriptView` and stamps `duration_ms` + `reasoning_tokens`
in place. This is **not** a pure re-derivation from the source
`Message` — the engine emits aggregate reasoning usage as a turn-level
stat after the `Reasoning` content has already been streamed and
committed, and there is no per-message metadata-attached event.

Two equivalent fixes are open:
1. Add a `ServerNotification::ReasoningMetadataAttached { message_uuid,
   duration_ms, reasoning_tokens }` event so the engine pushes the
   metadata through the wire like any other transcript-visible field.
2. Have the engine include reasoning usage on the `AssistantMessage`
   itself before the `MessageAppended` emit. Requires the engine to
   know the final usage at push-time (it currently only sees
   `inputUsage` mid-turn).

Until either lands, the in-place cell mutation is the single tolerated
exception. The mutation is idempotent and confined to one method, so
re-deriving cells from `cell.source` after a future fix is a small
refactor.
