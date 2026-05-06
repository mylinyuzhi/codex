//! `TuiHarness` — in-process TUI driver.
//!
//! What this is:
//! - A real `coco_tui::AppState` (the TUI's TEA model)
//! - A real `coco_query::QueryEngine` driven by a `ScriptedModel`
//! - Real `coco_tools` builtin tools (Bash / Read / Write / Edit / Glob)
//! - A real `coco_hooks::HookRegistry` (caller decides whether to install
//!   any hook definitions)
//! - A `Terminal<TestBackend>` that captures the rendered buffer so tests
//!   can assert on what the user would see
//!
//! What this is **not**:
//! - It does not call `coco_tui::App::run`. `App::run` opens a crossterm
//!   `EventStream` that owns stdin in raw mode — incompatible with a test
//!   harness that needs to inject events programmatically. Instead, the
//!   harness runs the same three building blocks `App::run` orchestrates
//!   (`handle_core_event` for engine→TUI, `update::handle_command` /
//!   `keybinding_bridge::map_key` for keystrokes, `render::render` for the
//!   view) but drives them on its own clock. The pipeline under test is
//!   identical; only the I/O edges are stubbed.
//!
//! Lifecycle:
//! 1. `TuiHarness::builder().build()` — wires channels, spawns the agent
//!    driver task, and returns a ready-to-drive harness.
//! 2. `harness.submit("…")` — pushes a user message into AppState and
//!    sends `UserCommand::SubmitInput` on the command channel.
//! 3. `harness.pump_until_idle(timeout)` — drains every `CoreEvent` the
//!    engine emits into AppState until the engine signals
//!    `SessionResult` (or the timeout fires, which surfaces as an error).
//! 4. `harness.render_to_string()` — paints AppState into the
//!    TestBackend buffer and returns the screen as a newline-separated
//!    string for substring assertions.
//! 5. `harness.shutdown()` (drop runs the same path) — closes the
//!    command channel so the driver task exits cleanly.
//!
//! Hermetic by construction: each harness owns a `tempfile::TempDir`
//! that backs the engine's cwd / project_dir, so any tool that writes to
//! disk lands inside `/tmp/coco-tests-tui-<rand>/...` and is cleaned up
//! when the harness drops.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use anyhow::Result;
use anyhow::anyhow;
use coco_hooks::HookDefinition;
use coco_hooks::HookRegistry;
use coco_inference::ApiClient;
use coco_inference::LanguageModel;
use coco_inference::RetryConfig;
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_tool_runtime::ToolRegistry;
use coco_tui::AppState;
use coco_tui::TuiCommand;
use coco_tui::UserCommand;
use coco_tui::keybinding_bridge;
use coco_tui::render;
use coco_tui::server_notification_handler::handle_core_event;
use coco_tui::update::handle_command;
use coco_types::AgentStreamEvent;
use coco_types::CoreEvent;
use coco_types::Features;
use coco_types::PermissionMode;
use coco_types::ServerNotification;
use coco_types::ToolOverrides;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::common;
use crate::tui::scripted_model::ScriptedModel;

/// Configuration knobs for [`TuiHarness::builder`]. Keep these tight
/// to the testable surface — anything bigger belongs in production
/// `QueryEngineConfig` plumbing, not in tests.
pub struct HarnessConfig {
    /// Width of the synthetic terminal in cells. 120×40 mirrors the
    /// default `coco-tui` snapshot size — wide enough that tool blocks
    /// and chat messages don't wrap surprises into the assertions.
    pub width: u16,
    pub height: u16,
    /// Engine permission mode. Tests default to `BypassPermissions` so
    /// file-writing tools execute without an Ask round-trip; flip to
    /// `Default` to assert the permission-prompt overlay appears.
    pub permission_mode: PermissionMode,
    /// Engine ceiling on agent loop turns. 8 is enough for the multi-
    /// turn / tool-chain suites without giving an underspecified
    /// `ScriptedModel` queue room to spin forever.
    pub max_turns: i32,
    /// Hooks installed before the engine is built. The harness owns
    /// the `HookRegistry`; production code paths fire through the
    /// engine's `Option<Arc<HookRegistry>>` field.
    pub hooks: Vec<HookDefinition>,
    /// Pre-scripted assistant replies. Pop-front per `do_generate` /
    /// `do_stream` call. See [`ScriptedModel`] for the shape.
    pub replies: Vec<crate::tui::scripted_model::Reply>,
    /// Pre-minted working directory. When `Some`, the harness uses
    /// this path as the engine's cwd / project_dir instead of minting
    /// a fresh tempdir. Tests that need to bake an absolute path into
    /// a `Reply::tool_call` (e.g. tool_chain's `Write` example) pass
    /// the dir here so they can compute the path *before* the harness
    /// boots. Ownership of cleanup transfers to the harness.
    pub workdir: Option<TempDir>,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            width: 120,
            height: 40,
            permission_mode: PermissionMode::BypassPermissions,
            max_turns: 8,
            hooks: Vec::new(),
            replies: Vec::new(),
            workdir: None,
        }
    }
}

/// Builder for [`TuiHarness`]. Set the canned replies / hooks / mode you
/// need, then `build()`.
pub struct TuiHarnessBuilder {
    cfg: HarnessConfig,
}

impl TuiHarnessBuilder {
    pub fn new() -> Self {
        Self {
            cfg: HarnessConfig::default(),
        }
    }

    pub fn with_replies(
        mut self,
        replies: impl IntoIterator<Item = crate::tui::scripted_model::Reply>,
    ) -> Self {
        self.cfg.replies = replies.into_iter().collect();
        self
    }

    pub fn with_hooks(mut self, hooks: impl IntoIterator<Item = HookDefinition>) -> Self {
        self.cfg.hooks = hooks.into_iter().collect();
        self
    }

    pub fn with_permission_mode(mut self, mode: PermissionMode) -> Self {
        self.cfg.permission_mode = mode;
        self
    }

    pub fn with_max_turns(mut self, max_turns: i32) -> Self {
        self.cfg.max_turns = max_turns;
        self
    }

    pub fn with_size(mut self, width: u16, height: u16) -> Self {
        self.cfg.width = width;
        self.cfg.height = height;
        self
    }

    /// Use a pre-minted tempdir as the engine's cwd. Lets a test compute
    /// an absolute path *before* the harness boots so it can bake that
    /// path into a `Reply::tool_call` input.
    pub fn with_workdir(mut self, dir: TempDir) -> Self {
        self.cfg.workdir = Some(dir);
        self
    }

    pub async fn build(self) -> Result<TuiHarness> {
        TuiHarness::build(self.cfg).await
    }
}

/// In-process TUI under test. See module docs.
pub struct TuiHarness {
    pub state: AppState,
    pub terminal: Terminal<TestBackend>,
    /// Sender into the agent driver task. Tests usually go through
    /// [`Self::submit`] / [`Self::press_key`] rather than touching this
    /// directly.
    pub command_tx: mpsc::Sender<UserCommand>,
    pub event_rx: mpsc::Receiver<CoreEvent>,
    /// Every `CoreEvent` the engine emitted, in order. Populated by
    /// [`Self::pump_until_idle`] as it folds events into AppState.
    pub events: Vec<CoreEvent>,
    /// Working directory for the engine. Owned so any tool-written
    /// files survive long enough for assertions, then cleaned on drop.
    pub workdir: TempDir,
    /// Reference to the ScriptedModel so tests can read `call_count`.
    pub model: Arc<ScriptedModel>,
    driver_task: Option<JoinHandle<()>>,
    cancel: CancellationToken,
}

impl TuiHarness {
    pub fn builder() -> TuiHarnessBuilder {
        TuiHarnessBuilder::new()
    }

    async fn build(cfg: HarnessConfig) -> Result<Self> {
        let workdir = match cfg.workdir {
            Some(dir) => dir,
            None => common::tmpdir::make("coco-tests-tui-")
                .with_context(|| "create cwd tempdir under /tmp")?,
        };
        let workdir_path = workdir.path().to_path_buf();

        // Engine plumbing: ScriptedModel → ApiClient → QueryEngine.
        let model = ScriptedModel::new(cfg.replies);
        let api_client = Arc::new(ApiClient::with_default_fingerprint(
            model.clone() as Arc<dyn LanguageModel>,
            RetryConfig::default(),
        ));

        // Tool registry: the same curated subset the cli/sdk_server live
        // suites use. Keeps the harness focused on agent-loop + TUI
        // behavior; anything wider is covered by `coco-tools` per-tool
        // tests.
        let tool_registry = ToolRegistry::new();
        tool_registry.register(Arc::new(coco_tools::BashTool));
        tool_registry.register(Arc::new(coco_tools::ReadTool));
        tool_registry.register(Arc::new(coco_tools::WriteTool));
        tool_registry.register(Arc::new(coco_tools::EditTool));
        tool_registry.register(Arc::new(coco_tools::GlobTool));
        let tools = Arc::new(tool_registry);

        // Hook registry: install caller-supplied definitions. Empty by
        // default — most tests don't need hooks.
        let hooks = if cfg.hooks.is_empty() {
            None
        } else {
            let mut reg = HookRegistry::new();
            for h in cfg.hooks {
                reg.register(h);
            }
            Some(Arc::new(reg))
        };

        let cancel = CancellationToken::new();
        let engine_cfg = QueryEngineConfig {
            model_id: model.model_id().to_string(),
            permission_mode: cfg.permission_mode,
            bypass_permissions_available: matches!(
                cfg.permission_mode,
                PermissionMode::BypassPermissions
            ),
            context_window: 200_000,
            max_output_tokens: 2_048,
            max_turns: cfg.max_turns,
            max_tokens: None,
            system_prompt: Some("You are a test scripted model.".into()),
            is_non_interactive: true,
            project_dir: Some(workdir_path.clone()),
            cwd_override: Some(workdir_path.clone()),
            features: Arc::new(Features::with_defaults()),
            tool_overrides: Arc::new(ToolOverrides::none()),
            ..QueryEngineConfig::default()
        };
        let engine = Arc::new(QueryEngine::new(
            engine_cfg,
            api_client,
            tools,
            cancel.clone(),
            hooks,
        ));

        // Channels — same shapes `coco_tui::create_channels` produces,
        // sized for the harness rather than a real TUI session.
        let (command_tx, command_rx) = mpsc::channel::<UserCommand>(64);
        let (event_tx, event_rx) = mpsc::channel::<CoreEvent>(512);

        // Spawn the agent driver: a stripped-down version of
        // `app/cli/src/tui_runner.rs::run_agent_driver`. Production
        // dispatcher handles ~15 UserCommand variants for slash commands,
        // permissions, rewind, etc. — none of those code paths are under
        // test here, so the harness only handles `SubmitInput` (the path
        // every conversational test exercises) and `Shutdown` (so drop
        // can join cleanly).
        let driver_task = tokio::spawn(run_test_agent_driver(engine, command_rx, event_tx));

        // Build the TestBackend-backed terminal. AppState starts empty —
        // production fills it via `app.state_mut()` post-`new`; we don't
        // need any of that bootstrapping for these scenarios.
        let backend = TestBackend::new(cfg.width, cfg.height);
        let terminal = Terminal::new(backend).context("build TestBackend terminal")?;
        let mut state = AppState::new();
        state.session.permission_mode = cfg.permission_mode;
        state.session.bypass_permissions_available =
            matches!(cfg.permission_mode, PermissionMode::BypassPermissions);
        state.session.model = "scripted-model".to_string();

        Ok(Self {
            state,
            terminal,
            command_tx,
            event_rx,
            events: Vec::new(),
            workdir,
            model,
            driver_task: Some(driver_task),
            cancel,
        })
    }

    /// Mimic the user typing `text` and pressing Enter. Pushes a
    /// ChatMessage::User into the displayed conversation (matches what
    /// `update::edit::submit` does in production) and sends
    /// `UserCommand::SubmitInput` on the command channel.
    pub async fn submit(&mut self, text: &str) {
        // Set the input buffer so `TuiCommand::SubmitInput` picks it up.
        for c in text.chars() {
            self.state.ui.input.insert_char(c);
        }
        let _ = handle_command(&mut self.state, TuiCommand::SubmitInput, &self.command_tx).await;
    }

    /// Synthesize a key event the way `App::handle_event` would —
    /// crossterm `KeyEvent` → `keybinding_bridge::map_key` →
    /// `update::handle_command`. Returns `true` if the keystroke produced
    /// a state change. Tests use this to verify dispatcher routing
    /// without piping through the real crossterm `EventStream`.
    pub async fn press_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        let key = KeyEvent::new(code, modifiers);
        if let Some(cmd) = keybinding_bridge::map_key(&self.state, key) {
            handle_command(&mut self.state, cmd, &self.command_tx).await
        } else {
            false
        }
    }

    /// Drain every queued event into AppState, returning when the engine
    /// emits a `SessionResult` (turn-loop done) or `timeout` elapses.
    /// `SessionResult` is the canonical "engine is idle" signal; the
    /// production TUI uses it as the cue to re-enable input.
    ///
    /// Returns the `SessionResult.is_error` flag — `false` ⇒ clean run,
    /// `true` ⇒ engine flagged an error in the result envelope.
    pub async fn pump_until_idle(&mut self, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!(
                    "TuiHarness: timed out after {:?} waiting for SessionResult \
                     ({} events drained, last={:?})",
                    timeout,
                    self.events.len(),
                    self.events.last().map(event_summary)
                ));
            }
            let next = match tokio::time::timeout(remaining, self.event_rx.recv()).await {
                Ok(Some(evt)) => evt,
                Ok(None) => {
                    return Err(anyhow!(
                        "TuiHarness: event channel closed before SessionResult"
                    ));
                }
                Err(_) => continue, // outer-loop guard handles deadline
            };
            // Fold into AppState so render_to_string reflects the
            // post-event view. Then stash for assertion-side
            // introspection.
            handle_core_event(&mut self.state, next.clone());
            let is_terminal = matches!(
                &next,
                CoreEvent::Protocol(ServerNotification::SessionResult(_))
            );
            self.events.push(next);
            if is_terminal {
                let is_err = self
                    .events
                    .iter()
                    .rev()
                    .find_map(|e| match e {
                        CoreEvent::Protocol(ServerNotification::SessionResult(p)) => {
                            Some(p.is_error)
                        }
                        _ => None,
                    })
                    .unwrap_or(false);
                return Ok(!is_err);
            }
        }
    }

    /// Render AppState through `coco_tui::render` and return the buffer
    /// as a newline-separated string. Suitable for `assert!(s.contains
    /// (...))` checks. Whitespace at end-of-line is preserved verbatim.
    pub fn render_to_string(&mut self) -> Result<String> {
        let state = &self.state;
        self.terminal
            .draw(|frame| render::render(frame, state))
            .context("render TUI to TestBackend")?;
        let buf = self.terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        Ok(out)
    }

    /// Convenience: every tool-name that started executing during the
    /// session, in order. Mirrors `cli::events::tool_uses_started`.
    pub fn tool_starts(&self) -> Vec<&str> {
        self.events
            .iter()
            .filter_map(|e| match e {
                CoreEvent::Stream(AgentStreamEvent::ToolUseStarted { name, .. }) => {
                    Some(name.as_str())
                }
                _ => None,
            })
            .collect()
    }

    /// Convenience: `(tool_name, is_error)` for every completed tool.
    pub fn tool_completions(&self) -> Vec<(&str, bool)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                CoreEvent::Stream(AgentStreamEvent::ToolUseCompleted {
                    name, is_error, ..
                }) => Some((name.as_str(), *is_error)),
                _ => None,
            })
            .collect()
    }

    /// Path inside the harness workdir — useful for hooks that need
    /// to write a trace file outside the engine's view.
    pub fn workdir(&self) -> PathBuf {
        self.workdir.path().to_path_buf()
    }

    /// Stop the agent driver task and wait for it to exit. `drop` does
    /// the same with a 2s budget; tests that want a deterministic
    /// shutdown order can call this explicitly.
    pub async fn shutdown(mut self) {
        // Closing the command channel makes the driver's `recv()` return
        // `None` — that's the agreed shutdown trigger.
        drop(self.command_tx.clone());
        self.cancel.cancel();
        if let Some(handle) = self.driver_task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
        }
    }
}

impl Drop for TuiHarness {
    fn drop(&mut self) {
        self.cancel.cancel();
        // Best-effort: abort the driver. We can't `await` it from a
        // sync drop, but cancellation + `abort` plus the cancelled
        // channel are enough that the runtime reaps the task on tear
        // down.
        if let Some(handle) = self.driver_task.take() {
            handle.abort();
        }
    }
}

/// Stripped-down agent driver. The production driver
/// (`app/cli/src/tui_runner.rs::run_agent_driver`) handles 15+
/// `UserCommand` variants — slash commands, permission mode changes,
/// rewind, plan-approval, file-history queries, etc. The TUI scenarios
/// in this crate only exercise `SubmitInput` (the conversational path
/// every test uses) and let `Shutdown` / dropped channels close the
/// loop. Anything not covered here either:
///   1. routes through the same `engine.run_with_events` path
///      `SubmitInput` already exercises (e.g. `ExecuteSkill`), or
///   2. needs the full `SessionRuntime` we deliberately don't build
///      (e.g. `Rewind`, `Compact`, `PlanApprovalResponse`).
async fn run_test_agent_driver(
    engine: Arc<QueryEngine>,
    mut command_rx: mpsc::Receiver<UserCommand>,
    event_tx: mpsc::Sender<CoreEvent>,
) {
    while let Some(cmd) = command_rx.recv().await {
        match cmd {
            UserCommand::SubmitInput { content, .. } => {
                if content.is_empty() {
                    continue;
                }
                // `run_with_events` itself emits SessionStarted /
                // TurnStarted / TurnCompleted / SessionResult into the
                // event channel — the harness folds them into AppState.
                let _ = engine.run_with_events(&content, event_tx.clone()).await;
            }
            UserCommand::Shutdown => break,
            // Other commands are intentionally ignored — see fn docs.
            _ => {}
        }
    }
}

/// One-line debug summary used in timeout error messages.
fn event_summary(evt: &CoreEvent) -> String {
    match evt {
        CoreEvent::Protocol(n) => format!("Protocol::{:?}", n.method()),
        CoreEvent::Stream(s) => match s {
            AgentStreamEvent::TextDelta { .. } => "Stream::TextDelta".into(),
            AgentStreamEvent::ThinkingDelta { .. } => "Stream::ThinkingDelta".into(),
            AgentStreamEvent::ToolUseQueued { name, .. } => {
                format!("Stream::ToolUseQueued({name})")
            }
            AgentStreamEvent::ToolUseStarted { name, .. } => {
                format!("Stream::ToolUseStarted({name})")
            }
            AgentStreamEvent::ToolUseCompleted { name, is_error, .. } => {
                format!("Stream::ToolUseCompleted({name}, err={is_error})")
            }
            AgentStreamEvent::McpToolCallBegin { .. } => "Stream::McpToolCallBegin".into(),
            AgentStreamEvent::McpToolCallEnd { .. } => "Stream::McpToolCallEnd".into(),
        },
        CoreEvent::Tui(_) => "Tui".into(),
    }
}
