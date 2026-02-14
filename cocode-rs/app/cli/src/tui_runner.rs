//! TUI runner - integrates cocode-tui with the CLI.
//!
//! This module provides the bridge between the CLI and the TUI,
//! setting up channels and running the TUI event loop.

use std::fs::OpenOptions;
use std::path::PathBuf;

use std::sync::Arc;

use std::sync::Mutex;

use cocode_config::Config;
use cocode_config::ConfigManager;
use cocode_config::ConfigOverrides;
use cocode_otel::otel_provider::OtelProvider;
use cocode_protocol::LoopError;
use cocode_protocol::LoopEvent;
use cocode_protocol::RoleSelection;
use cocode_protocol::SubmissionId;
use cocode_protocol::TokenUsage;
use cocode_protocol::model::ModelRole;
use cocode_session::Session;
use cocode_system_reminder::QueuedCommandInfo;
use cocode_tui::App;
use cocode_tui::UserCommand;
use cocode_tui::create_channels;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::Layer;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Logging state returned by init_tui_logging.
/// All fields must be kept alive for the duration of the program.
struct TuiLoggingState {
    _file_guard: WorkerGuard,
    _event_log_guard: Option<WorkerGuard>,
    _otel_provider: Option<OtelProvider>,
}

impl TuiLoggingState {
    /// Flush buffered metrics/traces before process exit.
    fn shutdown(&self) {
        if let Some(provider) = &self._otel_provider {
            provider.shutdown();
        }
    }
}

/// Initialize file logging for TUI mode with OTel integration.
///
/// Logs are written to `~/.cocode/log/cocode-tui.log`.
/// If OTel is configured, adds OTLP log/trace layers.
/// If event_log_file is enabled, adds JSON event log at `~/.cocode/log/otel-events.log`.
fn init_tui_logging(
    config: &ConfigManager,
    snapshot: &Config,
    verbose: bool,
) -> Option<TuiLoggingState> {
    // Get logging config
    let logging_config = config.logging_config();
    let common_logging = logging_config
        .map(|c| c.to_common_logging())
        .unwrap_or_default();

    // Override level if verbose flag is set
    let effective_logging = if verbose {
        cocode_utils_common::LoggingConfig {
            level: "info,cocode=debug".to_string(),
            ..common_logging
        }
    } else {
        common_logging
    };

    // Create log directory
    let log_dir = cocode_config::log_dir();
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("Warning: Could not create log directory {log_dir:?}: {e}");
        return None;
    }

    // Open log file with append mode and restrictive permissions
    let mut log_file_opts = OpenOptions::new();
    log_file_opts.create(true).append(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        log_file_opts.mode(0o600);
    }

    let log_path = log_dir.join("cocode-tui.log");
    let log_file = match log_file_opts.open(&log_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: Could not open log file {log_path:?}: {e}");
            return None;
        }
    };

    // Wrap file in non-blocking writer
    let (non_blocking, file_guard) = tracing_appender::non_blocking(log_file);

    // Build file layer (timezone is handled inside the macro via ConfigurableTimer)
    let file_layer = cocode_utils_common::configure_fmt_layer!(
        fmt::layer().with_writer(non_blocking).with_ansi(false),
        &effective_logging,
        "info"
    );

    // Build OTel provider from config
    let otel_provider = crate::otel_init::build_provider(snapshot);

    // Build optional OTel event log file layer
    let mut event_log_guard = None;
    let event_log_enabled = snapshot.otel.is_some()
        || std::env::var("COCODE_OTEL_EVENT_LOG")
            .ok()
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);

    let event_log_layer = if event_log_enabled {
        let event_log_path = log_dir.join("otel-events.log");
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&event_log_path)
        {
            Ok(file) => {
                let (nb, guard) = tracing_appender::non_blocking(file);
                event_log_guard = Some(guard);
                Some(
                    fmt::layer()
                        .with_writer(nb)
                        .with_ansi(false)
                        .json()
                        .with_filter(tracing_subscriber::filter::filter_fn(
                            OtelProvider::otel_export_filter,
                        )),
                )
            }
            Err(e) => {
                eprintln!("Warning: Could not open OTel event log {event_log_path:?}: {e}");
                None
            }
        }
    } else {
        None
    };

    // Extract OTLP layers from OtelProvider (logger for log export, tracer for trace export)
    let logger_layer = otel_provider.as_ref().and_then(|p| p.logger_layer());
    let tracing_layer = otel_provider.as_ref().and_then(|p| p.tracing_layer());

    // Compose all layers and initialize
    let result = tracing_subscriber::registry()
        .with(file_layer)
        .with(event_log_layer)
        .with(logger_layer)
        .with(tracing_layer)
        .try_init();

    match result {
        Ok(()) => Some(TuiLoggingState {
            _file_guard: file_guard,
            _event_log_guard: event_log_guard,
            _otel_provider: otel_provider,
        }),
        Err(_) => None, // Already initialized
    }
}

/// Run the TUI interface.
///
/// This sets up the TUI with channels for communicating with the agent loop.
pub async fn run_tui(
    title: Option<String>,
    config: &ConfigManager,
    verbose: bool,
    system_prompt_suffix: Option<String>,
) -> anyhow::Result<()> {
    info!("Starting TUI mode");

    // Get working directory
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Build initial Config snapshot
    let snapshot = Arc::new(config.build_config(ConfigOverrides::default().with_cwd(cwd.clone()))?);

    // Initialize file logging for TUI mode (needs snapshot for OTel config)
    let _logging_state = init_tui_logging(config, &snapshot, verbose);

    // Build the initial RoleSelection from ConfigManager (single source of truth)
    let initial_selection = config.current_main_selection();

    // Create channels for TUI-Agent communication
    let (agent_tx, agent_rx, command_tx, command_rx) = create_channels(256);

    // Create and run the TUI with Config snapshot
    let mut app = App::new(agent_rx, command_tx.clone(), snapshot.clone())
        .map_err(|e| anyhow::anyhow!("Failed to create TUI: {e}"))?;

    // Spawn a task to handle user commands and drive the agent
    let agent_handle = tokio::spawn(run_agent_driver(
        command_rx,
        agent_tx,
        snapshot.clone(),
        config.clone(),
        initial_selection,
        title,
        cwd,
        system_prompt_suffix,
    ));

    // Run the TUI (blocks until exit)
    let tui_result = app.run().await;

    // Wait for agent driver to finish
    let _ = agent_handle.await;

    // Flush OTel buffered metrics/traces before exit
    if let Some(ref logging_state) = _logging_state {
        logging_state.shutdown();
    }

    tui_result.map_err(|e| anyhow::anyhow!("TUI error: {e}"))
}

/// Agent driver that handles user commands and sends events to TUI.
///
/// Uses `tokio::select!` so that **Interrupt** and **QueueCommand** are
/// processed immediately — even while a turn is running — instead of being
/// queued behind the sequential `while let` loop.
///
/// ## Concurrency model
///
/// - **cancel_token** (`CancellationToken`) is cloned from `SessionState`
///   before each turn. `Interrupt` calls `.cancel()` directly, which is
///   visible to the running `AgentLoop` at its next yield point.
/// - **queued_commands** (`Arc<Mutex<Vec>>`) is shared between the driver
///   and the `AgentLoop`. Commands pushed here appear at the loop's next
///   Step 6.5 drain — within the *same* turn.
/// - Non-critical commands received during a turn are deferred and
///   replayed after the turn finishes.
async fn run_agent_driver(
    mut command_rx: mpsc::Receiver<UserCommand>,
    event_tx: mpsc::Sender<LoopEvent>,
    snapshot: Arc<Config>,
    config: ConfigManager,
    initial_selection: RoleSelection,
    title: Option<String>,
    working_dir: PathBuf,
    system_prompt_suffix: Option<String>,
) {
    info!("Agent driver started");

    // Create session with model spec
    let mut session = Session::new(working_dir.clone(), initial_selection);
    if let Some(t) = title {
        session.set_title(t);
    }

    // Create session state from config snapshot
    let state_result = cocode_session::SessionState::new(session, snapshot).await;
    let mut state = match state_result {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to create session: {e}");
            let _ = event_tx
                .send(LoopEvent::Error {
                    error: LoopError {
                        code: "session_error".to_string(),
                        message: format!("Failed to create session: {e}"),
                        recoverable: false,
                    },
                })
                .await;
            return;
        }
    };

    // Set system prompt suffix if provided
    if let Some(suffix) = system_prompt_suffix {
        state.set_system_prompt_suffix(suffix);
    }

    let plan_file = working_dir.join(".cocode/plan.md");

    // Track current correlation ID for turn-related events.
    #[allow(unused_assignments)]
    let mut _current_correlation_id: Option<SubmissionId> = None;

    let mut turn_counter = 0;
    let mut deferred_commands: Vec<UserCommand> = Vec::new();
    let mut should_shutdown = false;

    while let Some(command) = command_rx.recv().await {
        if should_shutdown {
            break;
        }

        // Generate correlation ID for commands that trigger turns
        let correlation_id = if command.triggers_turn() {
            let id = SubmissionId::new();
            debug!(correlation_id = %id, "Generated correlation ID for command");
            Some(id)
        } else {
            None
        };

        match command {
            UserCommand::SubmitInput {
                content,
                display_text,
            } => {
                let message: String = content
                    .iter()
                    .filter_map(|block| block.as_text())
                    .collect::<Vec<_>>()
                    .join("");

                info!(
                    input_len = message.len(),
                    display_len = display_text.len(),
                    content_blocks = content.len(),
                    correlation_id = ?correlation_id.as_ref().map(cocode_protocol::SubmissionId::as_str),
                    "Processing user input"
                );

                _current_correlation_id = correlation_id.clone();

                turn_counter += 1;
                let turn_id = format!("turn-{turn_counter}");

                let _ = event_tx
                    .send(LoopEvent::TurnStarted {
                        turn_id: turn_id.clone(),
                        turn_number: turn_counter,
                    })
                    .await;
                let _ = event_tx.send(LoopEvent::StreamRequestStart).await;

                // ── Run turn with concurrent command handling ──
                let result = run_turn_concurrent(
                    &mut state,
                    &message,
                    &event_tx,
                    &turn_id,
                    &mut command_rx,
                    &mut deferred_commands,
                    &mut should_shutdown,
                )
                .await;

                emit_turn_result(&event_tx, &turn_id, result, "turn_error").await;

                // Reset cancel token if the turn was interrupted
                if state.is_cancelled() {
                    state.reset_cancel_token();
                }

                // Replay deferred commands
                process_deferred(
                    &mut deferred_commands,
                    &mut state,
                    &event_tx,
                    &config,
                    &working_dir,
                    &plan_file,
                )
                .await;
            }
            UserCommand::ExecuteSkill { name, args } => {
                info!(
                    name, args,
                    correlation_id = ?correlation_id.as_ref().map(cocode_protocol::SubmissionId::as_str),
                    "Skill execution requested"
                );

                // Handle local commands that need the agent driver
                if cocode_skill::find_local_command(&name).is_some() {
                    handle_local_command_in_driver(&name, &args, &mut state, &event_tx).await;
                    continue;
                }

                _current_correlation_id = correlation_id.clone();

                let slash_input = if args.is_empty() {
                    format!("/{name}")
                } else {
                    format!("/{name} {args}")
                };

                let skill_result = cocode_skill::execute_skill(state.skill_manager(), &slash_input);

                let (message, model_override) = match skill_result {
                    Some(result) => {
                        let prompt = format!(
                            "<command-name>{}</command-name>\n{}",
                            result.skill_name, result.prompt
                        );
                        (prompt, result.model)
                    }
                    None => (slash_input, None),
                };

                turn_counter += 1;
                let turn_id = format!("turn-{turn_counter}");

                let _ = event_tx
                    .send(LoopEvent::TurnStarted {
                        turn_id: turn_id.clone(),
                        turn_number: turn_counter,
                    })
                    .await;
                let _ = event_tx.send(LoopEvent::StreamRequestStart).await;

                // ── Run skill turn with concurrent command handling ──
                let result = if model_override.is_some() {
                    run_skill_turn_concurrent(
                        &mut state,
                        &message,
                        model_override.as_deref(),
                        &event_tx,
                        &mut command_rx,
                        &mut deferred_commands,
                        &mut should_shutdown,
                    )
                    .await
                } else {
                    run_turn_concurrent(
                        &mut state,
                        &message,
                        &event_tx,
                        &turn_id,
                        &mut command_rx,
                        &mut deferred_commands,
                        &mut should_shutdown,
                    )
                    .await
                    .map(|usage| cocode_session::TurnResult {
                        usage,
                        final_text: String::new(),
                        turns_completed: 1,
                        has_pending_tools: false,
                        is_complete: true,
                    })
                };

                let error_code = "skill_error";
                match result {
                    Ok(turn_result) => {
                        let usage = turn_result.usage.clone();
                        let _ = event_tx
                            .send(LoopEvent::StreamRequestEnd {
                                usage: usage.clone(),
                            })
                            .await;
                        let _ = event_tx
                            .send(LoopEvent::TurnCompleted { turn_id, usage })
                            .await;
                    }
                    Err(e) => {
                        error!("Skill execution failed: {e}");
                        let _ = event_tx
                            .send(LoopEvent::Error {
                                error: LoopError {
                                    code: error_code.to_string(),
                                    message: e.to_string(),
                                    recoverable: true,
                                },
                            })
                            .await;
                    }
                }

                if state.is_cancelled() {
                    state.reset_cancel_token();
                }
                process_deferred(
                    &mut deferred_commands,
                    &mut state,
                    &event_tx,
                    &config,
                    &working_dir,
                    &plan_file,
                )
                .await;
            }

            // ── Non-turn commands (handled immediately when idle) ──
            other => {
                handle_idle_command(
                    other,
                    &mut state,
                    &event_tx,
                    &config,
                    &working_dir,
                    &plan_file,
                    &mut should_shutdown,
                )
                .await;
            }
        }

        if should_shutdown {
            break;
        }
    }

    info!("Agent driver stopped");
}

/// Run a turn with concurrent command processing.
///
/// While the turn runs, `Interrupt` cancels the token immediately and
/// `QueueCommand` pushes to the shared queue (visible to the `AgentLoop`
/// at its next Step 6.5 drain). Other commands are deferred.
async fn run_turn_concurrent(
    state: &mut cocode_session::SessionState,
    input: &str,
    event_tx: &mpsc::Sender<LoopEvent>,
    turn_id: &str,
    command_rx: &mut mpsc::Receiver<UserCommand>,
    deferred: &mut Vec<UserCommand>,
    should_shutdown: &mut bool,
) -> anyhow::Result<TokenUsage> {
    // Extract shared handles BEFORE borrowing state for the turn.
    // These are cheap clones (CancellationToken and Arc).
    let cancel_token = state.cancel_token();
    let shared_queue = state.shared_queued_commands();

    // The turn future borrows &mut state for its duration.
    let turn_future = run_turn_with_events(state, input, event_tx, turn_id);
    tokio::pin!(turn_future);

    loop {
        tokio::select! {
            result = &mut turn_future => return result,
            Some(cmd) = command_rx.recv() => {
                handle_in_flight_command(
                    cmd,
                    &cancel_token,
                    &shared_queue,
                    event_tx,
                    deferred,
                    should_shutdown,
                ).await;
            }
        }
    }
}

/// Run a skill turn (with model override) with concurrent command processing.
async fn run_skill_turn_concurrent(
    state: &mut cocode_session::SessionState,
    input: &str,
    model_override: Option<&str>,
    event_tx: &mpsc::Sender<LoopEvent>,
    command_rx: &mut mpsc::Receiver<UserCommand>,
    deferred: &mut Vec<UserCommand>,
    should_shutdown: &mut bool,
) -> anyhow::Result<cocode_session::TurnResult> {
    let cancel_token = state.cancel_token();
    let shared_queue = state.shared_queued_commands();

    let turn_future = state.run_skill_turn_streaming(input, model_override, event_tx.clone());
    tokio::pin!(turn_future);

    loop {
        tokio::select! {
            result = &mut turn_future => return result,
            Some(cmd) = command_rx.recv() => {
                handle_in_flight_command(
                    cmd,
                    &cancel_token,
                    &shared_queue,
                    event_tx,
                    deferred,
                    should_shutdown,
                ).await;
            }
        }
    }
}

/// Handle a command that arrives while a turn is in progress.
///
/// **Interrupt** and **QueueCommand** take effect immediately.
/// Everything else is pushed to `deferred` for processing after the turn.
async fn handle_in_flight_command(
    command: UserCommand,
    cancel_token: &CancellationToken,
    shared_queue: &Arc<Mutex<Vec<QueuedCommandInfo>>>,
    event_tx: &mpsc::Sender<LoopEvent>,
    deferred: &mut Vec<UserCommand>,
    should_shutdown: &mut bool,
) {
    match command {
        UserCommand::Interrupt => {
            info!("Interrupt requested (during turn)");
            cancel_token.cancel();
            let _ = event_tx.send(LoopEvent::Interrupted).await;
        }
        UserCommand::QueueCommand { prompt } => {
            // Push directly to the shared queue — the running AgentLoop
            // will drain it at its next Step 6.5 iteration.
            let id = queue_command_shared(shared_queue, &prompt);
            #[allow(clippy::unwrap_used)]
            let count = shared_queue.lock().unwrap().len();
            info!(
                prompt_len = prompt.len(),
                queued_count = count,
                "Command queued for steering injection (during turn)"
            );
            let preview = if prompt.len() > 30 {
                format!("{}...", &prompt[..30])
            } else {
                prompt.clone()
            };
            let _ = event_tx
                .send(LoopEvent::CommandQueued { id, preview })
                .await;
        }
        UserCommand::ClearQueues => {
            #[allow(clippy::unwrap_used)]
            {
                shared_queue.lock().unwrap().clear();
            }
            info!("Cleared all queued commands (during turn)");
            let _ = event_tx
                .send(LoopEvent::QueueStateChanged { queued: 0 })
                .await;
        }
        UserCommand::ApprovalResponse {
            request_id,
            decision,
        } => {
            // Approval responses must be forwarded immediately — the tool
            // executor is blocking on them.
            info!(request_id, decision = ?decision, "Approval response (during turn)");
            let _ = event_tx
                .send(LoopEvent::ApprovalResponse {
                    request_id,
                    decision,
                })
                .await;
        }
        UserCommand::Shutdown => {
            info!("Shutdown requested (during turn)");
            cancel_token.cancel();
            *should_shutdown = true;
        }
        other => {
            // Defer non-critical commands until the turn completes.
            deferred.push(other);
        }
    }
}

/// Push a command to the shared queue, returning the assigned ID.
#[allow(clippy::unwrap_used)]
fn queue_command_shared(shared_queue: &Arc<Mutex<Vec<QueuedCommandInfo>>>, prompt: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let id = uuid::Uuid::new_v4().to_string();
    let cmd = QueuedCommandInfo {
        id: id.clone(),
        prompt: prompt.to_string(),
        queued_at: now,
    };
    shared_queue.lock().unwrap().push(cmd);
    id
}

/// Handle a command when no turn is running (idle state).
async fn handle_idle_command(
    command: UserCommand,
    state: &mut cocode_session::SessionState,
    event_tx: &mpsc::Sender<LoopEvent>,
    config: &ConfigManager,
    working_dir: &PathBuf,
    plan_file: &PathBuf,
    should_shutdown: &mut bool,
) {
    match command {
        UserCommand::Interrupt => {
            info!("Interrupt requested (idle)");
            // No turn running — just acknowledge.
            let _ = event_tx.send(LoopEvent::Interrupted).await;
        }
        UserCommand::Shutdown => {
            info!("Shutdown requested");
            *should_shutdown = true;
        }
        UserCommand::SetPlanMode { active } => {
            info!(active, "Plan mode changed");
            if active {
                let _ = event_tx
                    .send(LoopEvent::PlanModeEntered {
                        plan_file: plan_file.clone(),
                    })
                    .await;
            } else {
                let _ = event_tx
                    .send(LoopEvent::PlanModeExited { approved: false })
                    .await;
            }
        }
        UserCommand::SetThinkingLevel { level } => {
            info!(?level, "Thinking level changed");
            if let Err(e) = config.switch_thinking_level(ModelRole::Main, level.clone()) {
                warn!(error = %e, "Failed to update thinking level in config");
            }
            state.switch_thinking_level(ModelRole::Main, level);
        }
        UserCommand::SetModel { selection } => {
            let model_display = selection.model.to_string();
            info!(model = %model_display, "Model changed");

            if let Err(e) = config.switch_spec(&selection.model) {
                error!(error = %e, "Failed to switch model");
                let _ = event_tx
                    .send(LoopEvent::Error {
                        error: LoopError {
                            code: "model_switch_error".to_string(),
                            message: format!("Failed to switch model: {e}"),
                            recoverable: true,
                        },
                    })
                    .await;
                return;
            }

            let new_session = Session::new(working_dir.clone(), selection);

            let new_snapshot = match config
                .build_config(ConfigOverrides::default().with_cwd(working_dir.clone()))
            {
                Ok(cfg) => Arc::new(cfg),
                Err(e) => {
                    error!(error = %e, "Failed to build config snapshot after switch");
                    let _ = event_tx
                        .send(LoopEvent::Error {
                            error: LoopError {
                                code: "model_switch_error".to_string(),
                                message: format!("Failed to build config after switch: {e}"),
                                recoverable: true,
                            },
                        })
                        .await;
                    return;
                }
            };

            match cocode_session::SessionState::new(new_session, new_snapshot).await {
                Ok(new_state) => {
                    *state = new_state;
                    info!(model = %model_display, "Model switched successfully");
                }
                Err(e) => {
                    error!(error = %e, "Failed to create session with new model");
                    let _ = event_tx
                        .send(LoopEvent::Error {
                            error: LoopError {
                                code: "model_switch_error".to_string(),
                                message: format!("Failed to create session with new model: {e}"),
                                recoverable: true,
                            },
                        })
                        .await;
                }
            }
        }
        UserCommand::ApprovalResponse {
            request_id,
            decision,
        } => {
            info!(request_id, decision = ?decision, "Approval response received");
            let _ = event_tx
                .send(LoopEvent::ApprovalResponse {
                    request_id,
                    decision,
                })
                .await;
        }
        UserCommand::QueueCommand { prompt } => {
            let id = state.queue_command(&prompt);
            info!(
                prompt_len = prompt.len(),
                queued_count = state.queued_count(),
                "Command queued for steering injection"
            );
            let preview = if prompt.len() > 30 {
                format!("{}...", &prompt[..30])
            } else {
                prompt.clone()
            };
            let _ = event_tx
                .send(LoopEvent::CommandQueued { id, preview })
                .await;
        }
        UserCommand::ClearQueues => {
            state.clear_queued_commands();
            info!("Cleared all queued commands");
            let _ = event_tx
                .send(LoopEvent::QueueStateChanged { queued: 0 })
                .await;
        }
        UserCommand::BackgroundAllTasks => {
            // No-op when idle: the signal was already sent by the TUI.
            info!("BackgroundAllTasks received in idle handler (no active agents)");
        }
        UserCommand::SetOutputStyle { style } => {
            info!(?style, "Output style changed");
            state.set_output_style(style);
        }
        // Turn-triggering commands (SubmitInput, ExecuteSkill) should not
        // arrive here when called from process_deferred, but handle gracefully.
        UserCommand::SubmitInput { .. } | UserCommand::ExecuteSkill { .. } => {
            warn!("Turn-triggering command received in idle handler — ignoring");
        }
    }
}

/// Emit the standard turn result events (StreamRequestEnd + TurnCompleted or Error).
async fn emit_turn_result(
    event_tx: &mpsc::Sender<LoopEvent>,
    turn_id: &str,
    result: anyhow::Result<TokenUsage>,
    error_code: &str,
) {
    match result {
        Ok(usage) => {
            let _ = event_tx
                .send(LoopEvent::StreamRequestEnd {
                    usage: usage.clone(),
                })
                .await;
            let _ = event_tx
                .send(LoopEvent::TurnCompleted {
                    turn_id: turn_id.to_string(),
                    usage,
                })
                .await;
        }
        Err(e) => {
            error!("Turn failed: {e}");
            let _ = event_tx
                .send(LoopEvent::Error {
                    error: LoopError {
                        code: error_code.to_string(),
                        message: e.to_string(),
                        recoverable: true,
                    },
                })
                .await;
        }
    }
}

/// Process commands that were deferred while a turn was running.
async fn process_deferred(
    deferred: &mut Vec<UserCommand>,
    state: &mut cocode_session::SessionState,
    event_tx: &mpsc::Sender<LoopEvent>,
    config: &ConfigManager,
    working_dir: &PathBuf,
    plan_file: &PathBuf,
) {
    let mut dummy_shutdown = false;
    for cmd in deferred.drain(..) {
        handle_idle_command(
            cmd,
            state,
            event_tx,
            config,
            working_dir,
            plan_file,
            &mut dummy_shutdown,
        )
        .await;
    }
}

/// Handle local commands that arrive at the agent driver (e.g., /skills, /todos, /compact).
///
/// These commands need agent-side data (skill manager, todo state) so they can't be
/// handled purely in the TUI update loop.
async fn handle_local_command_in_driver(
    name: &str,
    args: &str,
    state: &mut cocode_session::SessionState,
    event_tx: &mpsc::Sender<LoopEvent>,
) {
    match name {
        "skills" => {
            let manager = state.skill_manager();
            let commands = manager.all_commands();
            let mut text = String::new();
            text.push_str(&format!("Available commands ({}):\n", commands.len()));
            for cmd in &commands {
                text.push_str(&format!(
                    "  /{} [{}] - {}\n",
                    cmd.name, cmd.command_type, cmd.description
                ));
            }
            emit_text_response(event_tx, &text).await;
        }
        "todos" => {
            // Read task list directly from the most recent TodoWrite tool call
            let text = state.current_todos();
            emit_text_response(event_tx, &text).await;
        }
        "compact" => {
            // Trigger compaction by sending as user input
            let prompt = "Please compact the conversation context now.";
            match run_turn_with_events(state, prompt, event_tx, "compact").await {
                Ok(usage) => {
                    let _ = event_tx.send(LoopEvent::StreamRequestEnd { usage }).await;
                }
                Err(e) => {
                    error!("Failed to compact: {e}");
                    emit_text_response(event_tx, &format!("Compaction failed: {e}")).await;
                }
            }
        }
        "output-style" => {
            handle_output_style_command(args, state, event_tx).await;
        }
        _ => {
            // Unknown local command that reached the driver — emit as text
            emit_text_response(event_tx, &format!("/{name} is not supported.")).await;
        }
    }
}

/// Handle the `/output-style` local command.
///
/// Subcommands:
/// - (no args) / "status": Show current output style
/// - "list": List all available styles (built-in + custom)
/// - "off" / "none" / "disable": Disable the output style
/// - `<name>`: Activate the named style
/// - "help": Show usage text
async fn handle_output_style_command(
    args: &str,
    state: &mut cocode_session::SessionState,
    event_tx: &mpsc::Sender<LoopEvent>,
) {
    let arg = args.trim();
    let cocode_home = cocode_config::find_cocode_home();

    match arg {
        "" | "status" => {
            let current = state.current_output_style_name();
            let text = match current {
                Some(name) => format!("Current output style: {name}"),
                None => "No output style active.".to_string(),
            };
            emit_text_response(event_tx, &text).await;
        }
        "list" => {
            let styles = cocode_config::builtin::load_all_output_styles(&cocode_home);
            if styles.is_empty() {
                emit_text_response(event_tx, "No output styles available.").await;
            } else {
                let mut text = format!("Available output styles ({}):\n", styles.len());
                for style in &styles {
                    let source = if style.source.is_builtin() {
                        "built-in"
                    } else {
                        "custom"
                    };
                    let desc = style.description.as_deref().unwrap_or("No description");
                    text.push_str(&format!("  {} [{}] - {}\n", style.name, source, desc));
                }
                emit_text_response(event_tx, &text).await;
            }
        }
        "off" | "none" | "disable" => {
            state.set_output_style(None);
            emit_text_response(event_tx, "Output style disabled.").await;
        }
        "help" => {
            let text = "\
Usage: /output-style [subcommand]

Subcommands:
  (no args)    Show current output style
  status       Show current output style
  list         List all available styles
  <name>       Activate the named style
  off          Disable the output style
  help         Show this help text";
            emit_text_response(event_tx, text).await;
        }
        name => {
            // Try to find and activate the named style
            if cocode_config::builtin::find_output_style(name, &cocode_home).is_some() {
                state.set_output_style(Some(name.to_string()));
                emit_text_response(event_tx, &format!("Output style set to: {name}")).await;
            } else {
                emit_text_response(
                    event_tx,
                    &format!(
                        "Unknown output style: {name}\nUse /output-style list to see available styles."
                    ),
                )
                .await;
            }
        }
    }
}

/// Emit a simple text response as a single-turn cycle (TurnStarted + TextDelta + TurnCompleted).
async fn emit_text_response(event_tx: &mpsc::Sender<LoopEvent>, text: &str) {
    let turn_id = format!("local-{}", uuid::Uuid::new_v4());
    let _ = event_tx
        .send(LoopEvent::TurnStarted {
            turn_id: turn_id.clone(),
            turn_number: 0,
        })
        .await;
    let _ = event_tx
        .send(LoopEvent::TextDelta {
            delta: text.to_string(),
            turn_id: turn_id.clone(),
        })
        .await;
    let _ = event_tx
        .send(LoopEvent::TurnCompleted {
            turn_id,
            usage: TokenUsage::default(),
        })
        .await;
}

async fn run_turn_with_events(
    state: &mut cocode_session::SessionState,
    input: &str,
    event_tx: &mpsc::Sender<LoopEvent>,
    _turn_id: &str,
) -> anyhow::Result<TokenUsage> {
    let result = state.run_turn_streaming(input, event_tx.clone()).await?;
    Ok(result.usage)
}
