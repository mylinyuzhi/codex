# Application Crates — Combined Plan (`app/`)

TS source: `src/state/`, `src/bootstrap/`, `src/components/`, `src/screens/`, `src/ink/`, `src/outputStyles/`, `src/entrypoints/`, `src/main.tsx`, `src/cli/`, `src/server/`, `src/bridge/`

All in `app/` directory: query, state, session, tui, cli.
`bridge/` is standalone at workspace root (separate entry point).

## Dependencies (app crates)

```
coco-state depends on:
  - coco-types (ThinkingLevel), coco-config (Settings, FastModeState)
  - coco-tool (Tool trait, ToolPermissionContext — Arc<dyn Tool> in McpState)

coco-session depends on:
  - coco-types (Message, SessionId), coco-messages (SessionCostState)
  - coco-state (AppState)

coco-tui depends on:
  - coco-types, coco-config, coco-state (AppState)
  - coco-query (QueryEvent — for streaming updates)
  - coco-commands (CommandRegistry — for command palette)
  - ratatui, crossterm

coco-cli depends on:
  - everything (top-level wiring: creates all registries, injects callbacks, starts runtime)
  - clap (CLI parsing)

coco-bridge depends on:
  - coco-types (Message, Attachment), coco-session (SessionState)
  - tokio-tungstenite (WebSocket transport)

App crates are L5 (top-level wiring). They do NOT have strict
"does NOT depend" constraints — coco-cli depends on everything.
coco-state/coco-session/coco-tui do NOT depend on each other circularly.
```

---

## coco-state

TS source: `src/state/AppState.tsx`, `src/state/AppStateStore.ts`

### Data Definitions

```rust
/// Global application state (Zustand-like: Arc<RwLock<AppState>>)
pub struct AppState {
    // Config
    pub settings: Settings,
    pub main_loop_model: String,
    pub verbose: bool,
    pub thinking_level: Option<ThinkingLevel>,
    pub fast_mode: bool,

    // UI
    pub expanded_view: ExpandedView,  // None, Tasks, Teammates
    pub footer_selection: Option<FooterItem>,
    pub spinner_tip: Option<String>,

    // MCP
    pub mcp: McpState,

    // Plugins
    pub plugins: PluginState,

    // Permissions
    pub permission_context: ToolPermissionContext,

    // Tasks
    pub tasks: HashMap<TaskId, TaskState>,
    pub foregrounded_task_id: Option<TaskId>,

    // Agents
    pub agent_name_registry: HashMap<String, AgentId>,
    pub agent_definitions: Vec<AgentDefinition>,

    // File tracking
    pub file_history: FileHistoryState,
    pub attribution: AttributionState,
    pub todos: HashMap<AgentId, TodoList>,

    // Notifications + elicitation
    pub notifications: NotificationState,
    pub elicitation_queue: Vec<ElicitationRequestEvent>,

    // Session hooks
    pub session_hooks: SessionHooksState,

    // Thinking toggle
    pub thinking_enabled: Option<bool>,
    pub prompt_suggestion_enabled: bool,

    // --- v2 fields (remote, tungsten, voice) ---
    // pub remote_session_url: Option<String>,
    // pub remote_connection_status: RemoteConnectionStatus,
    // pub remote_background_task_count: i32,
    // pub repl_bridge: ReplBridgeState,  // 12 fields
    // pub tungsten: Option<TungstenState>,  // tmux integration
    // pub bagel_active: bool,              // web browser tool
    // pub coordinator_task_index: i32,
    // pub view_selection_mode: ViewSelectionMode,
}

pub struct NotificationState {
    pub current: Option<Notification>,
    pub queue: Vec<Notification>,
}

pub struct McpState {
    pub clients: Vec<McpConnection>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub commands: Vec<Command>,
    pub resources: HashMap<String, Vec<ServerResource>>,
}

pub struct PluginState {
    pub enabled: Vec<LoadedPlugin>,
    pub disabled: Vec<LoadedPlugin>,
    pub commands: Vec<Command>,
    pub errors: Vec<PluginError>,
}
```

### Core Logic

```rust
pub struct AppStateStore {
    state: Arc<RwLock<AppState>>,
    subscribers: Vec<Box<dyn Fn() + Send + Sync>>,
}

impl AppStateStore {
    pub fn new() -> Self;
    pub fn get(&self) -> AppState;  // clone current state
    pub fn set(&self, updater: impl FnOnce(&mut AppState));
    pub fn subscribe(&mut self, listener: impl Fn() + Send + Sync + 'static) -> usize;
}

pub fn get_default_app_state() -> AppState;
```

---

## coco-session

TS source: `src/bootstrap/state.ts`, session management

### Data Definitions

```rust
pub struct Session {
    pub id: SessionId,
    pub created_at: i64,
    pub updated_at: i64,
    pub cwd: PathBuf,
    pub model: String,
    pub title: Option<String>,
}

pub struct SessionState {
    pub session: Session,
    pub messages: Vec<Message>,
    pub cost_state: SessionCostState,
    pub file_state_cache: FileStateCache,
}
```

### Core Logic

```rust
pub struct SessionManager {
    sessions_dir: PathBuf,  // ~/.coco/sessions/
}

impl SessionManager {
    pub fn create(&self, cwd: &Path, model: &str) -> Session;
    pub fn load(&self, id: &SessionId) -> Result<SessionState, SessionError>;
    pub fn save(&self, state: &SessionState);
    pub fn list(&self) -> Vec<Session>;
    pub fn resume(&self, id: &SessionId) -> Result<SessionState, SessionError>;
    pub fn delete(&self, id: &SessionId);
}
```

---

## coco-tui

TS source: `src/components/`, `src/screens/`, `src/ink/`, `src/outputStyles/`

### Architecture: TEA (The Elm Architecture) with ratatui

```rust
pub struct App {
    state: AppState,
    ui_state: UiState,
}

pub struct UiState {
    pub input_text: String,
    pub cursor_pos: usize,
    pub scroll_offset: usize,
    pub mode: UiMode,  // Normal, CommandPalette, PermissionDialog, SessionBrowser
    pub notification: Option<Notification>,
}

pub enum UiEvent {
    Key(KeyEvent),
    QueryEvent(QueryEvent),
    Resize(u16, u16),
    Tick,
}

impl App {
    pub async fn run(&mut self, terminal: &mut Terminal<impl Backend>) -> Result<(), AppError> {
        loop {
            terminal.draw(|f| self.render(f))?;
            match self.handle_event(next_event().await) {
                Action::Continue => {}
                Action::Quit => break,
                Action::Submit(input) => self.submit(input).await?,
            }
        }
    }
}
```

### Key Widgets

- `ChatWidget` — message list with markdown rendering
- `InputWidget` — user input with autocomplete
- `SpinnerWidget` — progress indicator with streaming deltas
- `ToolPanelWidget` — tool execution progress
- `PermissionDialogWidget` — permission approval overlay
- `CommandPaletteWidget` — fuzzy command search
- `SessionBrowserWidget` — session list with search
- `StatusBarWidget` — model, cost, context usage

---

## coco-cli

TS source: `src/main.tsx`, `src/entrypoints/cli.tsx`, `src/cli/`

### Entry Point

```rust
/// Binary: `coco`
/// Modes:
///   coco                     — interactive TUI
///   coco -p "prompt"         — print mode (non-interactive)
///   coco --sdk               — SDK mode (NDJSON structured IO)
///   coco resume <session-id> — resume session
///   coco config              — edit config
///   coco doctor              — diagnostics
fn main() {
    let cli = Cli::parse();  // clap
    match cli.command {
        None => run_tui(cli),
        Some(Command::Config) => run_config(),
        Some(Command::Doctor) => run_doctor(),
        Some(Command::Resume { id }) => run_resume(id),
        // ...
    }
}
```

### Startup Flow

```rust
/// 1. Parse CLI args (clap)
/// 2. Load settings (layered: user -> project -> local -> policy -> cli)
/// 3. Initialize auth (API key, OAuth, Bedrock/Vertex)
/// 4. Initialize OTel (if configured)
/// 5. Load tools + commands + skills + plugins
/// 6. Start MCP servers
/// 7. Enter mode (TUI, print, SDK)
pub async fn initialize(cli: &Cli) -> Result<AppContext, InitError>;
```

### Transport Types (from `src/cli/`)

```rust
pub enum Transport {
    Ndjson,       // NDJSON stdin/stdout (SDK mode)
    Sse,          // Server-sent events (remote streaming)
    WebSocket,    // Bidirectional (remote daemon)
}
```

---

## coco-bridge

TS source: `src/bridge/`

### Architecture

```rust
pub struct Bridge {
    transport: BridgeTransport,  // WebSocket or stdio
    session: SessionState,
}

pub enum BridgeTransport {
    WebSocket { url: String },
    Stdio,
}

impl Bridge {
    /// Handle IDE -> agent messages
    pub async fn handle_inbound(&mut self, msg: BridgeMessage);
    /// Send agent -> IDE messages
    pub async fn send_outbound(&self, msg: BridgeMessage);
}

pub enum BridgeMessage {
    UserInput { text: String, attachments: Vec<Attachment> },
    ToolApproval { tool_use_id: String, approved: bool },
    ModelResponse { content: String },
    ToolProgress { tool_use_id: String, progress: Value },
    SessionState { messages: Vec<Message> },
}
```
