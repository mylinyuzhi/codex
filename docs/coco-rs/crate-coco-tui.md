# coco-tui Current Implementation Baseline

Status: current-state documentation for `coco-rs/app/tui`.

This file describes what the TUI does today. The target architecture and
migration roadmap live in `docs/coco-rs/tui-overall-design.md`. If the two
documents disagree, treat this file as the source of current facts and
`tui-overall-design.md` as the source of intended direction.

Do not copy source statistics or long enum listings into this document. The TUI
surface changes quickly; use the canonical source files named below for exact
variant lists.

## Sources

Primary current sources:

- `coco-rs/app/tui/CLAUDE.md`
- `coco-rs/app/tui/src/app.rs`
- `coco-rs/app/tui/src/state/session.rs`
- `coco-rs/app/tui/src/state/ui.rs`
- `coco-rs/app/tui/src/state/overlay.rs`
- `coco-rs/app/tui/src/events.rs`
- `coco-rs/app/tui/src/command.rs`
- `coco-rs/app/tui/src/render.rs`
- `coco-rs/app/tui/src/render_overlays/`
- `coco-rs/app/tui/src/widgets/`
- `coco-rs/common/types/src/event.rs`
- `coco-rs/app/cli/src/tui_runner.rs`

Reference-only sources:

- `/lyz/codespace/3rd/claude-code` is a behavior reference for
  provider-neutral UI flows, not a Rust module template.
- `codex-rs/tui` is a terminal-mechanics reference for ratatui patterns.
- `cocode-rs/` and `codex-rs/` are not the active implementation.

## Role

`coco-tui` is a ratatui/crossterm terminal UI built around The Elm
Architecture:

```text
terminal input / CoreEvent
        -> keybinding and event dispatch
        -> update
        -> AppState
        -> render
        -> terminal frame
```

The crate owns interactive terminal state, rendering, key routing, autocomplete,
overlays, theme state, and notifications. It does not own the agent runtime,
session persistence, tool execution, model invocation, or slash-command
registry construction. Those side effects are orchestrated by
`app/cli/src/tui_runner.rs` and lower layers.

## Dependencies

`coco-tui` uses:

- `coco-types` for `CoreEvent`, protocol payloads, permission modes, model
  roles, reasoning efforts, and common event data.
- `coco-config` for typed environment helpers and display/theme integration
  points.
- `coco-context`, `coco-file-search`, `coco-file-watch`, `coco-git`, and
  `coco-keybindings` for TUI-local services.
- `ratatui`, `crossterm`, `tokio`, `tokio-stream`, `rust-i18n`, `nucleo`,
  `tracing`, `uuid`, and clipboard support.

The crate should not grow direct dependencies on inference, tool execution, or
session-runtime internals just to make a widget easier to render. Runtime
effects should cross the `UserCommand` / `CoreEvent` boundary.

## Event Loop

`App::run` in `app.rs` is the main loop. It multiplexes:

- crossterm keyboard, resize, focus, and paste events
- `CoreEvent` notifications from the agent driver
- async file-search results
- async symbol-search results
- keybinding, theme, display-setting, and config-reload warnings
- tick and spinner timers

High-volume core events are coalesced before redraw so streaming token deltas
do not force one terminal draw per delta.

`create_channels()` returns:

- `mpsc::Sender<UserCommand>` and `Receiver<UserCommand>` for TUI -> runner
- `mpsc::Sender<CoreEvent>` and `Receiver<CoreEvent>` for runner -> TUI

## CoreEvent Consumption

`CoreEvent` has three layers in `common/types/src/event.rs`:

- `CoreEvent::Protocol(ServerNotification)` for SDK-visible session, turn,
  model, task, MCP, hook, sandbox, queue, rewind, cost, and system events.
- `CoreEvent::Stream(AgentStreamEvent)` for streaming text, thinking, tool,
  and MCP tool-call state.
- `CoreEvent::Tui(TuiOnlyEvent)` for TUI-exclusive overlays, toasts, local
  command results, and UI-only prompts.

The TUI handles all three layers in `server_notification_handler/`:

- `protocol.rs` folds `ServerNotification` into `SessionState`.
- `stream.rs` folds `AgentStreamEvent` into streaming/tool display state.
- `tui_only.rs` opens overlays, records local command output, and surfaces
  UI-only status.

The TUI does not define a separate protocol. It consumes the shared event
types and emits `UserCommand` responses.

## State Model

`AppState` is the model:

- `SessionState`: agent-synchronized facts.
- `UiState`: local terminal state.
- `RunningState`: application lifetime state.

### SessionState

Canonical source: `app/tui/src/state/session.rs`.

Important current fields:

- `messages: Vec<ChatMessage>`
- `model`, `provider`
- `model_catalog: Vec<ModelCatalogEntry>`
- `provider_statuses: HashMap<String, ProviderStatus>`
- `model_by_role: HashMap<ModelRole, ModelBinding>`
- `permission_mode: PermissionMode`
- `bypass_permissions_available`, `auto_mode_available`
- `tool_executions`, `subagents`, `active_tasks`, `plan_tasks`,
  `todos_by_agent`, `active_hooks`
- `token_usage`, `context_window_used`, `context_window_total`
- `mcp_servers`, `lsp_active`
- `queued_commands`
- `available_commands`, `available_agents`, `saved_sessions`
- `thinking_effort`, `fast_mode`, model fallback/rate-limit/stream-health
  banners

Plan mode is not a separate session boolean. It is derived from
`permission_mode == PermissionMode::Plan`.

Model data is intentionally structured. Production paths seed
`model_catalog`, `provider_statuses`, and `model_by_role` from
`RuntimeConfig` in `app/cli/src/tui_runner.rs` before the TUI opens the model
picker.

### UiState

Canonical source: `app/tui/src/state/ui.rs`.

Important current fields:

- `input: InputState`
- `paste_manager`
- `scroll_offset`, `focus`
- `overlay: Option<Overlay>`
- `overlay_queue: VecDeque<Overlay>`
- `streaming: Option<StreamingState>`
- `active_suggestions: Option<ActiveSuggestions>`
- `theme`, `theme_state`, `display_settings`
- `toasts`
- `collapsed_tools`
- `help_scroll`
- exit and rewind double-press trackers
- `terminal_focused`
- `clipboard_lease`
- `kb_handle`
- input stash and teammate preview flags

Autocomplete is unified under `active_suggestions`. There are not separate
long-lived state fields for file, skill, agent, and symbol popups.

### InputState

`InputState` stores text, cursor position, and frecency-ranked history.
Cursor positions are character indexes, not byte offsets.

Current prefix modes:

- normal chat input
- `!` bash mode
- memory prefix mode, listed below as migration debt

## Overlay System

Canonical source: `app/tui/src/state/overlay.rs`.

`Overlay` is the typed modal state for permission prompts, model picker,
command/session pickers, question and elicitation forms, sandbox prompts,
search, quick open, export, diff view, MCP approval, worktree/trust/auto-mode
confirmation, task detail, feedback, MCP server select, context visualization,
rewind, settings, plan approval, memory dialog, transcript view, help, and
errors.

`UiState::set_overlay` applies priority ordering:

| Tier | Current variants | Meaning |
|---|---|---|
| 0 | `SandboxPermission` | security-critical |
| 1 | `Permission`, `PlanExit`, `PlanEntry` | blocks agent execution |
| 2 | `Question`, `Elicitation`, `McpServerApproval`, `IdleReturn`, `PlanApproval` | structured input required |
| 3 | `CostWarning`, `BypassPermissions`, `WorktreeExit` | high-stakes confirmation |
| 4 | `Error`, `InvalidConfig` | error surface |
| 5 | `Rewind`, `DiffView` | content review |
| 6 | `AutoModeOptIn`, `Trust`, `Bridge`, `McpServerSelect` | settings or trust confirmation |
| 7 | model/command/session/search/open/export/feedback/task/doctor/context/settings/transcript/memory user surfaces | user-triggered surfaces |
| 8 | `Help` | read-only reference |

Lower number wins. A higher-priority overlay displaces the active overlay into
the queue. Same-or-lower priority overlays are inserted into
`overlay_queue` in priority order. Overflow past `MAX_OVERLAY_QUEUE` drops the
lowest-priority tail.

## User Commands

Canonical source: `app/tui/src/command.rs`.

`UserCommand` is the outbound TUI -> runner contract. It covers user input,
local bash submission, permission responses, permission-mode changes,
thinking-effort changes, role-aware model selection, slash/skill execution,
queueing, compaction, rewind, clear, plan approval, idle hooks, and shutdown.

The current long-term model-switching entrypoint is:

```rust
UserCommand::SetModelRole {
    role: ModelRole,
    provider: String,
    model_id: String,
    effort: Option<ReasoningEffort>,
}
```

Model changes should keep this structured provider/model/role/effort shape.
Do not add display-only model strings that bypass `ModelRole`.

## TUI Commands

Canonical source: `app/tui/src/events.rs`.

`TuiCommand` is the high-level command produced by keybinding resolution. It
covers mode toggles, input editing, cursor movement, scrolling, focus movement,
overlay approval/navigation/filtering, model-picker role/effort axes, task
management, editor/clipboard operations, display toggles, transcript toggles,
stash, expanded views, and exit handling.

Do not duplicate the enum in docs. Keep source as the exact catalog.

## Terminal Input and Mouse

`terminal.rs` enables raw mode, alternate screen, bracketed paste, and focus
change events. It deliberately does not enable mouse capture.

Current behavior:

- `TuiEvent` has key, resize, focus, tick, spinner, paste, and classifier
  events.
- There is no mouse event in `TuiEvent`.
- `app.rs` drops any stray `Event::Mouse` defensively.

This preserves native terminal drag-to-select and Cmd/Ctrl-C behavior.

## Keybinding Routing

Keybinding code lives in:

- `keybinding_setup.rs`
- `keybinding_resolver.rs`
- `keybinding_bridge.rs`
- `keybinding_dispatch.rs`

The bridge derives an active context from `AppState`, then maps crossterm keys
through the configured keybinding handle. The context order is overlay and
surface state first, autocomplete next, then global/input commands.

Keybinding configuration is hot-reloaded by the CLI runner and surfaced as
toasts through the TUI event loop.

## Model Picker Baseline

Current model picker state lives in `ModelPickerOverlay`:

- `role: ModelRole`
- provider/model `entries`
- filter text
- selected filtered-entry index
- optional selected effort

Current behavior:

- Tab and Shift-Tab cycle role across Main, Fast, Plan, Explore, Review,
  HookAgent, Memory, and Subagent.
- Up/Down changes model selection.
- Left/Right changes thinking effort.
- Confirm emits `UserCommand::SetModelRole`.
- Rows are grouped by provider display name and carry typed unavailable
  reasons.
- Production rows come from `SessionState.model_catalog` and
  `SessionState.provider_statuses`.
- `update/show.rs` keeps a builtin fallback that infers provider from model id
  only when the catalog is empty, mainly for tests or pre-bootstrap paths.

Target changes are documented in `tui-overall-design.md`.

## Memory Baseline

Current memory UX has two paths:

- `/memory` opens `Overlay::MemoryDialog` from
  `TuiOnlyEvent::OpenMemoryDialog`.
- The legacy memory prefix path dispatches a runner command that appends a
  bullet directly to `CLAUDE.md`.

The `/memory` dialog currently receives path/label/scope rows only:

- managed
- user
- project
- project-local
- subdirectory

The dialog opens the selected file through current TUI/editor logic and uses
simple string rendering. The target is a row-kind-aware memory picker with
file/folder/toggle rows and side effects routed through a narrow editor/opener
service.

## Autocomplete Baseline

Canonical sources:

- `app/tui/src/autocomplete/`
- `app/tui/src/widgets/suggestion_popup.rs`

Current triggers:

| Trigger | Kind | Data source | Mode |
|---|---|---|---|
| leading `/` | slash command | `session.available_commands` | sync |
| `@agent-` | agent | `session.available_agents` | sync |
| `@path` | file | `FileSearchManager` | async |
| `@#symbol` | symbol | `SymbolSearchManager` | async |

Async triggers install a pending `ActiveSuggestions` value, then `App` dispatches
file or symbol search when the `(kind, query)` pair changes. Stale async results
are discarded if the active trigger no longer matches.

The autocomplete keybinding context activates only when suggestion items are
available, so empty async searches do not hijack history navigation.

## Rendering Baseline

Top-level rendering lives in `render.rs`:

- header
- lifecycle and status banners
- conversation area
- optional side panel
- input area
- queued command list
- status bar
- overlays
- toasts

Message rendering is split under `widgets/chat/`:

- `render_user.rs`
- `render_assistant.rs`
- `render_tool.rs`
- `render_system.rs`

Overlay rendering is split under `render_overlays/`. Most overlays still
produce `(title, body, color)` and are wrapped in a generic centered paragraph.
The model picker already has a custom renderer because it needs richer layout
than the string-body path can provide.

## Layout Baseline

The top-level layout is a full-screen ratatui frame:

```text
header
main conversation plus optional right-side activity panel
input and queued-command area
status bar
overlays and toasts above the base frame
```

The right-side panel is hidden on narrow terminals and switches between tool,
subagent, coordinator, and task views based on current state. One known debt is
that `render.rs` still reads a typed environment helper to decide coordinator
mode during render.

## Theme and Display Settings

TUI theme state is separate from `RuntimeConfig`:

- `~/.coco/theme.json` is loaded and watched by `theme::install_theme`.
- valid reloads update `UiState.theme` and `UiState.theme_state`.
- invalid reloads surface warning toasts and keep the prior palette.

Display settings come from `settings.json` via the CLI runner and hot-reload
into `UiState.display_settings`.

Current widgets commonly read `Theme` fields directly. The target design adds a
minimal semantic `UiStyles` facade so new presentation code depends on style
intent rather than palette field names.

## Notifications and Clipboard

Terminal notification delivery lives in `widgets/notification.rs`.
It detects iTerm2, Kitty, Ghostty, terminal bell, tmux, and screen cases and is
used for turn-complete notifications when the terminal is unfocused.

Clipboard copy lives in `clipboard_copy.rs`:

- SSH prefers OSC 52.
- local sessions prefer `arboard`.
- Linux may keep a clipboard lease in `UiState`.
- macOS suppresses noisy pasteboard stderr while initializing.
- WSL PowerShell is a fallback.
- OSC 52 has a raw payload cap to avoid terminal abuse.

Copy-last-agent-response is available through `/copy` and the configured
clipboard keybinding.

## Known Deviations, Conflict Register, and Migration Debt

These are current facts, not target design:

| Area | Current state | Target direction |
|---|---|---|
| Memory prompt mode | `PromptMode::Memory`, `UserCommand::SubmitMemory`, `TuiOnlyEvent::MemorySaved`, and `MessageContent::MemoryInput` exist for the legacy prefix path. | Remove the prefix path. `/memory` becomes the only memory entrypoint. |
| Direct memory append | `app/cli/src/tui_runner.rs::run_prompt_mode_memory` appends a bullet directly to `CLAUDE.md`. | Route memory editing through the `/memory` picker and editor/opener service. |
| Memory dialog payload | `OpenMemoryDialog` carries path/label/scope only. | Expand to row-kind-aware memory rows with file/folder/toggle semantics. |
| Input rendering | `render.rs::render_input` and `widgets/input.rs` both implement input presentation details. | Collapse to a single input view model and renderer. |
| Render-time config/env decisions | `render.rs` calls typed config env helpers for coordinator mode. | Resolve display mode before render and store it in state/view model. |
| Theme coupling | Widgets and overlay renderers read `Theme` fields directly. | New presentation code goes through `UiStyles`; migrate existing widgets incrementally. |
| Overlay rendering | Many overlays are still string-body dialogs. | Complex overlays move to typed view models and shared dialog/picker/pager primitives. |
| Model picker fallback | Provider inference exists when the session catalog is empty. | Keep inference out of production paths; constrain fallback to tests/pre-bootstrap. |
| Editor spawning | External editor logic is split across update/runner paths. | Route all editor/open intents through one CLI/service boundary. |
| Command dialogs | Some slash command `DialogSpec` variants report dialog-pending status instead of opening a TUI overlay. | Add typed overlays or document intentional non-support per command. |

## Testing

Current TUI tests are colocated companion files, not inline `#[cfg(test)]`
modules. Snapshot tests use `insta` and ratatui `TestBackend`.

Useful commands from `coco-rs/`:

```bash
just quick-check
just test-crate coco-tui
cargo insta pending-snapshots -p coco-tui
cargo insta accept -p coco-tui
```

For documentation-only edits, source and stale-content `rg` checks are normally
enough. Rust checks are required when code changes accompany the docs.

## Maintenance Rules

- Keep this file descriptive, not aspirational.
- Name canonical source files instead of copying long enums.
- Do not record volatile file counts or line counts.
- Use `tui-overall-design.md` for target-state architecture and migration
  sequencing.
- Use `commands/CLAUDE.md` for slash-command parity and deliberate omissions.
- Keep multi-LLM facts structured around provider id, API, model id,
  `ModelRole`, and `ReasoningEffort`.
