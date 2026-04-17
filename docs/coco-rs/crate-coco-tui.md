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

### TuiCommand Enum (40+ variants)

```rust
pub enum TuiCommand {
    // Mode
    TogglePlanMode, CycleThinkingLevel, CycleModel, ShowModelPicker,
    CyclePermissionMode, ToggleFastMode,
    // Input
    SubmitInput, Interrupt, Cancel, ClearScreen,
    InsertChar(char), InsertNewline, DeleteBackward, DeleteWordBackward,
    CursorLeft, CursorRight, CursorUp, CursorDown,
    // Navigation
    ScrollUp, ScrollDown, PageUp, PageDown, FocusNext, FocusPrevious,
    // Suggestions (×4 types)
    SelectNextSuggestion, AcceptSuggestion, DismissSuggestions,
    // Overlays
    Approve, Deny, ApproveAll,
    // Commands
    ExecuteSkill(String), ShowHelp, ShowCommandPalette, ShowSessionBrowser,
    BackgroundAllTasks, KillAllAgents,
    OpenExternalEditor, OpenPlanEditor,
    ToggleToolCollapse, ToggleSystemReminders,
    Quit,
}
```

### UserCommand Enum (TUI → Core)

```rust
pub enum UserCommand {
    SubmitInput { content: String, display_text: Option<String> },
    Interrupt,
    SetPlanMode { active: bool },
    SetPermissionMode { mode: PermissionMode },
    SetThinkingLevel { level: ThinkingLevel },
    SetModel { selection: RoleSelection },
    ApprovalResponse { request_id: String, decision: PermissionDecision, feedback: Option<String> },
    QuestionResponse { request_id: String, answers: Vec<String> },
    ElicitationResponse { request_id: String, action: ElicitAction, content: Option<Value> },
    ExecuteSkill { name: String, args: Option<String> },
    QueueCommand { prompt: String },
    // ...
}
```

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

    // New (from TS dialog types — add as needed)
    ThemePicker(ThemePickerOverlay),
    AgentEditor(AgentEditorOverlay),
    McpSettings(McpSettingsOverlay),
    BackgroundTasks(BackgroundTasksOverlay),
    Settings(SettingsOverlay),
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

```rust
/// Terminal notification system — detects terminal type and sends native notifications.
pub enum NotificationBackend {
    ITerm2,              // OSC proprietary protocol
    ITerm2WithBell,      // OSC + BEL character
    Kitty,               // Kitty terminal protocol (random ID)
    Ghostty,             // Ghostty terminal protocol
    TerminalBell,        // Simple BEL character (Apple Terminal fallback)
}

/// Auto-detect from $TERM_PROGRAM or config.preferred_notif_channel.
pub fn detect_notification_backend() -> NotificationBackend;
pub fn send_notification(backend: &NotificationBackend, title: &str, body: &str);
```

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

## Autocomplete Systems (4 parallel async)

```rust
/// Each system follows the same debounced pattern:
/// 1. User types trigger character (@, /, @agent-, @#)
/// 2. Query sent to background tokio task (100ms debounce)
/// 3. Results arrive via dedicated mpsc channel
/// 4. SuggestionPopup renders filtered results
/// 5. Tab/Enter accepts, Esc dismisses

pub struct FileSuggestionState { /* fuzzy file search via nucleo */ }
pub struct SkillSuggestionState { /* skill catalog lookup */ }
pub struct AgentSuggestionState { /* agent definition search */ }
pub struct SymbolSuggestionState { /* LSP symbol search */ }
```

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

**History:** frecency scoring = `ln(frequency) * recency_factor` (entries >24h penalized), max 200 entries

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
| `Ctrl+Shift+E` | Chat | Toggle tool collapse |
| `Ctrl+Shift+R` | Chat | Toggle system reminders |
| `Ctrl+Shift+F` | Chat | Toggle fast mode |
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

**Context resolution priority:** Overlay > Skill suggestions > Agent suggestions > Symbol suggestions > File suggestions > Global keys > Input keys

## Overlay Priority System

```rust
/// Lower number = higher priority. Agent-driven overlays queue if a higher-priority
/// overlay is active. User-triggered overlays displace agent overlays.
pub fn overlay_priority(overlay: &Overlay) -> i32 {
    match overlay {
        Overlay::SandboxPermission(_) => 0,  // security-critical
        Overlay::Permission(_) | Overlay::PlanExit(_) => 1,  // blocks execution
        Overlay::Question(_) | Overlay::Elicitation(_) => 2,  // tool needs input
        Overlay::CostWarning(_) => 3,
        Overlay::Error(_) => 4,
        Overlay::RewindSelector(_) => 5,
        Overlay::PluginManager(_) => 6,
        Overlay::ModelPicker(_) | Overlay::OutputStylePicker(_)
            | Overlay::CommandPalette(_) | Overlay::SessionBrowser(_) => 7,
        Overlay::Help => 8,
    }
}
```

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
  render.rs                   # main render function (layout hierarchy)
  update.rs                   # TuiCommand → state mutations
  command.rs                  # UserCommand enum (TUI → Core)
  terminal.rs                 # Terminal setup/teardown, panic hook
  theme.rs                    # 5 named themes, color palette
  animation.rs                # Spinner frames, time-based
  constants.rs                # Layout breakpoints, timing constants
  keybinding_bridge.rs        # KeyEvent → TuiCommand via KeybindingsManager
  tui_event_handler.rs        # TuiEvent → TuiCommand mapping
  stream_event_handler.rs     # StreamEvent → streaming display
  server_notification_handler/    # CoreEvent → state changes (exhaustive match, no TuiNotification bridge)
    mod.rs                      # handle_core_event → dispatch to handle_protocol/handle_stream/handle_tui_only
    session.rs                  # SessionStarted, SessionResult, SessionEnded, SessionStateChanged
    turn.rs                     # TurnStarted, TurnCompleted, TurnFailed, TurnInterrupted, MaxTurnsReached
    tool.rs                     # ToolUseQueued/Completed, ToolProgress, ToolUseSummary, ItemStarted/Updated/Completed
    mcp.rs                      # McpStartupStatus, McpStartupComplete
    system.rs                   # Error, RateLimit, KeepAlive, CostWarning, ContextCompacted/UsageWarning/Cleared/Compaction*
    hook.rs                     # HookExecuted, HookStarted, HookProgress, HookResponse
    sandbox.rs                  # SandboxStateChanged, SandboxViolationsDetected
    stream.rs                   # AgentStreamEvent (TextDelta, ThinkingDelta, ToolUse*, McpToolCall*)
    tui_only.rs                 # TuiOnlyEvent (ApprovalRequired, QuestionAsked, Elicitation*, Rewind*, DiffStats*)
  state/
    mod.rs                    # AppState composite
    session.rs                # SessionState (agent-synchronized)
    ui.rs                     # UiState (local: input, overlay, scroll, toast)
  event/
    mod.rs                    # TuiEvent, TuiCommand enums
    stream.rs                 # TuiEventStream (terminal + tick multiplexing)
    broker.rs                 # EventBroker (pause/resume for external editor)
  widgets/
    mod.rs                    # widget re-exports
    chat.rs                   # ChatWidget (message rendering, markdown)
    input.rs                  # InputWidget (multi-line, syntax highlighting)
    status_bar.rs             # StatusBar (model, tokens, plan mode)
    header_bar.rs             # HeaderBar (session, cwd, branch)
    tool_panel.rs             # ToolPanel (running/completed/failed)
    subagent_panel.rs         # SubagentPanel (status, focus)
    team_panel.rs             # TeamPanel (coordinator)
    toast.rs                  # ToastWidget (auto-expire, severity)
    queued_list.rs            # QueuedListWidget (steering queue)
    suggestion_popup.rs       # Shared popup infrastructure
    file_suggestion.rs        # @path autocomplete
    skill_suggestion.rs       # /command autocomplete
    agent_suggestion.rs       # @agent-* autocomplete
    symbol_suggestion.rs      # @#symbol autocomplete
    diff.rs                   # DiffDisplay (code diff)
    markdown.rs               # Markdown → ratatui Lines
    spinner.rs                # Animated spinner
    permission.rs             # PermissionOverlay rendering (12 tool types)
    notification.rs           # NotificationBackend (5 terminal types)
  streaming/
    mod.rs                    # StreamingState, display pacing
  autocomplete/
    file_search.rs            # Async fuzzy file search
    skill_search.rs           # Skill catalog lookup
    agent_search.rs           # Agent definition search
    symbol_search.rs          # LSP symbol search
  paste.rs                    # Paste handling (text + image)
  clipboard_paste.rs          # Clipboard integration
  editor.rs                   # External editor ($EDITOR)
  i18n/
    en.yaml                   # English strings
    zh-CN.yaml                # Chinese strings
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
