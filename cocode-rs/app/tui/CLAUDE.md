# CLAUDE.md - cocode-tui Development Guide

## Architecture

The TUI follows **The Elm Architecture (TEA)** with async event handling:

```
┌─────────────────────────────────────────────────────────────────┐
│                         TUI Layer                                │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐  │
│  │  Model   │◄───│  Update  │◄───│  Render  │◄───│  Events  │  │
│  │(AppState)│    │(update.rs)│   │(render.rs)│   │(stream.rs)│  │
│  └──────────┘    └──────────┘    └──────────┘    └──────────┘  │
└─────────────────────────────────────────────────────────────────┘
         ▲                                              │
         └──────────────────────────────────────────────┘
```

## Key Files

| File | Purpose |
|------|---------|
| `app.rs` | Main `App` struct, async run loop with `tokio::select!` |
| `command.rs` | `UserCommand` enum (TUI → Core) |
| `update.rs` | `handle_command()`, `handle_agent_event()` |
| `render.rs` | `render()` function, overlay rendering, help overlay |
| `state/mod.rs` | `AppState`, `SessionState`, `UiState` |
| `state/ui.rs` | `UiState`, `InputState`, `Overlay`, suggestion states |
| `state/session.rs` | `SessionState` (model, thinking, plan_mode, messages, tools) |
| `event/mod.rs` | `TuiEvent`, `TuiCommand` enums |
| `event/handler.rs` | Key event → `TuiCommand` mapping (all keybindings defined here) |
| `event/broker.rs` | `EventBroker` — pause/resume stdin for external editors |
| `event/stream.rs` | `TuiEventStream` — async event stream |
| `i18n/mod.rs` | Internationalization with `rust-i18n`, `t!()` macro |
| `file_search.rs` | File autocomplete (`@path` mentions) |
| `skill_search.rs` | Skill autocomplete (`/command` slash commands) |
| `agent_search.rs` | Agent autocomplete (`@agent-*` mentions) |
| `symbol_search.rs` | Symbol autocomplete (`@#symbol` mentions) |
| `clipboard_paste.rs` | Clipboard handling (image + text paste) |
| `paste.rs` | Bracketed paste mode handling |
| `editor.rs` | External editor integration (Ctrl+E) |
| `terminal.rs` | Terminal setup/teardown, alternate screen |
| `theme.rs` | Theme definitions and colors |
| `lib.rs` | Crate root, re-exports |

### Widgets (`widgets/`)

| Widget | Purpose |
|--------|---------|
| `chat.rs` | Chat history with thinking display, tool results |
| `input.rs` | Multi-line input with cursor, selection |
| `status_bar.rs` | Bottom bar: model, thinking level, plan mode, MCP, tokens |
| `tool_panel.rs` | Running/completed tool calls display |
| `subagent_panel.rs` | Subagent status (running/completed/backgrounded) |
| `toast.rs` | Toast notifications (info/success/warning/error, auto-expire) |
| `queued_list.rs` | Queued steering messages display |
| `file_suggestion_popup.rs` | File autocomplete dropdown |
| `skill_suggestion_popup.rs` | Skill autocomplete dropdown |
| `agent_suggestion_popup.rs` | Agent autocomplete dropdown |
| `symbol_suggestion_popup.rs` | Symbol autocomplete dropdown |

### Locales (`locales/`)

| File | Purpose |
|------|---------|
| `en.yaml` | English translations (base/fallback) |
| `zh-CN.yaml` | Simplified Chinese translations |

## Communication Channels

```rust
// Core → TUI: LoopEvent (streaming, tools, approvals)
let (agent_tx, agent_rx) = mpsc::channel::<LoopEvent>(32);

// TUI → Core: UserCommand (input, interrupts, settings)
let (command_tx, command_rx) = mpsc::channel::<UserCommand>(32);

// File search: async fuzzy file search results
let (file_search_tx, file_search_rx) = mpsc::channel(16);

// Symbol search: async LSP symbol search results
let (symbol_search_tx, symbol_search_rx) = mpsc::channel(16);
```

**UserCommand variants:** `SubmitInput`, `Interrupt`, `SetPlanMode`, `SetThinkingLevel`, `SetModel`, `ApprovalResponse`, `ExecuteSkill`, `QueueCommand`, `BackgroundAllTasks`, `ClearQueues`, `Shutdown`

## Keyboard Shortcuts

All keybindings are defined in `event/handler.rs`. No separate `keybindings.rs` — the handler functions themselves are the single source of truth.

### Event Handling Priority

```
overlay > skill suggestions > agent suggestions > symbol suggestions > file suggestions > global keys > input keys
```

### Global Shortcuts (`handle_global_key`)

| Key | Action | TuiCommand |
|-----|--------|------------|
| Tab | Toggle plan mode | `TogglePlanMode` |
| Ctrl+T | Cycle thinking level | `CycleThinkingLevel` |
| Ctrl+Shift+T | Toggle thinking display | `ToggleThinking` |
| Ctrl+M | Model picker | `CycleModel` |
| Ctrl+B | Background all tasks | `BackgroundAllTasks` |
| Ctrl+C | Interrupt | `Interrupt` |
| Ctrl+L | Clear screen | `ClearScreen` |
| Ctrl+E | Open external editor | `OpenExternalEditor` |
| Ctrl+P | Command palette | `ShowCommandPalette` |
| Ctrl+S | Session browser | `ShowSessionBrowser` |
| Ctrl+V / Alt+V | Smart paste (image first, text fallback) | `PasteFromClipboard` |
| Ctrl+Q | Quit | `Quit` |
| ? / F1 | Show help | `ShowHelp` |
| Esc | Cancel/close | `Cancel` |
| PageUp / Ctrl+Up | Page up | `PageUp` |
| PageDown / Ctrl+Down | Page down | `PageDown` |

### Input Keys (`handle_input_key_with_streaming`)

| Key | Not Streaming | Streaming |
|-----|---------------|-----------|
| Enter / Ctrl+Enter | `SubmitInput` | `QueueInput` |
| Shift+Enter / Alt+Enter | `InsertNewline` | `InsertNewline` |
| Alt+Up/Down | `ScrollUp`/`ScrollDown` | same |
| Ctrl+Left/Right | `WordLeft`/`WordRight` | same |
| Ctrl+Backspace | `DeleteWordBackward` | same |
| Ctrl+Delete | `DeleteWordForward` | same |

### Overlay Keys (`handle_overlay_key`)

| Key | Action |
|-----|--------|
| Y | Approve |
| N | Deny |
| Ctrl+A | Approve All |
| Up/k, Down/j | Navigate |
| Enter | Approve/Select |
| Esc / Ctrl+C | Cancel |
| Char input | Filter (model picker, command palette, session browser) |

### Suggestion Keys (all 4 autocomplete types)

| Key | Action |
|-----|--------|
| Up/Down | Navigate suggestions |
| Tab/Enter | Accept suggestion |
| Esc | Dismiss suggestions |

## Autocomplete Systems

| Trigger | System | Module | Widget |
|---------|--------|--------|--------|
| `@path` | File search | `file_search.rs` | `file_suggestion_popup.rs` |
| `/command` | Skill search | `skill_search.rs` | `skill_suggestion_popup.rs` |
| `@agent-*` | Agent search | `agent_search.rs` | `agent_suggestion_popup.rs` |
| `@#symbol` | Symbol search | `symbol_search.rs` | `symbol_suggestion_popup.rs` |

## Overlay Types

| Variant | Trigger | State |
|---------|---------|-------|
| `Permission` | Tool approval request | `PermissionOverlay` (Y/N/A) |
| `ModelPicker` | Ctrl+M → model list | `ModelPickerOverlay` (filter, select) |
| `CommandPalette` | Ctrl+P | `CommandPaletteOverlay` (filter, select) |
| `SessionBrowser` | Ctrl+S | `SessionBrowserOverlay` (filter, select) |
| `Help` | ? / F1 | Static display |
| `Error` | Error events | String message |

## State Structure

```rust
pub struct AppState {
    pub session: SessionState,  // model, thinking_level, plan_mode, messages, tools, subagents
    pub ui: UiState,            // input, scroll, focus, overlay, streaming, suggestions, toasts, theme
    pub running: RunningState,  // Running | Done
}
```

## i18n

Uses `rust-i18n` with the `t!()` macro. Locale files in `locales/{en,zh-CN}.yaml`.

Translation key namespaces: `command.*`, `status.*`, `dialog.*`, `help.*`, `toast.*`, `chat.*`, `input.*`, `palette.*`, `tool.*`, `subagent.*`.

## Adding New Features

1. **New keyboard shortcut**: Add to `event/handler.rs` → add `TuiCommand` variant in `event/mod.rs` → handle in `update.rs`
2. **New overlay**: Add variant to `state/ui.rs::Overlay`, render in `render.rs`
3. **Handle new LoopEvent**: Add case in `update.rs::handle_agent_event()`
4. **New widget**: Create in `widgets/`, use in `render.rs`
5. **New UserCommand**: Add to `command.rs`, handle in `tui_runner.rs` (`handle_turn_command` + `handle_idle_command`)
6. **New i18n key**: Add to both `locales/en.yaml` and `locales/zh-CN.yaml`

## Development Commands

```bash
# From cocode-rs/ directory
just check          # Type-check all crates
just test           # Run all tests
just fmt            # Format code
just pre-commit     # REQUIRED before commit
just clippy         # Run clippy
```

## Code Conventions

**DO:**
- Use `i32`/`i64` (never `u32`/`u64`)
- Inline format args: `format!("{var}")`
- Chain Stylize helpers
- Filter `KeyEventKind::Press` for cross-platform
- Use `t!()` for all user-facing strings

**DON'T:**
- Use `.unwrap()` in non-test code
- Use `.white()` (breaks themes)
- Block the render loop
- Hardcode user-facing strings (use i18n)
