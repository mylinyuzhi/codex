//! TUI runner - integrates cocode-tui with the CLI.
//!
//! This module provides the bridge between the CLI and the TUI,
//! setting up channels and running the TUI event loop.

use std::fs::OpenOptions;
use std::path::Path;
use std::path::PathBuf;

use std::sync::Arc;

use std::sync::Mutex;

use cocode_config::Config;
use cocode_config::ConfigManager;
use cocode_config::ConfigOverrides;
use cocode_loop::StopReason;
use cocode_otel::otel_provider::OtelProvider;
use cocode_protocol::LoopError;
use cocode_protocol::LoopEvent;
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
    cli_agents: Vec<cocode_subagent::AgentDefinition>,
) -> anyhow::Result<()> {
    info!("Starting TUI mode");

    // Get working directory
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Build initial Config snapshot
    let snapshot = Arc::new(config.build_config(ConfigOverrides::default().with_cwd(cwd.clone()))?);

    // Initialize file logging for TUI mode (needs snapshot for OTel config)
    let _logging_state = init_tui_logging(config, &snapshot, verbose);

    // Build initial RoleSelections for ALL configured roles
    let initial_selections = config.build_all_selections();

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
        initial_selections,
        title,
        cwd,
        system_prompt_suffix,
        cli_agents,
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
#[allow(clippy::too_many_arguments)]
async fn run_agent_driver(
    mut command_rx: mpsc::Receiver<UserCommand>,
    event_tx: mpsc::Sender<LoopEvent>,
    snapshot: Arc<Config>,
    config: ConfigManager,
    initial_selections: cocode_protocol::RoleSelections,
    title: Option<String>,
    working_dir: PathBuf,
    system_prompt_suffix: Option<String>,
    cli_agents: Vec<cocode_subagent::AgentDefinition>,
) {
    info!("Agent driver started");

    // Create session with all configured role selections
    let mut session = Session::with_selections(working_dir.clone(), initial_selections);
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

    // Register CLI-provided agent definitions
    if !cli_agents.is_empty() {
        let mut mgr = state.subagent_manager().lock().await;
        for agent in cli_agents {
            info!(agent_type = %agent.agent_type, "Registering CLI agent");
            mgr.register_agent_type(agent);
        }
    }

    // Emit plugin agent definitions to TUI for autocomplete.
    // We send all agent definitions (builtin + plugin) so the TUI has
    // the complete set after plugins are loaded.
    {
        let manager = state.subagent_manager().lock().await;
        let agents: Vec<_> = manager
            .definitions()
            .iter()
            .map(|d| cocode_protocol::PluginAgentInfo {
                name: d.name.clone(),
                agent_type: d.agent_type.clone(),
                description: d.description.clone(),
            })
            .collect();
        if !agents.is_empty() {
            let _ = event_tx
                .send(LoopEvent::PluginAgentsLoaded { agents })
                .await;
        }
    }

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
                    .filter_map(|block| match block {
                        cocode_tui::UserContentPart::Text(tp) => Some(tp.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");

                info!(
                    input_len = message.len(),
                    display_len = display_text.len(),
                    content_blocks = content.len(),
                    correlation_id = ?correlation_id.as_ref().map(cocode_protocol::SubmissionId::as_str),
                    "Processing user input"
                );

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
                let mut pending_plan_exit_option = None;
                let mut pending_feedback = None;
                let result = run_turn_concurrent(
                    &mut state,
                    &message,
                    &event_tx,
                    &turn_id,
                    &mut command_rx,
                    &mut deferred_commands,
                    &mut should_shutdown,
                    &mut pending_plan_exit_option,
                    &mut pending_feedback,
                )
                .await;

                emit_turn_result(&event_tx, &turn_id, result, "turn_error").await;

                // Handle plan exit option (clear context / mode change)
                if let Some(exit_option) = pending_plan_exit_option.take() {
                    let plan_prompt =
                        handle_plan_exit_option(&exit_option, &mut state, &event_tx, &config).await;

                    // Auto-submit plan as the first message in the fresh conversation.
                    // This matches CC's behavior: after clear context, the plan is
                    // injected as a user message so the LLM starts implementing.
                    if let Some(prompt) = plan_prompt {
                        turn_counter += 1;
                        let plan_turn_id = format!("turn-{turn_counter}");
                        info!(
                            turn_id = %plan_turn_id,
                            prompt_len = prompt.len(),
                            "Auto-submitting plan as initial message after clear context"
                        );

                        let _ = event_tx
                            .send(LoopEvent::TurnStarted {
                                turn_id: plan_turn_id.clone(),
                                turn_number: turn_counter,
                            })
                            .await;
                        let _ = event_tx.send(LoopEvent::StreamRequestStart).await;

                        let mut plan_exit = None;
                        let mut plan_feedback = None;
                        let plan_result = run_turn_concurrent(
                            &mut state,
                            &prompt,
                            &event_tx,
                            &plan_turn_id,
                            &mut command_rx,
                            &mut deferred_commands,
                            &mut should_shutdown,
                            &mut plan_exit,
                            &mut plan_feedback,
                        )
                        .await;

                        emit_turn_result(&event_tx, &plan_turn_id, plan_result, "plan_turn_error")
                            .await;
                    }
                }

                // Handle "keep planning" feedback: auto-submit as a new turn
                // so the LLM sees the feedback alongside the ExitPlanMode denial.
                if let Some(feedback) = pending_feedback.take() {
                    turn_counter += 1;
                    let feedback_turn_id = format!("turn-{turn_counter}");
                    info!(
                        turn_id = %feedback_turn_id,
                        feedback_len = feedback.len(),
                        "Auto-submitting plan feedback as new turn"
                    );

                    let _ = event_tx
                        .send(LoopEvent::TurnStarted {
                            turn_id: feedback_turn_id.clone(),
                            turn_number: turn_counter,
                        })
                        .await;
                    let _ = event_tx.send(LoopEvent::StreamRequestStart).await;

                    let mut fb_plan_exit = None;
                    let mut fb_feedback = None;
                    let fb_result = run_turn_concurrent(
                        &mut state,
                        &feedback,
                        &event_tx,
                        &feedback_turn_id,
                        &mut command_rx,
                        &mut deferred_commands,
                        &mut should_shutdown,
                        &mut fb_plan_exit,
                        &mut fb_feedback,
                    )
                    .await;

                    emit_turn_result(
                        &event_tx,
                        &feedback_turn_id,
                        fb_result,
                        "feedback_turn_error",
                    )
                    .await;
                }

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

                let slash_input = if args.is_empty() {
                    format!("/{name}")
                } else {
                    format!("/{name} {args}")
                };

                let skill_result = cocode_skill::execute_skill(state.skill_manager(), &slash_input);

                // Handle fork context — spawn subagent instead of inline
                if let Some(ref result) = skill_result
                    && result.context == cocode_skill::SkillContext::Fork
                {
                    let agent_type = result
                        .agent
                        .clone()
                        .unwrap_or_else(|| "general".to_string());
                    info!(
                        skill = %result.skill_name,
                        agent_type,
                        "Spawning subagent for fork context skill"
                    );

                    match state
                        .spawn_subagent_for_skill(
                            &agent_type,
                            &result.prompt,
                            result.model.as_deref(),
                            result.allowed_tools.clone(),
                        )
                        .await
                    {
                        Ok(spawn_result) => {
                            let output = spawn_result.output.unwrap_or_default();
                            emit_text_response(&event_tx, &output).await;
                            continue;
                        }
                        Err(e) => {
                            warn!(
                                skill = %result.skill_name,
                                error = %e,
                                "Fork spawn failed; falling back to inline execution"
                            );
                            // Fall through to inline execution
                        }
                    }
                }

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
                let mut _skill_plan_exit = None;
                let mut _skill_feedback = None;
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
                        &mut _skill_plan_exit,
                        &mut _skill_feedback,
                    )
                    .await
                    .map(|usage| cocode_session::TurnResult {
                        usage,
                        final_text: String::new(),
                        turns_completed: 1,
                        has_pending_tools: false,
                        is_complete: true,
                        stop_reason: StopReason::ModelStopSignal,
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
#[allow(clippy::too_many_arguments)]
async fn run_turn_concurrent(
    state: &mut cocode_session::SessionState,
    input: &str,
    event_tx: &mpsc::Sender<LoopEvent>,
    turn_id: &str,
    command_rx: &mut mpsc::Receiver<UserCommand>,
    deferred: &mut Vec<UserCommand>,
    should_shutdown: &mut bool,
    pending_plan_exit_option: &mut Option<cocode_protocol::PlanExitOption>,
    pending_feedback: &mut Option<String>,
) -> Result<TokenUsage, cocode_error::BoxedError> {
    // Extract shared handles BEFORE borrowing state for the turn.
    // These are cheap clones (CancellationToken and Arc).
    let cancel_token = state.cancel_token();
    let shared_queue = state.shared_queued_commands();
    let question_responder = state.question_responder();

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
                    pending_plan_exit_option,
                    pending_feedback,
                    &question_responder,
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
) -> Result<cocode_session::TurnResult, cocode_error::BoxedError> {
    let cancel_token = state.cancel_token();
    let shared_queue = state.shared_queued_commands();
    let question_responder = state.question_responder();
    let mut dummy_exit_option = None;
    let mut dummy_feedback = None;

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
                    &mut dummy_exit_option,
                    &mut dummy_feedback,
                    &question_responder,
                ).await;
            }
        }
    }
}

/// Handle a command that arrives while a turn is in progress.
///
/// **Interrupt** and **QueueCommand** take effect immediately.
/// Everything else is pushed to `deferred` for processing after the turn.
#[allow(clippy::too_many_arguments)]
async fn handle_in_flight_command(
    command: UserCommand,
    cancel_token: &CancellationToken,
    shared_queue: &Arc<Mutex<Vec<QueuedCommandInfo>>>,
    event_tx: &mpsc::Sender<LoopEvent>,
    deferred: &mut Vec<UserCommand>,
    should_shutdown: &mut bool,
    pending_plan_exit_option: &mut Option<cocode_protocol::PlanExitOption>,
    pending_feedback: &mut Option<String>,
    question_responder: &cocode_session::QuestionResponder,
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
            let count = shared_queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len();
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
            shared_queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
            info!("Cleared all queued commands (during turn)");
            let _ = event_tx
                .send(LoopEvent::QueueStateChanged { queued: 0 })
                .await;
        }
        UserCommand::ApprovalResponse {
            request_id,
            decision,
            plan_exit_option,
            feedback,
        } => {
            // Approval responses must be forwarded immediately — the tool
            // executor is blocking on them.
            info!(request_id, decision = ?decision, ?plan_exit_option, has_feedback = feedback.is_some(), "Approval response (during turn)");
            // Store the plan exit option for post-turn processing
            if plan_exit_option.is_some() {
                *pending_plan_exit_option = plan_exit_option;
            }
            // Store feedback for post-turn injection as a user message
            if let Some(text) = feedback
                && !text.trim().is_empty()
            {
                *pending_feedback = Some(text);
            }
            let _ = event_tx
                .send(LoopEvent::ApprovalResponse {
                    request_id,
                    decision,
                })
                .await;
        }
        UserCommand::QuestionResponse {
            request_id,
            answers,
        } => {
            // Forward to the question responder to unblock the AskUserQuestion tool.
            let delivered = question_responder.respond(&request_id, answers);
            info!(request_id, delivered, "Question response (during turn)");
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
    shared_queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(cmd);
    id
}

/// Handle a command when no turn is running (idle state).
async fn handle_idle_command(
    command: UserCommand,
    state: &mut cocode_session::SessionState,
    event_tx: &mpsc::Sender<LoopEvent>,
    config: &ConfigManager,
    working_dir: &Path,
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
                    .send(LoopEvent::PlanModeEntered { plan_file: None })
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

            // Preserve non-main roles from current session
            let mut new_selections = state.session.selections.clone();
            new_selections.set(ModelRole::Main, selection);
            let new_session = Session::with_selections(working_dir.to_path_buf(), new_selections);

            let new_snapshot = match config
                .build_config(ConfigOverrides::default().with_cwd(working_dir.to_path_buf()))
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
            ..
        } => {
            info!(request_id, decision = ?decision, "Approval response received (idle)");
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
            persist_output_style(style.as_deref()).await;
            state.set_output_style(style);
        }
        UserCommand::RequestOutputStyles => {
            info!("Output styles requested for picker");
            let cocode_home = state.cocode_home().to_path_buf();
            let project_dir = state.project_dir().to_path_buf();
            let mut styles: Vec<cocode_protocol::OutputStyleItem> =
                cocode_config::builtin::load_all_output_styles(&cocode_home, Some(&project_dir))
                    .into_iter()
                    .map(|s| cocode_protocol::OutputStyleItem {
                        name: s.name,
                        source: s.source.label().to_string(),
                        description: s.description,
                    })
                    .collect();
            // Include plugin-contributed styles
            for (name, _prompt) in state.plugin_output_styles() {
                styles.push(cocode_protocol::OutputStyleItem {
                    name: name.clone(),
                    source: "plugin".to_string(),
                    description: None,
                });
            }
            let _ = event_tx.send(LoopEvent::OutputStylesReady { styles }).await;
        }
        UserCommand::RequestPluginData => {
            info!("Plugin data requested");
            let (installed, marketplaces) = state.plugin_summaries();
            let _ = event_tx
                .send(LoopEvent::PluginDataReady {
                    installed,
                    marketplaces,
                })
                .await;
        }
        UserCommand::Rewind => {
            info!("Rewind requested (legacy — rewinding last turn)");
            execute_rewind(
                state,
                event_tx,
                None,
                cocode_protocol::RewindMode::CodeAndConversation,
            )
            .await;
        }
        UserCommand::RequestRewindCheckpoints => {
            info!("Rewind checkpoints requested");
            build_checkpoint_items(state, event_tx).await;
        }
        UserCommand::RewindToTurn { turn_number, mode } => {
            info!(turn_number, mode = ?mode, "Rewind to turn requested");
            execute_rewind(state, event_tx, Some(turn_number), mode).await;
        }
        UserCommand::SummarizeFromTurn {
            turn_number,
            context,
        } => {
            info!(turn_number, ?context, "Summarize from turn requested");
            execute_summarize_from_turn(state, event_tx, turn_number, context.as_deref()).await;
        }
        UserCommand::RequestDiffStats { turn_number } => {
            compute_diff_stats_for_turn(state, event_tx, turn_number).await;
        }
        UserCommand::SetPermissionMode { mode } => {
            info!(?mode, "Permission mode changed");
            let was_plan =
                state.loop_config().permission_mode == cocode_protocol::PermissionMode::Plan;
            state.set_permission_mode(mode);
            let is_plan = mode == cocode_protocol::PermissionMode::Plan;
            if is_plan && !was_plan {
                let _ = event_tx
                    .send(LoopEvent::PlanModeEntered { plan_file: None })
                    .await;
            } else if !is_plan && was_plan {
                let _ = event_tx
                    .send(LoopEvent::PlanModeExited { approved: false })
                    .await;
            }
            let _ = event_tx
                .send(LoopEvent::PermissionModeChanged { mode })
                .await;
        }
        UserCommand::QuestionResponse {
            request_id,
            answers,
        } => {
            // Forward to the question responder. This is typically a no-op
            // during idle because questions are answered during turns, but
            // handles edge cases gracefully.
            let delivered = state.question_responder().respond(&request_id, answers);
            if !delivered {
                warn!(
                    request_id,
                    "Question response received (idle — no pending question)"
                );
            }
        }
        UserCommand::ElicitationResponse {
            request_id, action, ..
        } => {
            // Elicitation responses are forwarded to the MCP client layer.
            // During idle, there is typically no pending elicitation, so
            // just log and discard.
            info!(
                request_id,
                action, "Elicitation response received (idle — no pending request)"
            );
        }
        // Turn-triggering commands (SubmitInput, ExecuteSkill) should not
        // arrive here when called from process_deferred, but handle gracefully.
        UserCommand::SubmitInput { .. } | UserCommand::ExecuteSkill { .. } => {
            warn!("Turn-triggering command received in idle handler — ignoring");
        }
    }
}

/// Build checkpoint items from snapshot manager and message history.
///
/// Used by both `RequestRewindCheckpoints` and `/rewind` local command.
/// Diff stats are NOT computed eagerly — they are fetched on-demand via
/// `RequestDiffStats` when the user selects a checkpoint in the overlay.
async fn build_checkpoint_items(
    state: &cocode_session::SessionState,
    event_tx: &mpsc::Sender<LoopEvent>,
) -> bool {
    let Some(sm) = state.snapshot_manager().cloned() else {
        let _ = event_tx
            .send(LoopEvent::RewindFailed {
                error: "Rewind is not available (no snapshot manager)".to_string(),
            })
            .await;
        return false;
    };
    let infos = sm.list_checkpoints().await;

    // Telemetry: rewind selector opened
    if let Some(otel) = state.otel_manager() {
        otel.counter(
            "cocode.rewind.selector_opened",
            1,
            &[("checkpoint_count", &infos.len().to_string())],
        );
    }

    let checkpoints: Vec<cocode_protocol::RewindCheckpointItem> = infos
        .into_iter()
        .map(|cp| {
            let preview = state
                .message_history
                .turns()
                .iter()
                .find(|t| t.number == cp.turn_number)
                .map(|t| {
                    let text = t.user_message.text();
                    // Truncate by char count (not bytes) to avoid panics on non-ASCII
                    if text.chars().count() > 80 {
                        let end: String = text.chars().take(80).collect();
                        format!("{end}...")
                    } else {
                        text
                    }
                })
                .unwrap_or_default();

            cocode_protocol::RewindCheckpointItem {
                turn_number: cp.turn_number,
                file_count: cp.file_count,
                user_message_preview: preview,
                has_ghost_commit: cp.has_ghost_commit,
                modified_files: cp
                    .modified_files
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect(),
                diff_stats: None, // Computed lazily via RequestDiffStats
            }
        })
        .collect();

    let _ = event_tx
        .send(LoopEvent::RewindCheckpointsReady { checkpoints })
        .await;
    true
}

/// Compute diff stats for a single checkpoint on-demand and emit the result.
async fn compute_diff_stats_for_turn(
    state: &cocode_session::SessionState,
    event_tx: &mpsc::Sender<LoopEvent>,
    turn_number: i32,
) {
    let Some(sm) = state.snapshot_manager().cloned() else {
        return;
    };
    match sm.dry_run_diff_stats(turn_number).await {
        Ok(stats) => {
            let _ = event_tx
                .send(LoopEvent::DiffStatsReady {
                    turn_number,
                    stats: cocode_protocol::RewindDiffStats {
                        files_changed: stats.files_changed,
                        insertions: stats.insertions,
                        deletions: stats.deletions,
                    },
                })
                .await;
        }
        Err(e) => {
            tracing::debug!(turn_number, "Failed to compute diff stats: {e}");
        }
    }
}

/// Execute a rewind operation with the given target turn and mode.
///
/// File restoration is handled by `SnapshotManager::rewind_to_turn_with_mode`.
/// Conversation state changes (history truncation, todo rebuild, file tracker
/// pruning) are delegated to `SessionState::apply_rewind_mode_for_turn` to
/// avoid duplicating that logic here.
async fn execute_rewind(
    state: &mut cocode_session::SessionState,
    event_tx: &mpsc::Sender<LoopEvent>,
    target_turn: Option<i32>,
    mode: cocode_protocol::RewindMode,
) {
    if let Some(sm) = state.snapshot_manager().cloned() {
        match sm.rewind_to_turn_with_mode(target_turn, mode).await {
            Ok(result) => {
                // Delegate ALL conversation state changes to SessionState.
                // This handles: history truncation, todo rebuild, file tracker
                // pruning, and prompt capture — in a single consistent operation.
                let (messages_removed, restored_prompt) =
                    state.apply_rewind_mode_for_turn(result.rewound_turn, mode);

                // Telemetry: rewind completed
                if let Some(otel) = state.otel_manager() {
                    otel.counter(
                        "cocode.rewind.completed",
                        1,
                        &[
                            ("mode", &format!("{mode:?}")),
                            ("used_git", &result.used_git_restore.to_string()),
                            ("files_restored", &result.restored_files.len().to_string()),
                        ],
                    );
                }

                let _ = event_tx
                    .send(LoopEvent::RewindCompleted {
                        rewound_turn: result.rewound_turn,
                        restored_files: result.restored_files.len() as i32,
                        messages_removed,
                        mode,
                        restored_prompt,
                    })
                    .await;
            }
            Err(e) => {
                // Telemetry: rewind failed
                if let Some(otel) = state.otel_manager() {
                    otel.counter("cocode.rewind.failed", 1, &[]);
                }

                let _ = event_tx
                    .send(LoopEvent::RewindFailed {
                        error: e.to_string(),
                    })
                    .await;
            }
        }
    } else {
        let _ = event_tx
            .send(LoopEvent::RewindFailed {
                error: "Rewind is not available (no snapshot manager)".to_string(),
            })
            .await;
    }
}

/// Execute a summarize (partial compact) from a specific turn.
async fn execute_summarize_from_turn(
    state: &mut cocode_session::SessionState,
    event_tx: &mpsc::Sender<LoopEvent>,
    turn_number: i32,
    context: Option<&str>,
) {
    match state
        .run_partial_compact(turn_number, event_tx.clone(), context)
        .await
    {
        Ok(result) => {
            let _ = event_tx
                .send(LoopEvent::SummarizeCompleted {
                    from_turn: result.from_turn,
                    summary_tokens: result.summary_tokens,
                })
                .await;
        }
        Err(e) => {
            error!("Summarize from turn {turn_number} failed: {e}");
            let _ = event_tx
                .send(LoopEvent::SummarizeFailed {
                    error: e.to_string(),
                })
                .await;
        }
    }
}

/// Handle the plan exit option after a turn completes.
///
/// This implements the clear context flow: when the user selects "Clear context + accept edits"
/// or "Clear context + bypass", this function clears the conversation history, reads the plan
/// content from disk, and returns it as the initial prompt for the next turn.
///
/// Returns `Some(plan_prompt)` when clear context was selected and a plan was found,
/// so the caller can auto-submit it as a new turn. This matches Claude Code's behavior
/// of injecting the plan as the first user message in a fresh conversation.
async fn handle_plan_exit_option(
    exit_option: &cocode_protocol::PlanExitOption,
    state: &mut cocode_session::SessionState,
    event_tx: &mpsc::Sender<LoopEvent>,
    _config: &ConfigManager,
) -> Option<String> {
    let Some(target_mode) = exit_option.target_mode() else {
        // KeepPlanning — nothing to do
        return None;
    };

    if exit_option.should_clear_context() {
        info!(
            ?exit_option,
            ?target_mode,
            "Clearing conversation context after plan exit"
        );

        // Read plan content BEFORE clearing (plan_file_path persists after exit)
        let plan_content = state
            .plan_mode_state()
            .plan_file_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok());

        // Capture transcript path BEFORE clearing context (session ID changes after clear)
        let transcript_path = cocode_session::persistence::session_file_path(state.session_id());

        // Clear context: fires SessionEnd hooks, creates child session
        // (new ID, parent tracking), clears history, resets shell CWD,
        // and fires SessionStart hooks.
        state.clear_context().await;

        // Apply the target permission mode
        state.set_permission_mode(target_mode);

        // Notify TUI of context clear and mode change
        let _ = event_tx
            .send(LoopEvent::ContextCleared {
                new_mode: target_mode,
            })
            .await;
        let _ = event_tx
            .send(LoopEvent::PermissionModeChanged { mode: target_mode })
            .await;

        // Return plan as initial prompt for auto-submission, with transcript reference
        plan_content.map(|plan| {
            format!(
                "Implement the following plan:\n\n{plan}\n\n\
                 If you need specific details from before exiting plan mode \
                 (like exact code snippets, error messages, or content you generated), \
                 read the full transcript at: {}",
                transcript_path.display()
            )
        })
    } else if exit_option.is_approved() {
        // Keep context but change permission mode (KeepAndElevate or KeepAndDefault)
        info!(
            ?exit_option,
            ?target_mode,
            "Changing permission mode after plan exit (keeping context)"
        );

        state.set_permission_mode(target_mode);

        let _ = event_tx
            .send(LoopEvent::PermissionModeChanged { mode: target_mode })
            .await;
        None
    } else {
        None
    }
}

/// Emit the standard turn result events (StreamRequestEnd + TurnCompleted or Error).
async fn emit_turn_result(
    event_tx: &mpsc::Sender<LoopEvent>,
    turn_id: &str,
    result: Result<TokenUsage, cocode_error::BoxedError>,
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
            // 日志：使用内部错误的 Debug（stack_trace_debug）输出虚拟堆栈，避免系统 backtrace 噪音。
            // 这里的 `e` 不再是 anyhow::Error。
            error!(error = ?e, status = ?e.status_code(), "Turn failed");
            let _ = event_tx
                .send(LoopEvent::Error {
                    error: LoopError {
                        code: error_code.to_string(),
                        message: e.output_msg(),
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
    working_dir: &Path,
) {
    let mut dummy_shutdown = false;
    for cmd in deferred.drain(..) {
        handle_idle_command(
            cmd,
            state,
            event_tx,
            config,
            working_dir,
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
        "agents" => {
            let mgr = state.subagent_manager().lock().await;
            let defs = mgr.definitions();
            let mut text = String::new();
            text.push_str(&format!("Available agent types ({}):\n\n", defs.len()));
            for def in defs {
                let source = format!("{:?}", def.source);
                let tools_display = if def.tools.is_empty() {
                    "all".to_string()
                } else {
                    def.tools.join(", ")
                };
                text.push_str(&format!("  {} [{}]\n", def.name, source));
                text.push_str(&format!("    {}\n", def.description));
                text.push_str(&format!("    tools: {tools_display}\n"));
                if def.background {
                    text.push_str("    background: true\n");
                }
                if let Some(ref mem) = def.memory {
                    text.push_str(&format!("    memory: {mem:?}\n"));
                }
                text.push('\n');
            }
            drop(mgr);
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
        "rewind" | "checkpoint" => {
            // Trigger the rewind checkpoint selector by sending checkpoint data
            build_checkpoint_items(state, event_tx).await;
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
    let cocode_home = state.cocode_home().to_path_buf();
    let project_dir = state.project_dir().to_path_buf();

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
            let mut styles =
                cocode_config::builtin::load_all_output_styles(&cocode_home, Some(&project_dir));

            // Include plugin-contributed styles
            for (name, _prompt) in state.plugin_output_styles() {
                styles.push(cocode_config::builtin::OutputStyleInfo {
                    name: name.clone(),
                    description: None,
                    content: String::new(),
                    source: cocode_config::builtin::OutputStyleSource::Plugin,
                    keep_coding_instructions: false,
                });
            }

            if styles.is_empty() {
                emit_text_response(event_tx, "No output styles available.").await;
            } else {
                let mut text = format!("Available output styles ({}):\n", styles.len());
                for style in &styles {
                    let desc = style.description.as_deref().unwrap_or("No description");
                    text.push_str(&format!(
                        "  {} [{}] - {}\n",
                        style.name,
                        style.source.label(),
                        desc
                    ));
                }
                emit_text_response(event_tx, &text).await;
            }
        }
        "off" | "none" | "disable" => {
            state.set_output_style(None);
            persist_output_style(None).await;
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
            let found_builtin =
                cocode_config::builtin::find_output_style(name, &cocode_home, Some(&project_dir))
                    .is_some();
            // Also check plugin-contributed styles
            let found_plugin = !found_builtin
                && state
                    .plugin_output_styles()
                    .iter()
                    .any(|(n, _)| n.eq_ignore_ascii_case(name));
            if found_builtin || found_plugin {
                state.set_output_style(Some(name.to_string()));
                persist_output_style(Some(name)).await;
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

/// Persist the output style setting to `settings.local.json`.
///
/// Saves `outputStyle` to `{cocode_home}/settings.local.json` so the
/// preference is remembered across sessions.
async fn persist_output_style(style: Option<&str>) {
    let config_dir = cocode_config::find_cocode_home();
    let settings_path = config_dir.join("settings.local.json");

    // Read existing config or start fresh
    let mut config: serde_json::Value = if settings_path.exists() {
        match tokio::fs::read_to_string(&settings_path).await {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => serde_json::Value::Object(serde_json::Map::new()),
        }
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    };

    // Set or remove the outputStyle key
    if let Some(obj) = config.as_object_mut() {
        match style {
            Some(name) => {
                obj.insert(
                    "outputStyle".to_string(),
                    serde_json::Value::String(name.to_string()),
                );
            }
            None => {
                obj.remove("outputStyle");
            }
        }
    }

    // Write back
    if let Some(parent) = settings_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(content) = serde_json::to_string_pretty(&config) {
        let _ = tokio::fs::write(&settings_path, content).await;
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
) -> Result<TokenUsage, cocode_error::BoxedError> {
    let result = state.run_turn_streaming(input, event_tx.clone()).await?;
    Ok(result.usage)
}
