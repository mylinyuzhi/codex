# coco-tui — Crate Plan

TS source: `src/components/` (389 files, ~68K LOC), `src/screens/` (3 files, 6K LOC),
`src/ink/` (96 files — custom React terminal renderer), `src/outputStyles/`,
`src/services/notifier.ts`, `src/hooks/` (UI-relevant subset)

cocode-rs source: `app/tui/` (55 source files, 18.6K LOC) — production-ready TEA implementation

## Strategy: HYBRID (cocode-rs TEA architecture + TS component taxonomy)

cocode-rs provides the complete TUI infrastructure: TEA pattern, event loop, widget system,
keybindings, streaming, overlays, themes, i18n. TS defines the component taxonomy (389 files)
and UI behaviors that need Rust widget implementations.

| Layer | Source | Status |
|-------|--------|--------|
| TEA event loop (`tokio::select!` multiplexing) | cocode-rs `app.rs` | **KEEP** |
| State model (AppState = SessionState + UiState) | cocode-rs `state/` | **KEEP** (extend fields) |
| Event system (CoreEvent → 3-layer dispatch) | cocode-rs + `event-system-design.md` | **KEEP** |
| Widget infrastructure (ratatui Widget trait) | cocode-rs `widgets/` | **KEEP** (extend set) |
| Keybinding bridge (context-aware resolution) | cocode-rs `keybinding_bridge.rs` | **KEEP** |
| Streaming display (pacing, accumulation) | cocode-rs `streaming/` | **KEEP** |
| Overlay system (modal queue with priority) | cocode-rs `state/ui.rs` | **KEEP** (extend types) |
| Theme system (5 named themes) | cocode-rs `theme.rs` | **KEEP** |
| i18n (rust-i18n, en + zh-CN) | cocode-rs `i18n/` | **KEEP** |
| TS component taxonomy (41 message renderers) | TS `components/messages/` | **Port** (widget impls) |
| TS dialog types (44 dialog variants) | TS `components/permissions/` etc. | **Port** (overlay variants) |
| TS design system (16 primitives) | TS `components/design-system/` | **Map** to ratatui primitives |
| TS notification backends (5 terminal types) | TS `services/notifier.ts` | **Port** |

## Dependencies

```
coco-tui depends on:
  - coco-types, coco-config, coco-state (AppState)
  - coco-query (re-exports CoreEvent from coco-types — QueryEngine emits these directly)
  - coco-commands (CommandRegistry — for command palette)
  - ratatui, crossterm

coco-tui does NOT depend on:
  - coco-tools, coco-inference, coco-shell (no direct tool/LLM calls)
  - coco-session (session persistence is coco-cli's responsibility)
```

## Event System Integration

The TUI is a **consumer** of the 3-layer CoreEvent system defined in `event-system-design.md`.
It does NOT define its own event protocol — it receives events from the agent loop via mpsc channels.

```
Agent Loop (core/loop)
    │
    │ emit(CoreEvent)
    ▼
┌─────────────────────────────────────────────┐
│ TUI Event Loop (tokio::select!)              │
│                                              │
│  agent_rx     → ServerNotification handler   │  ← Protocol events (43+ variants)
│               → StreamEvent → accumulator    │  ← Streaming deltas
│               → TuiEvent → overlay/toast     │  ← UI-only events (20 variants)
│                                              │
│  event_stream → Key/Mouse/Resize/Paste       │  ← Terminal input
│               → Tick/SpinnerTick/Draw        │  ← Timing events
│                                              │
│  file_search_rx  → suggestion updates        │  ← Async file search
│  symbol_search_rx → suggestion updates       │  ← Async LSP symbols
│                                              │
│  command_tx   → UserCommand → Core           │  ← TUI→Core responses
└─────────────────────────────────────────────┘
```

See `event-system-design.md` for complete event catalogs:
- **ServerNotification**: 52 variants (43 existing + 9 proposed) — SDK + TUI
- **StreamEvent**: 7 variants → StreamAccumulator → ThreadItem mapping
- **TuiEvent**: 20 variants — TUI-exclusive overlays, toasts, streaming deltas
- **ClientRequest**: 29 variants — control protocol from SDK consumers
- **UserCommand**: TUI→Core responses (approval, input, model change, etc.)

## Architecture: The Elm Architecture (TEA)

Follows cocode-rs pattern exactly:

```
Model (AppState)
    ↑ &mut
    │
Update (handle_command, handle_server_notification, handle_stream_event_tui)
    ↑
    │
View (render: &AppState → Frame)
    ↑
    │
Events (TuiEvent → keybinding_bridge → TuiCommand)
```

### State Model (from cocode-rs, extended)

```rust
pub struct AppState {
    pub session: SessionState,  // Protocol-level: messages, tools, subagents, model, tokens
    pub ui: UiState,            // UI-only: input, overlay, scroll, streaming, suggestions, theme
    pub running: RunningState,  // Running | Done
}

// SessionState: synchronized with agent loop via CoreEvent
pub struct SessionState {
    pub messages: Vec<ChatMessage>,
    pub tool_executions: Vec<ToolExecution>,
    pub subagents: Vec<SubagentInstance>,
    pub background_tasks: Vec<BackgroundTask>,
    pub current_selection: Option<RoleSelection>,  // model + thinking
    pub plan_mode: bool,
    pub permission_mode: PermissionMode,
    pub token_usage: TokenUsage,
    pub context_window_used: i64,
    pub context_window_total: i64,
    pub mcp_servers: Vec<McpServerStatus>,
    pub team: Option<TeamInfo>,
    pub queued_commands: Vec<UserQueuedCommand>,
    pub working_dir: String,
    pub session_id: String,
    pub turn_count: i32,
}

// UiState: local TUI state, never sent to agent
pub struct UiState {
    pub input: InputState,               // multi-line input with history
    pub overlay: Option<Overlay>,        // active modal
    pub overlay_queue: VecDeque<Overlay>, // queued overlays (priority-ordered)
    pub scroll_offset: i32,
    pub streaming: Option<StreamingState>,
    pub file_suggestions: FileSuggestionState,
    pub skill_suggestions: SkillSuggestionState,
    pub agent_suggestions: AgentSuggestionState,
    pub symbol_suggestions: SymbolSuggestionState,
    pub show_thinking: bool,
    pub theme: Theme,
    pub toasts: VecDeque<Toast>,
    pub animation: Animation,
    pub collapsed_tools: HashSet<String>,
    pub focus: FocusTarget,              // Input | Chat
    pub query_timing: QueryTiming,
}
```

### TuiCommand Enum

Canonical source: `app/tui/src/events.rs`. Grouped by category (matches the
`update.rs` dispatch sections):

```rust
pub enum TuiCommand {
    // Mode toggles
    TogglePlanMode, CyclePermissionMode, CycleThinkingLevel, ToggleThinking,
    CycleModel, ToggleFastMode,

    // Input actions
    SubmitInput, QueueInput, Interrupt, Cancel, ClearScreen,

    // Text editing (Emacs-flavored)
    InsertChar(char), InsertNewline,
    DeleteBackward, DeleteForward, DeleteWordBackward, DeleteWordForward,
    KillToEndOfLine, Yank,

    // Cursor movement
    CursorLeft, CursorRight, CursorUp, CursorDown,
    CursorHome, CursorEnd, WordLeft, WordRight,

    // Scrolling
    ScrollUp, ScrollDown, PageUp, PageDown,

    // Focus
    FocusNext, FocusPrevious, FocusNextAgent, FocusPrevAgent,

    // Overlay actions
    Approve, Deny, ApproveAll,
    ClassifierAutoApprove { request_id: String, matched_rule: Option<String> },

    // Overlay navigation — generic across pickers/scrollables/suggestions
    OverlayFilter(char), OverlayFilterBackspace,
    OverlayNext, OverlayPrev, OverlayConfirm,

    // Commands & overlays
    ExecuteSkill(String),
    ShowHelp, ShowCommandPalette, ShowSessionBrowser,
    ShowGlobalSearch, ShowQuickOpen, ShowExport,
    ShowContextViz, ShowRewind, ShowDoctor,
    ShowSettings, SettingsNextTab, SettingsPrevTab,

    // Mouse
    MouseScroll(i32), MouseClick { col: u16, row: u16 },

    // Task management
    BackgroundAllTasks, KillAllAgents,

    // External editor / clipboard
    OpenExternalEditor, OpenPlanEditor, PasteFromClipboard, CopyLastMessage,

    // Display toggles
    ToggleToolCollapse, ToggleSystemReminders,

    // Application
    Quit,
}
```

Autocomplete dispatch uses the generic `OverlayFilter*` / `Overlay{Next,Prev,
Confirm}` variants — context (`KeybindingContext::Autocomplete`) is resolved
in `keybinding_bridge.rs`. There is no per-suggestion-type context enum;
Skill/Agent/Symbol/File suggestions share the same Tab/Up/Down/Esc handling.

### UserCommand Enum (TUI → Core)

Canonical source: `app/tui/src/command.rs`.

```rust
pub enum UserCommand {
    SubmitInput { content: String, display_text: Option<String>, images: Vec<ImageData> },
    Interrupt,
    SetPlanMode { active: bool },
    SetPermissionMode { mode: PermissionMode },
    SetThinkingLevel { level: String },
    SetModel { model: String },
    ApprovalResponse {
        request_id: String,
        approved: bool,
        always_allow: bool,
        feedback: Option<String>,
        updated_input: Option<Value>,
        permission_updates: Vec<PermissionUpdate>,
    },
    ExecuteSkill { name: String, args: Option<String> },
    QueueCommand { prompt: String },
    BackgroundAllTasks,
    KillAllAgents,
    ToggleFastMode,
    Compact,
    Rewind { message_id: String, restore_type: RestoreType },
    RequestDiffStats { message_id: String },
    Shutdown,
}
```

The TS-side `QuestionResponse` / `ElicitationResponse` flow through
`ApprovalResponse` with `feedback: Some(chosen_option)` — the overlay's
`handle_overlay_confirm` packs the selection into the `feedback` field
rather than introducing dedicated variants.

## Layout

```
┌─────────────────────────────────────────┐
│ HeaderBar (1 line) — session, branch    │  Constraint::Length(1)
├─────────────────────────────────────────┤
│                     │                   │
│  ChatWidget (main)  │ ToolPanel/        │  Constraint::Min(1)
│  + StreamingView    │ SubagentPanel     │
│                     │ (if tools exist)  │
│                     │                   │
│  InputWidget        │                   │
│  QueuedListWidget   │                   │
│                     │                   │
├─────────────────────────────────────────┤
│ StatusBar (1 line) — model, tokens      │  Constraint::Length(1)
└─────────────────────────────────────────┘
     ↑
Overlays (modal, centered, priority-queued)
Toasts (top-right corner, auto-expire)
```

### Responsive Breakpoints

```rust
const SIDE_PANEL_MIN_WIDTH: u16 = 120;   // hide panel on narrow terminals
const WIDE_TERMINAL_WIDTH: u16 = 160;

// < 120 cols: chat only, no side panel
// 120-159 cols: 70% chat / 30% tools
// >= 160 cols: 75% chat / 25% tools
```

## Widget Catalog

### Core Widgets (from cocode-rs, 17 existing)

| Widget | Purpose | ratatui Pattern |
|--------|---------|-----------------|
| `ChatWidget` | Conversation with markdown, thinking, tools | Stateless, builder pattern |
| `InputWidget` | Multi-line input with syntax highlighting | Stateless, tokenization |
| `StatusBar` | Model, thinking, tokens, MCP, plan mode | Stateless |
| `HeaderBar` | Session ID, cwd, turn count, branch | Stateless |
| `ToolPanel` | Running/completed tool calls with elapsed time | Stateless |
| `SubagentPanel` | Subagent instances with focus indicator | Stateless |
| `ToastWidget` | Auto-expiring notification bubbles | Stateless |
| `QueuedListWidget` | Commands queued during streaming | Stateless |
| `TeamPanel` | Team member coordination display | Stateless |
| `FileSuggestionPopup` | @path autocomplete | Stateless |
| `SkillSuggestionPopup` | /command autocomplete | Stateless |
| `AgentSuggestionPopup` | @agent-* autocomplete | Stateless |
| `SymbolSuggestionPopup` | @#symbol autocomplete | Stateless |
| `DiffDisplay` | Code diff rendering | Stateless |
| `Markdown` | Markdown→ratatui Line conversion | Helper function |
| `SuggestionPopup` | Shared popup infrastructure | Stateless |
| `Spinner` | Animated progress indicator | Time-based |

### Message Renderers (from TS, 41 types → Rust widget variants)

TS has 41 dedicated React components for message types. In Rust, these map to a
`ChatMessage` enum with per-variant render logic inside `ChatWidget`:

```rust
pub enum ChatMessage {
    // Assistant (5 types)
    AssistantText { id: String, content: String },
    AssistantThinking { id: String, content: String, duration_ms: Option<i64> },
    AssistantRedactedThinking { id: String },
    AssistantToolUse { id: String, tool_name: String, input: Value, status: ToolStatus },
    AdvisorMessage { id: String, content: String },

    // User (15 types)
    UserPrompt { id: String, content: String },
    UserImage { id: String, path: String },
    UserBashInput { id: String, command: String },
    UserBashOutput { id: String, output: String, exit_code: i32 },
    UserCommand { id: String, command: String },
    UserPlan { id: String, action: PlanAction },
    UserMemoryInput { id: String, content: String },
    UserAgentNotification { id: String, agent_id: String, summary: String },
    UserTeammate { id: String, teammate: String, content: String },
    UserChannel { id: String, channel: String, content: String },
    UserLocalCommandOutput { id: String, content: String },
    UserResourceUpdate { id: String, resource: String },
    UserAttachment { id: String, attachment_type: String, content: String },
    UserText { id: String, content: String },
    // ...

    // Tool Result (7 types)
    ToolSuccess { id: String, tool_name: String, output: String },
    ToolError { id: String, tool_name: String, error: String },
    ToolRejected { id: String, tool_name: String, reason: String },
    ToolCanceled { id: String, tool_name: String },
    RejectedToolUse { id: String, tool_name: String },
    // ...

    // System (8 types)
    SystemText { id: String, content: String },
    SystemApiError { id: String, error: String, retryable: bool },
    RateLimit { id: String, info: RateLimitInfo },
    Shutdown { id: String },
    HookProgress { id: String, hook_name: String, output: String },
    PlanApproval { id: String, plan: String },
    TaskAssignment { id: String, task_id: String },
    CompactBoundary { id: String },
}

impl ChatWidget<'_> {
    fn render_message(&self, msg: &ChatMessage, area: Rect, buf: &mut Buffer) {
        match msg {
            ChatMessage::AssistantText { content, .. } => self.render_markdown(content, area, buf),
            ChatMessage::AssistantThinking { content, duration_ms, .. } => {
                // Collapsible thinking block with duration
            }
            ChatMessage::AssistantToolUse { tool_name, status, .. } => {
                // Tool icon + name + status indicator
            }
            // ... pattern match for each variant
        }
    }
}
```

### Overlay System (from cocode-rs, 14 existing — extend from TS 44 dialog types)

```rust
pub enum Overlay {
    // Existing (cocode-rs)
    Permission(PermissionOverlay),
    ModelPicker(ModelPickerOverlay),
    CommandPalette(CommandPaletteOverlay),
    SessionBrowser(SessionBrowserOverlay),
    Help(HelpContent),
    Error(String),
    Question(QuestionOverlay),
    Elicitation(ElicitationOverlay),
    CostWarning(CostWarningOverlay),
    PlanExit(PlanExitOverlay),
    SandboxPermission(SandboxPermissionOverlay),
    PluginManager(PluginManagerOverlay),
    RewindSelector(RewindSelectorOverlay),
    OutputStylePicker(OutputStylePickerOverlay),

    // Added (v1 implementation)
    Settings(SettingsPanelState),   // tabbed: Theme / OutputStyle / Permissions / About

    // Future (from TS dialog types — add as needed)
    AgentEditor(AgentEditorOverlay),
    McpSettings(McpSettingsOverlay),
    BackgroundTasks(BackgroundTasksOverlay),
    // ...
}

/// Overlay queue: agent-driven overlays queue during transition gates.
/// User-triggered overlays displace agent overlays.
/// Max queue size: MAX_OVERLAY_QUEUE constant.
```

### Permission Widgets (from TS, 51 files → Rust overlay variants)

TS has 12 tool-specific permission request components. In Rust, these are
variants of `PermissionOverlay` with per-tool render logic:

```rust
pub struct PermissionOverlay {
    pub request_id: String,
    pub tool_name: String,
    pub input: Value,
    pub detail: PermissionDetail,
}

pub enum PermissionDetail {
    Bash { command: String, risk_level: SecurityRisk },
    FileEdit { path: String, diff: String },
    FileWrite { path: String, content_preview: String },
    WebFetch { url: String },
    Skill { skill_name: String, description: String },
    NotebookEdit { path: String, cell_id: String },
    Filesystem { operation: String, path: String },
    PlanModeEnter,
    PlanModeExit { plan: Option<String> },
    ComputerUse { action: String },
    SedEdit { path: String, pattern: String, replacement: String },
    Fallback { description: String },
}
```

## Notification Backends

Implemented in `app/tui/src/widgets/notification.rs`. Detects the terminal from
`$TERM_PROGRAM` / `$LC_TERMINAL` / `$TERM` and emits the appropriate OSC
sequence. Auto-wraps outputs for tmux (`ESC P tmux; … ESC \`) and GNU screen
DCS passthrough when `$TMUX` / `$STY` is set.

```rust
pub enum NotificationBackend {
    ITerm2,          // OSC 9;1;<payload>ST
    ITerm2WithBell,  // OSC 9;1 + BEL (tmux bell-action fallback)
    Kitty,           // OSC 99 title + body + focus frames (3 writes, shared id)
    Ghostty,         // OSC 777;notify;<title>;<body>ST
    TerminalBell,    // raw BEL (\x07)
    Disabled,        // no supported channel for current terminal
}

impl NotificationBackend {
    pub fn detect() -> Self;
    pub fn send(self, writer: &mut impl Write, title: &str, message: &str) -> io::Result<()>;
}

/// Convenience: detect + send to stdout, best-effort.
pub fn notify(title: &str, message: &str);
```

The BEL character is always emitted raw (never DCS-wrapped) so tmux's own
`bell-action` handler fires and the visual cue propagates to the outer
terminal.

### Turn-complete gating

`server_notification_handler::protocol::on_turn_completed` calls
`notification::notify("coco", "Turn complete")` when `state.ui.terminal_focused`
is false. `TuiEvent::FocusChanged` (from crossterm's focus-change mode) sets
this flag. Focused terminals never see a notification — the user is already
looking.

## Streaming Display

```rust
/// Adaptive pacing for streaming content display.
/// Separate buffers for text and thinking content.
/// Tool uses tracked as they stream in.
pub struct StreamingState {
    pub turn_id: String,
    pub content: String,          // accumulated text deltas
    pub thinking: String,         // accumulated thinking deltas
    pub tool_uses: Vec<StreamingToolUse>,
    pub display_cursor: usize,    // for adaptive pacing (catchup)
    pub mode: StreamMode,         // Text | ThinkingText | ToolUse
}

pub enum StreamMode { Text, ThinkingText, ToolUse }
```

## Autocomplete Systems (4 parallel — sync + async)

Single unified state: `UiState.active_suggestions: Option<ActiveSuggestions>`.
A pure `detect(text, cursor) -> Option<Trigger>` recognises four triggers:

| Trigger   | Kind          | Data source                   | Mode     |
|-----------|---------------|-------------------------------|----------|
| `/foo`    | SlashCommand  | `session.available_commands`  | Sync     |
| `@agent-` | Agent         | `session.available_agents`    | Sync     |
| `@path`   | File          | `FileSearchManager` (mpsc)    | Async    |
| `@#sym`   | Symbol        | `SymbolSearchManager` (mpsc)  | Async    |

Sync kinds populate `items` inline in `autocomplete::refresh_suggestions`.
Async kinds install the trigger with empty `items` so the App loop can see
the query and call `manager.search(query, pos)`; results arrive through a
dedicated mpsc arm and are applied via `autocomplete::apply_async_result`,
which discards stale results (different kind or different query).

```rust
pub enum SuggestionKind { SlashCommand, File, Agent, Symbol }

pub struct ActiveSuggestions {
    pub kind: SuggestionKind,
    pub items: Vec<SuggestionItem>,
    pub selected: i32,
    pub query: String,
    pub trigger_pos: i32,
}
```

**Keybinding gate**: `active_context()` returns `Autocomplete` only when
`items` is non-empty. Async triggers therefore don't hijack arrow keys
before results arrive — the user can still navigate history while the
search is in flight.

**Lifecycle** (App event loop):
1. User types — `update::handle_command` calls `refresh_suggestions` after
   detecting text/cursor change.
2. For async triggers, `App::dispatch_pending_search` fires manager.search
   if `(kind, query)` differs from the last dispatch. Previous searches
   are aborted by the manager's internal JoinHandle cancel.
3. Results arrive on `file_search_rx` / `symbol_search_rx` → `apply_async_result`
   updates `items` if the query still matches.
4. Tab or Enter → `accept_suggestion` splices the selected label back into
   the input at `trigger_pos` (with a trailing space) and dismisses the
   popup. Esc dismisses without applying.

## Output Styles

```rust
/// Markdown-based output styles loaded from:
/// - Project: .claude/output-styles/*.md
/// - User: ~/.coco/output-styles/*.md
/// - Plugins: plugin-contributed styles
/// Supports frontmatter: name, description, keep-coding-instructions, force-for-plugin.
pub struct OutputStyle {
    pub name: String,
    pub description: Option<String>,
    pub content: String,  // markdown body
    pub keep_coding_instructions: bool,
}
```

## Markdown Rendering Pipeline

TS uses `marked` (GFM parser) + `cliHighlight` for syntax highlighting + module-level token cache (500 entries LRU).
Rust uses a `markdown_to_lines()` function in `widgets/markdown.rs`.

```rust
/// Convert markdown text to styled ratatui Lines.
/// Only assistant messages use markdown parsing; user messages use plain wrapping.
///
/// Supported elements: paragraphs, headers (#-######), code blocks (``` with language),
/// inline code (`), bold (**), italic (*), lists (- and 1.), links, tables, blockquotes.
///
/// Performance: TS uses fast-path regex scan of first 500 chars to skip plain text.
/// Rust equivalent: check for markdown markers before full parse.
///
/// TS caches parsed tokens per content hash (LRU 500) to survive virtual-scroll remounts.
/// Rust: cache not needed (no React remount overhead), but can cache for large messages.
pub fn markdown_to_lines(content: &str, width: u16, theme: &Theme) -> Vec<Line<'static>>;

/// Syntax highlighting for code blocks.
/// TS uses cliHighlight (async). Rust can use syntect or tree-sitter-highlight.
pub fn highlight_code(code: &str, language: &str) -> Vec<Line<'static>>;
```

### Table Rendering

```rust
/// Tables use adaptive layout:
/// - Columns get proportional width based on content
/// - MIN_COLUMN_WIDTH: 3 cells
/// - MAX_ROW_LINES: 4 (rows exceeding this switch to vertical key-value format)
/// - ANSI-aware wrapping preserves formatting across line breaks
pub fn render_table(
    headers: &[String],
    rows: &[Vec<String>],
    width: u16,
    theme: &Theme,
) -> Vec<Line<'static>>;
```

## ChatWidget Internals

```rust
pub struct ChatWidget<'a> {
    messages: &'a [ChatMessage],
    scroll_offset: i32,
    streaming_content: Option<&'a str>,
    streaming_thinking: Option<&'a str>,
    show_thinking: bool,
    is_thinking: bool,
    spinner_frame: &'a str,
    thinking_duration: Option<Duration>,
    theme: &'a Theme,
    collapsed_tools: &'a HashSet<String>,
    width: u16,
    user_scrolled: bool,
    streaming_tool_uses: &'a [StreamingToolUse],
    show_system_reminders: bool,
}
```

**Content rendering per message type:**
- User: green indicator, plain text wrapping (no markdown)
- Assistant: cyan indicator, markdown-parsed content
- System/meta: gray, collapsed as `[category] preview...` (toggle with show_system_reminders)
- Tool calls: status icons (⏳ running, ✓ completed, ✗ failed) + elapsed time + description (40 char truncation)
- Thinking: collapsible (▸ 💭 {tokens} tokens), token estimate = word_count * 1.3
- Streaming: blinking cursor ▌, adaptive pacing via StreamDisplay

**Batch grouping:** parallel tool calls marked with ‖ separator and count label

## InputWidget Internals

```rust
pub struct InputState {
    pub text: String,
    pub cursor: i32,                    // char index (0-based, NOT byte)
    pub selection_start: Option<i32>,
    pub history: Vec<HistoryEntry>,
    pub history_index: Option<i32>,
    pub kill_buffer: Option<String>,    // Emacs Ctrl+K/Y
}

pub struct HistoryEntry {
    pub text: String,
    pub frequency: i32,
    pub last_used: i64,
}
```

**Tokenization (5 types for syntax highlighting):**

| Token | Trigger | Style | Validation |
|-------|---------|-------|------------|
| `AtMention` | `@path` | cyan | starts at text-start or after whitespace |
| `AgentMention` | `@agent-*` | red/bold | after whitespace, matches agent prefix |
| `SymbolMention` | `@#symbol` | green/bold | after whitespace |
| `SlashCommand` | `/command` | magenta | at text start only |
| `PastePill` | `[Pasted text #1]` | green/italic | bracket-delimited, max 50 chars |

**Emacs-style editing:**
- `Ctrl+K` → kill to end of line (saved to kill_buffer)
- `Ctrl+Y` → yank kill_buffer at cursor
- `Ctrl+A/E` → cursor to start/end
- `Alt+B/F` → word left/right
- `Alt+Backspace` → delete word backward

**History:** implemented as `Vec<HistoryEntry { text, frequency, last_used_secs }>`
sorted by `HistoryEntry::frecency(now) = ln(freq + 1) * recency_factor`. Recency
factor is `1.0` for entries younger than 24h and decays with a 7-day half-life
afterward. Up arrow walks the most-relevant entry first; Down cycles back and
clears the input on exit. Capped at `MAX_HISTORY_ENTRIES` by dropping the
lowest-scoring tail.

## Streaming Display Pacing

```rust
/// Two-gear adaptive chunking with hysteresis (prevents flapping):
pub struct StreamDisplay {
    display_cursor: usize,     // byte offset into content
    chunking: AdaptiveChunkingPolicy,
    pending_since: Option<Instant>,
}

pub enum ChunkingMode {
    Smooth,   // 1 line per SpinnerTick (typewriter effect)
    CatchUp,  // N lines per tick (batch draining)
}

/// Smooth → CatchUp when:
///   queue_depth >= 8 lines OR oldest_unrevealed_age >= 120ms
/// CatchUp → Smooth when:
///   queue_depth <= 2 AND oldest_age <= 40ms (held for 250ms to debounce)
/// Severe backlog bypass: >= 64 lines or >= 300ms → instant CatchUp
```

## Diff Rendering

```rust
/// Structured diff display with two-column layout (gutter + content).
/// TS uses StructuredPatchHunk from `diff` npm package.
/// Rust can use `similar` crate for unified/structured diffs.
///
/// Gutter: line numbers + change markers (+/-/~)
/// Content: syntax-highlighted with diff colors (added=green, removed=red)
/// Word-level highlighting: diffAddedWord/diffRemovedWord for inline changes
///
/// TS caches rendered diffs in WeakMap (survives React remounts).
/// Rust: no WeakMap needed, render on demand.
pub struct DiffWidget<'a> {
    hunks: &'a [DiffHunk],
    file_path: Option<&'a str>,  // for language detection
    theme: &'a Theme,
}
```

## Theme System (36 colors, 5 themes)

```rust
pub struct Theme {
    // Base
    pub primary: Color,         // @mentions, file paths (cyan family)
    pub secondary: Color,       // parallel tool separator
    pub accent: Color,          // /commands (magenta family)
    pub user_message_bg: Option<Color>,  // terminal-adaptive tint

    // Text
    pub text: Color,            // default foreground (Reset — inherits terminal)
    pub text_dim: Color,        // DarkGray
    pub text_bold: Color,       // White

    // Messages
    pub user_message: Color,    // "▶ You" prefix (Green)
    pub assistant_message: Color, // "◀ Assistant" prefix (Cyan)
    pub thinking: Color,        // thinking blocks (Magenta)
    pub system_message: Color,  // system reminders (DarkGray)

    // Status
    pub tool_running: Color,    // ⏳ (Yellow)
    pub tool_completed: Color,  // ✓ (Green)
    pub tool_error: Color,      // ✗ (Red)
    pub warning: Color,         // permission dialog border (Yellow)
    pub success: Color,         // checkmarks (Green)
    pub error: Color,           // error overlays (Red)

    // UI Elements
    pub border: Color,          // default borders (DarkGray)
    pub border_focused: Color,  // active input (Cyan)
    pub scrollbar: Color,       // scroll indicator
    pub plan_mode: Color,       // plan mode indicator (Blue)

    // Diff
    pub diff_added: Color,      // Green
    pub diff_removed: Color,    // Red
    pub diff_added_word: Color,  // Bold Green (word-level)
    pub diff_removed_word: Color, // Bold Red (word-level)
}

pub enum ThemeName { Default, Dark, Light, Dracula, Nord }
```

**Color discipline:**
- Avoid Blue for text (hard to read on dark terminals)
- Avoid Yellow for backgrounds (invisible on light terminals)
- Never use `.white()` — prefer Reset (inherits terminal foreground)

## Keyboard Shortcuts

| Key | Context | Action |
|-----|---------|--------|
| `Enter` | Chat (empty input) | Submit |
| `Shift+Enter` / `Ctrl+J` | Chat | Insert newline |
| `Tab` | Chat | Toggle plan mode |
| `Shift+Tab` | Chat | Cycle permission mode |
| `Ctrl+T` | Chat | Cycle thinking level |
| `Shift+Ctrl+T` | Chat | Toggle thinking display |
| `Ctrl+M` | Chat | Cycle model (show picker) |
| `Ctrl+B` | Chat | Background all tasks |
| `Ctrl+F` | Chat | Kill all agents |
| `Ctrl+C` | Global | Interrupt |
| `Ctrl+L` | Chat | Clear screen |
| `Ctrl+E` | Chat | Open external editor |
| `Ctrl+G` | Chat | Open plan editor |
| `Ctrl+P` | Chat | Command palette |
| `Ctrl+S` | Chat | Session browser |
| `Ctrl+Q` | Global | Quit |
| `Ctrl+V` / `Alt+V` | Chat | Paste (image + text) |
| `Ctrl+O` | Chat | Copy last agent response to clipboard |
| `Ctrl+Shift+O` | Chat | Quick Open file picker |
| `Ctrl+Shift+E` | Chat | Toggle tool collapse |
| `Ctrl+Shift+R` | Chat | Toggle system reminders |
| `Ctrl+Shift+F` | Chat | Toggle fast mode |
| `Ctrl+,` | Chat | Open Settings overlay |
| `Tab` | Settings | Next tab |
| `Shift+Tab` | Settings | Previous tab |
| `?` / `F1` | Chat | Show help |
| `Esc` | Overlay/Autocomplete | Cancel/dismiss |
| `PageUp/Down` | Chat | Scroll history |
| `Ctrl+K` | Chat | Kill to end of line |
| `Ctrl+Y` | Chat | Yank kill buffer |
| `Ctrl+A` | Chat | Cursor to line start |
| `Alt+B/F` | Chat | Word left/right |
| `Tab` | Autocomplete | Accept suggestion |
| `Up/Down` | Autocomplete | Navigate suggestions |
| `Y/N` | Permission | Approve/Deny |
| `A` | Permission | Approve all (always allow) |

**Context resolution priority** (as implemented in `keybinding_bridge.rs`):

```
Overlay (Confirmation | Picker | Scrollable) > Autocomplete > Global keys > Input keys
```

Autocomplete is a single shared context covering skill / agent / symbol / file
suggestions (they share Tab/Up/Down/Esc handling). Per-type contexts would
only matter if suggestion types had divergent keymaps — they don't today.

## Overlay Priority System

Implemented as `Overlay::priority(&self) -> i32` in `app/tui/src/state/ui.rs`.
Lower number wins.

| Tier | Overlay variants | Meaning |
|------|------------------|---------|
| 0 | `SandboxPermission` | security-critical |
| 1 | `Permission`, `PlanExit`, `PlanEntry` | blocks agent execution |
| 2 | `Question`, `Elicitation`, `McpServerApproval`, `IdleReturn` | awaiting structured input |
| 3 | `CostWarning`, `BypassPermissions`, `WorktreeExit` | high-stakes confirmation |
| 4 | `Error`, `InvalidConfig` | error surface |
| 5 | `Rewind`, `DiffView` | content review |
| 6 | `AutoModeOptIn`, `Trust`, `Bridge`, `McpServerSelect` | settings confirmation |
| 7 | `ModelPicker`, `CommandPalette`, `SessionBrowser`, `GlobalSearch`, `QuickOpen`, `Export`, `Feedback`, `TaskDetail`, `Doctor`, `ContextVisualization` | user-triggered pickers |
| 8 | `Help` | read-only reference |

`UiState::set_overlay` rules:

1. No active overlay → install directly.
2. New overlay has strictly higher priority (lower number) than the current one
   → displace current back into the queue, install the new one.
3. Otherwise → insert into the queue at its priority position. Same priority
   preserves insertion order (stable within a tier).

Queue overflow (past `MAX_OVERLAY_QUEUE`) drops the lowest-priority tail entry
so a security-critical overlay can still enqueue.

## Toast System

```rust
pub struct Toast {
    pub id: String,
    pub message: String,
    pub severity: ToastSeverity,
    pub created_at: Instant,
    pub duration: Duration,
}

pub enum ToastSeverity {
    Info,      // 3s, dim border
    Success,   // 3s, green border
    Warning,   // 5s, yellow border
    Error,     // 8s, red border
}

/// Max 5 active toasts. Oldest dropped on overflow.
/// Auto-expire checked on Tick event (250ms interval).
/// Rendered top-right corner, stacked vertically.
/// remaining_percent() drives progress bar animation.
```

## Clipboard & Mouse

**Mouse**: coco-tui deliberately does **not** call `EnableMouseCapture` in
`terminal.rs`. The terminal keeps ownership of mouse events, so native
drag-to-select + Cmd/Ctrl+C work exactly as they do in `vim` / `less`.
Same choice as codex-rs. `TuiEvent` has no `Mouse` variant; `app.rs` drops
any stray `Event::Mouse` defensively.

**Copy**: the `/copy` slash command and `Ctrl+O` hotkey both dispatch
`TuiCommand::CopyLastMessage`, which calls `clipboard_copy::copy_to_clipboard`
and surfaces a success / info / error toast.

`clipboard_copy.rs` is a direct port of codex-rs (≈350 LoC impl + 12 unit
tests). Selection order:

1. **SSH** (`SSH_TTY` / `SSH_CONNECTION` set) → OSC 52 only. Remote
   X11/Wayland clipboard is useless; OSC 52 tunnels to the local terminal.
2. **Local** → `arboard` native clipboard.
   - Linux: returns a `ClipboardLease` stored on `UiState::clipboard_lease`
     so the X11/Wayland clipboard keeps serving the copied text until the
     TUI exits (dropping the handle erases the selection).
   - macOS: `SuppressStderr` RAII guard redirects fd 2 around
     `arboard::Clipboard::new()` so `NSPasteboard`'s `os_log` chatter
     doesn't corrupt the TUI display. Serialized through a `OnceLock<Mutex>`.
3. **WSL fallback** (only if arboard fails): spawn `powershell.exe`
   `Set-Clipboard -Value` with UTF-8 stdin.
4. **OSC 52 fallback** (last resort, or if WSL path also fails).

Payload cap: **100 KB raw** before base64 encoding for OSC 52 — larger
payloads fail fast to avoid DOSing the terminal. Encoded sequences are
wrapped in tmux DCS passthrough (`\x1bPtmux;\x1b…\x1b\\`) when `$TMUX` is
set, same as the notification backends.

State tracking (`SessionState`):
- `last_agent_markdown: Option<String>` — populated in `on_turn_completed`
  when streaming content is flushed; cleared on `SessionStarted`.
- `record_agent_markdown(text)` — shared entrypoint that skips empty input.

Deliberate divergences from codex-rs:
- `saw_copy_source_this_turn` precedence flag is skipped for v1 (coco-rs
  only has one capture source, so there's nothing to arbitrate).
- Success/error feedback is surfaced as `Toast` instead of
  `history_cell::new_info_event` — coco-rs has no persistent info-cell
  history pattern; toasts are idiomatic.

Keybinding: `Ctrl+O` → copy. `Ctrl+Shift+O` still opens Quick Open (the
previous Ctrl+O binding) — Shift is the power-user escape hatch.

## ratatui 0.30 Migration Notes

Upgraded from ratatui 0.29 → 0.30 in Apr 2026. Workspace crossterm bumped to
0.29 at the same time so `ratatui-crossterm` selects a single `crossterm`
version (avoids duplicate 0.28+0.29 in the dep tree).

Code-side changes:

- `Stylize` is no longer required for inherent `Style` methods — `Style::bold`,
  `Style::italic`, etc. are now direct. Import `Stylize` only when you need
  the trait-based `.red()` / `.bold()` methods on `Span` and `Line`.
- Manual overlay centering (compute `x = (area.width - width) / 2`, etc.)
  replaced with `Rect::centered(Constraint::Length(w), Constraint::Length(h))`.
- Layout splits rewritten from `Layout::default().direction(..).constraints(..).split(area)`
  returning `Rc<[Rect]>` to `area.layout(&Layout::vertical([..]))` returning
  `[Rect; N]` that can be destructured directly. Compiler enforces the slot
  count at compile time.

Still missing: **OSC 8 hyperlinks**. ratatui 0.30 cell buffer still has no
native `Span::hyperlink` primitive and still strips raw escape sequences
embedded in spans. Deferred until upstream support lands.

## Ink → ratatui Migration Guide

| Ink (TS React) | ratatui (Rust) | Notes |
|----------------|----------------|-------|
| `<Box>` (flexbox) | `Layout` + `Constraint` | Yoga layout → CSS-like constraints |
| `<Text>` | `Span` / `Line` / `Paragraph` | `.bold()`, `.fg(color)` via Stylize |
| `<ScrollBox>` | Custom scroll widget + viewport | Manual scroll tracking in UiState |
| `<Button onClick>` | Key handler in overlay match | No mouse buttons; keyboard-driven |
| `<Link>` | OSC 8 escape sequence in Span | `\x1b]8;;url\x1b\\text\x1b]8;;\x1b\\` |
| `useInput(cb)` | `TuiEvent::Key → keybinding_bridge` | Context-aware dispatch |
| `useState/useEffect` | `&mut AppState` + `handle_command()` | No GC, no hooks lifecycle |
| `React.Context` | `&AppState` passed to render | Direct reference, no provider tree |
| `useSyncExternalStore` | `tokio::sync::watch` | For async state observation |
| `setTimeout` | `tokio::time::interval` | Tick (250ms) + SpinnerTick (50ms) |
| Virtual DOM diffing | Immediate-mode `Buffer` diffing | ratatui diffs buffers automatically |
| `<Newline/>` / `<Spacer/>` | `Constraint::Length(1)` / `Constraint::Min(0)` | Layout primitives |

**Key architectural difference:** React re-renders entire subtree on state change; ratatui re-renders entire frame but diffs the terminal buffer. Both are efficient — React avoids DOM mutations, ratatui avoids ANSI writes. The Rust approach is simpler (no reconciler, no fiber scheduler).

## TS Design System → ratatui Primitives

| TS Component | ratatui Equivalent |
|--------------|--------------------|
| `Dialog.tsx` | `Block::bordered().title(title).border_style(theme.warning)` + `Clear` + centered `Rect` |
| `Pane.tsx` | `Block::bordered()` with optional title |
| `ThemedBox.tsx` | `Block::new().style(Style::default().bg(theme.bg))` |
| `ThemedText.tsx` | `Span::styled(text, Style::default().fg(theme.text))` |
| `ProgressBar.tsx` | `Gauge::default().ratio(pct).gauge_style(theme.success)` |
| `StatusIcon.tsx` | `Span::raw("✓"/"✗"/"⏳").fg(status_color)` |
| `Tabs.tsx` | `Tabs::new(titles).select(active).highlight_style(theme.accent)` |
| `ListItem.tsx` | `ListItem::new(Line::from(...))` |
| `FuzzyPicker.tsx` | Custom widget: `List` + `InputWidget` + fuzzy scoring |
| `LoadingState.tsx` | `Span::raw(spinner_frame).fg(theme.accent)` |
| `Divider.tsx` | `Block::default().borders(Borders::TOP).border_style(theme.border)` |
| `KeyboardShortcutHint.tsx` | `Line::from(vec![key_span.bold(), " ".into(), desc_span.dim()])` |
| `Byline.tsx` | `Paragraph::new(Line::from(...)).alignment(Alignment::Right)` |
| `Ratchet.tsx` | Rotating animation frames via `animation.rs` |

## TS Component → Rust Widget Mapping

| TS Category | Files | LOC | Rust Approach |
|-------------|-------|-----|---------------|
| Message renderers | 41 | 6K | `ChatMessage` enum variants, rendered in `ChatWidget` |
| Permission dialogs | 51 | 12K | `PermissionDetail` enum variants, rendered in `PermissionOverlay` |
| Agent management | 26 | 4.5K | `AgentEditorOverlay` + helper widgets |
| Prompt input | 21 | 5K | `InputWidget` (already exists in cocode-rs) |
| MCP UI | 13 | 4K | `McpSettingsOverlay` + `McpStatusWidget` |
| Tasks UI | 12 | 4K | `BackgroundTasksOverlay` + `ToolPanel` extension |
| Design system | 16 | 2K | ratatui primitives (Block, Paragraph, List, Tabs) |
| Diff/file | 5 | 1.5K | `DiffDisplay` widget (exists in cocode-rs) |
| Ink framework | 96 | 750K | **SKIP** — replaced by ratatui + crossterm |
| Screens | 3 | 6K | `App::run()` handles all screens |
| Settings | 4 | 2.5K | `SettingsOverlay` |
| Notifications | 1 | — | `NotificationBackend` enum |

**Key insight**: TS's 389 component files collapse to ~20 Rust widgets + ~20 overlay variants.
React's component model inflates file count; Rust's enum + match is more compact.

## Snapshot Testing

```rust
// TUI snapshot tests use insta crate with TestBackend
#[test]
fn test_chat_with_messages() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let state = test_state_with_messages();

    terminal.draw(|frame| render(frame, &state)).unwrap();

    // Snapshot captures the rendered buffer
    insta::assert_snapshot!(terminal.backend());
}

// Run: cargo test -p coco-tui
// Review: cargo insta review -p coco-tui
// Accept: cargo insta accept -p coco-tui
```

## Module Layout

```
app/tui/src/
  lib.rs                      # crate root, re-exports
  app.rs                      # TEA event loop (tokio::select!)
  events.rs                   # TuiEvent (low-level) + TuiCommand (high-level) enums
  render.rs                   # main render function (layout hierarchy)
  update.rs                   # TuiCommand → state mutations (top-level dispatch)
  update/
    edit.rs                   # text editing, history, word movement
    overlay.rs                # approve/deny/filter/nav/confirm + filter helpers
    show.rs                   # Show* overlay constructors
    clipboard.rs              # Paste + copy-last integration
  command.rs                  # UserCommand enum (TUI → Core)
  terminal.rs                 # Terminal setup/teardown, panic hook
  theme.rs                    # 5 named themes, color palette
  animation.rs                # Spinner frames, time-based
  constants.rs                # Layout breakpoints, timing constants
  keybinding_bridge.rs        # KeyEvent → TuiCommand via KeybindingsManager
  server_notification_handler/ # CoreEvent → state changes
    mod.rs                      # handle_core_event dispatch
    protocol.rs                 # ServerNotification arms (session/turn/tool/mcp/system/hook/sandbox)
    stream.rs                   # AgentStreamEvent (TextDelta, ThinkingDelta, …)
    tui_only.rs                 # TuiOnlyEvent (ApprovalRequired, QuestionAsked, Elicitation*, Rewind*, …)
  state/
    mod.rs                    # AppState composite + RunningState
    session.rs                # SessionState (agent-synchronized)
    ui.rs                     # UiState + InputState + Streaming + Toast + FocusTarget
    overlay.rs                # Overlay enum + priority() + PermissionDetail + all overlay payload structs
    rewind.rs                 # RewindOverlay + RewindPhase + RewindableMessage
  render_overlays/            # per-category overlay content renderers
    mod.rs                      # render_overlay + overlay_content dispatch
    permission.rs               # permission_content (12 PermissionDetail variants)
    pickers.rs                  # filterable lists (model, command, session, quick open, export, mcp-select)
    search.rs                   # global search (ripgrep streaming)
    help.rs                     # keybinding help lines
    question.rs                 # AskUserQuestion overlay
    diff.rs                     # full-screen diff view
    context_viz.rs              # context window usage bar
    rewind.rs                   # rewind message-select + restore-options
    settings.rs                 # tabbed settings (theme/output style/permissions/about)
    confirm.rs                  # small confirmation dialogs (cost, plan, sandbox, trust, etc.)
  widgets/
    mod.rs                    # widget re-exports
    chat/                       # ChatWidget (message rendering, markdown)
      mod.rs                      # ChatWidget struct + builder + build_lines + render
      render_user.rs              # user variants (text, image, bash, plan, memory, teammate, …)
      render_assistant.rs         # assistant variants (text, thinking, tool_use, advisor)
      render_tool.rs              # tool-result variants (success/error/rejected/diff/write)
      render_system.rs            # system variants (API error, rate limit, shutdown, hook*, plan, …)
    input.rs                  # InputWidget (multi-line, syntax highlighting)
    status_bar.rs             # StatusBar (model, tokens, plan mode)
    header_bar.rs             # HeaderBar (session, cwd, branch)
    subagent_panel.rs         # SubagentPanel (status, focus)
    coordinator_panel.rs      # Team coordinator display
    toast.rs                  # (via lifecycle_banner — auto-expire)
    queue_status_widget.rs    # Queued steering commands
    suggestion_popup.rs       # Shared popup infrastructure
    diff_display.rs           # DiffDisplay (code diff, gutter + content)
    markdown.rs               # Markdown → ratatui Lines (tables, code blocks, lists, …)
    notification.rs           # NotificationBackend (5 terminal types + tmux/screen DCS wrap)
    lifecycle_banner.rs       # shared render_banner_row for single-row banners
    context_warning_banner.rs # Context-usage warning strip
    rate_limit_panel.rs       # Rate-limit status banner
    stream_stall_indicator.rs # Stream stall indicator
    interrupt_banner.rs       # Interrupt in progress
    model_fallback_banner.rs  # Model fallback notice
    permission_mode_banner.rs # Current permission mode strip
    mcp_status_panel.rs       # MCP server status
    hook_status_panel.rs      # Hook execution summary
    local_command_log.rs      # Recent local commands
    history_search.rs         # Frecency history typeahead
    task_list.rs              # Background task list
    team_status.rs            # Teammate status
    teammate_spinner.rs       # Teammate activity indicator
    teammate_view_header.rs   # Teammate view chrome
    ide_dialog.rs             # IDE bridge dialog
    error_dialog.rs           # Error dialog body formatter
    progress_bar.rs           # Shared progress bar primitive
    context_viz.rs            # Context visualization widget
    plugin_manager.rs         # Plugin manager overlay body
    settings_panel.rs         # SettingsPanelState + tabs
  streaming/
    mod.rs                    # StreamingState, display pacing
    chunking.rs               # AdaptiveChunkingPolicy (Smooth/CatchUp hysteresis)
  autocomplete/
    mod.rs                    # SuggestionState (shared)
    file_search.rs            # Async fuzzy file search
    skill_search.rs           # Skill catalog lookup
    agent_search.rs           # Agent definition search
    symbol_search.rs          # LSP symbol search
  paste.rs                    # Paste handling (text + image)
  clipboard.rs                # Clipboard read helpers
  clipboard_copy.rs           # Clipboard write (arboard + OSC 52 + WSL fallback)
  i18n/
    mod.rs                    # Locale detection (COCO_LANG → LANG → LC_ALL)
  locales/
    en.yaml                   # English strings (495 keys)
    zh-CN.yaml                # Simplified Chinese strings (495 keys)
```

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| Enum variants over separate widget structs | TS needs 41 message component files; Rust's enum + match in ChatWidget is ~500 LOC total |
| Overlay queue with priority | Agent-driven overlays queue; user-triggered displace. Prevents dialog storms |
| 4 parallel autocomplete systems | Each has independent debounce, cancel, and mpsc channel. No interference |
| Streaming pacing separate from chat | StreamingState tracks display cursor; committed to ChatMessage on turn end |
| Theme as struct, not trait | 5 named themes are compile-time; no need for dynamic dispatch |
| i18n via `t!()` macro | All user-facing strings in YAML. Compile-time key validation |
| No Ink framework port | ratatui + crossterm replaces React-based Ink entirely. Zero overlap needed |
