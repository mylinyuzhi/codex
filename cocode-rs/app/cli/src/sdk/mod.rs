//! SDK mode — non-interactive NDJSON interface for programmatic access.
//!
//! When `--sdk-mode` is passed, the CLI:
//! 1. Reads a `SessionStartRequestParams` from stdin (first JSON line)
//! 2. Maps `LoopEvent` → `ServerNotification` → NDJSON to stdout
//! 3. Reads `ClientRequest` from stdin (subsequent lines)
//! 4. Routes approval/question responses back to the agent loop

mod control;
mod event_mapper;
mod session_builder;
mod stdio;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use cocode_app_server_protocol::RequestUserInputParams;
use cocode_app_server_protocol::ServerNotification;
use cocode_app_server_protocol::ServerRequest;
use cocode_app_server_protocol::SessionStartRequestParams;
use cocode_app_server_protocol::SessionStartedParams;
use cocode_app_server_protocol::TurnFailedParams;
use cocode_app_server_protocol::TurnStartedParams;
use cocode_config::ConfigManager;
use cocode_config::ConfigOverrides;
use cocode_protocol::LoopEvent;
use cocode_session::Session;
use cocode_session::SessionState;
use tokio::sync::mpsc;
use tracing::info;
use tracing::warn;

use cocode_app_server_protocol::MaxTurnsReachedParams;
use cocode_app_server_protocol::SessionEndedParams;
use cocode_app_server_protocol::SessionEndedReason;
use cocode_app_server_protocol::SessionResultParams;

use self::control::SdkPermissionBridge;
use self::event_mapper::EventMapper;
use self::stdio::InboundMessage;
use self::stdio::StdinReader;
use self::stdio::StdoutWriter;

/// Why the SDK turn loop exited.
enum SdkExitReason {
    /// Maximum turn limit reached.
    MaxTurns,
    /// Turn failed with an error.
    Error(String),
    /// User interrupted or stdin closed.
    StdinClosed,
}

/// Aggregated session metrics collected during the turn loop.
struct SessionMetrics {
    total_turns: i32,
    usage: cocode_app_server_protocol::Usage,
}

/// Run the CLI in SDK mode.
///
/// Reads configuration from stdin, runs agent turns, and streams
/// events as NDJSON to stdout.
pub async fn run_sdk_mode(config: &ConfigManager) -> anyhow::Result<()> {
    // Initialize stderr-only logging (stdout is reserved for NDJSON)
    init_sdk_logging(config);

    let mut reader = StdinReader::new();
    let mut writer = StdoutWriter::new();

    // Step 1: Read session start or resume request from stdin
    let (start_params, mut state, hook_bridge) = match reader.read_message().await? {
        InboundMessage::SessionStart(params) => {
            info!(prompt_len = params.prompt.len(), "SDK session start");
            let (state, bridge) = build_session_state(config, &params).await?;
            (*params, state, bridge)
        }
        InboundMessage::SessionResume(params) => {
            info!(session_id = params.session_id, "SDK session resume");
            let state = resume_session(config, &params).await?;
            let prompt = params.prompt.unwrap_or_default();
            let start = cocode_app_server_protocol::SessionStartRequestParams {
                prompt,
                model: None,
                max_turns: None,
                cwd: None,
                system_prompt_suffix: None,
                system_prompt: None,
                permission_mode: None,
                env: None,
                agents: None,
                mcp_servers: None,
                output_format: None,
                sandbox: None,
                thinking: None,
                tools: None,
                permission_rules: None,
                max_budget_cents: None,
                hooks: None,
                disable_builtin_agents: None,
            };
            (start, state, None)
        }
        other => anyhow::bail!(
            "expected session/start or session/resume as first message, got {other:?}"
        ),
    };

    // Emit session/started with available models and commands
    let session_id = state.session.id.clone();
    let model_name = state.model().to_string();
    let skill_commands: Vec<cocode_app_server_protocol::CommandInfo> = state
        .skills()
        .iter()
        .map(|s| cocode_app_server_protocol::CommandInfo {
            name: format!("/{}", s.name),
            description: Some(s.description.clone()),
        })
        .collect();
    writer
        .write_notification(&ServerNotification::SessionStarted(SessionStartedParams {
            session_id: session_id.clone(),
            protocol_version: "1".to_string(),
            models: Some(vec![model_name]),
            commands: if skill_commands.is_empty() {
                None
            } else {
                Some(skill_commands)
            },
        }))
        .await?;

    let session_start_time = std::time::Instant::now();

    // Step 3: Run turn loop
    let (exit_reason, metrics) = run_sdk_turn_loop(
        &mut state,
        &start_params,
        &mut reader,
        &mut writer,
        &hook_bridge,
    )
    .await;

    // Emit session/ended with typed reason
    let reason = match &exit_reason {
        Ok(SdkExitReason::MaxTurns) => SessionEndedReason::MaxTurns,
        Ok(SdkExitReason::StdinClosed) => SessionEndedReason::StdinClosed,
        Ok(SdkExitReason::Error(_)) => SessionEndedReason::Error,
        Err(_) => SessionEndedReason::Error,
    };

    // Emit turn failure for unexpected errors
    if let Err(ref e) = exit_reason {
        writer
            .write_notification(&ServerNotification::TurnFailed(TurnFailedParams {
                error: format!("{e:#}"),
            }))
            .await?;
    } else if let Ok(SdkExitReason::Error(ref msg)) = exit_reason {
        writer
            .write_notification(&ServerNotification::TurnFailed(TurnFailedParams {
                error: msg.clone(),
            }))
            .await?;
    }

    // Emit session/result with aggregated metrics
    let session_duration = session_start_time.elapsed().as_millis() as i64;
    writer
        .write_notification(&ServerNotification::SessionResult(SessionResultParams {
            session_id: session_id.clone(),
            total_turns: metrics.total_turns,
            total_cost_cents: None,
            duration_ms: session_duration,
            duration_api_ms: None,
            usage: metrics.usage,
            stop_reason: reason,
            structured_output: None,
        }))
        .await?;

    writer
        .write_notification(&ServerNotification::SessionEnded(SessionEndedParams {
            reason,
        }))
        .await?;

    Ok(())
}

/// Run the main turn loop for SDK mode.
///
/// Uses `tokio::select!` to concurrently:
/// - Stream events from the agent loop to stdout
/// - Read client requests from stdin (approvals, questions, interrupts)
///
/// Follows the pattern from `tui_runner.rs:run_turn_concurrent`.
async fn run_sdk_turn_loop(
    state: &mut SessionState,
    start_params: &SessionStartRequestParams,
    reader: &mut StdinReader,
    writer: &mut StdoutWriter,
    hook_bridge: &Option<std::sync::Arc<session_builder::SdkHookBridge>>,
) -> (anyhow::Result<SdkExitReason>, SessionMetrics) {
    let mut metrics = SessionMetrics {
        total_turns: 0,
        usage: Default::default(),
    };
    let result = run_sdk_turn_loop_inner(
        state,
        start_params,
        reader,
        writer,
        hook_bridge,
        &mut metrics,
    )
    .await;
    (result, metrics)
}

/// Inner turn loop that propagates errors normally.
/// Metrics are accumulated in `metrics` so they survive early returns.
async fn run_sdk_turn_loop_inner(
    state: &mut SessionState,
    start_params: &SessionStartRequestParams,
    reader: &mut StdinReader,
    writer: &mut StdoutWriter,
    hook_bridge: &Option<std::sync::Arc<session_builder::SdkHookBridge>>,
    metrics: &mut SessionMetrics,
) -> anyhow::Result<SdkExitReason> {
    // First turn uses the prompt from session/start
    let mut prompt = start_params.prompt.clone();
    let mut turn_number: i32 = 0;
    let mut agg_usage = cocode_app_server_protocol::Usage::default();

    loop {
        turn_number += 1;

        // Create event channel for streaming
        let (event_tx, mut event_rx) = mpsc::channel::<LoopEvent>(256);

        // Create permission bridge and wire into session state
        let bridge = Arc::new(SdkPermissionBridge::new(event_tx.clone()));
        state.set_permission_requester(bridge.clone());

        // Emit turn/started
        let turn_id = format!("turn_{turn_number}");
        writer
            .write_notification(&ServerNotification::TurnStarted(TurnStartedParams {
                turn_id: turn_id.clone(),
                turn_number,
            }))
            .await?;

        // Run the turn in a scoped block so the turn future (which borrows
        // &mut state) is dropped before the between-turns code.
        enum TurnOutcome {
            Completed(cocode_app_server_protocol::TurnCompletedParams),
            Failed(String),
            Interrupted,
        }

        let outcome = {
            // Extract shared handles BEFORE borrowing state for the turn.
            let cancel_token = state.cancel_token();
            let question_responder = state.question_responder();
            let subagent_mgr = state.subagent_manager().clone();

            let turn_prompt = prompt.clone();
            let turn_future = state.run_turn_streaming(&turn_prompt, event_tx);
            tokio::pin!(turn_future);

            let mut mapper = EventMapper::new(turn_id.clone());
            let mut turn_result = None;

            // Concurrent event loop: process events, stdin, and hook callbacks
            loop {
                tokio::select! {
                    result = &mut turn_future => {
                        turn_result = Some(result);
                        break;
                    }
                    // Poll for SDK hook callback requests that need to be sent to client
                    Some(req) = async {
                        match hook_bridge {
                            Some(b) => b.recv_request().await,
                            None => std::future::pending().await,
                        }
                    } => {
                        writer.write_server_request(
                            &ServerRequest::HookCallback(
                                cocode_app_server_protocol::HookCallbackParams {
                                    request_id: req.request_id,
                                    callback_id: req.callback_id,
                                    event_type: req.event_type,
                                    input: req.input,
                                },
                            ),
                        ).await?;
                    }
                    Some(event) = event_rx.recv() => {
                        if let LoopEvent::ApprovalRequired { ref request } = event {
                            let server_req = SdkPermissionBridge::create_server_request(request);
                            writer.write_server_request(&server_req).await?;
                            continue;
                        }
                        if let LoopEvent::QuestionAsked { request_id, questions } = event {
                            writer
                                .write_server_request(&ServerRequest::RequestUserInput(
                                    RequestUserInputParams {
                                        request_id,
                                        message: "Agent is asking a question".into(),
                                        questions: Some(questions),
                                    },
                                ))
                                .await?;
                            continue;
                        }
                        if let LoopEvent::ElicitationRequested { request_id, server_name, message, schema, .. } = event {
                            let elicitation_msg = format!("[{server_name}] {message}");
                            writer
                                .write_server_request(&ServerRequest::RequestUserInput(
                                    RequestUserInputParams {
                                        request_id,
                                        message: elicitation_msg,
                                        questions: schema,
                                    },
                                ))
                                .await?;
                            continue;
                        }
                        for notif in mapper.map(event) {
                            writer.write_notification(&notif).await?;
                        }
                    }
                    msg = reader.read_message() => {
                        match msg {
                            Ok(InboundMessage::ApprovalResolve(params)) => {
                                bridge.resolve(&params.request_id, &params.decision).await;
                            }
                            Ok(InboundMessage::UserInputResolve(params)) => {
                                let delivered = question_responder.respond(
                                    &params.request_id,
                                    params.response,
                                );
                                if !delivered {
                                    warn!(
                                        request_id = params.request_id,
                                        "Question response for unknown request_id"
                                    );
                                }
                            }
                            Ok(InboundMessage::HookCallbackResponse(params)) => {
                                if let Some(bridge) = hook_bridge {
                                    bridge.resolve(
                                        &params.request_id,
                                        params.output,
                                    ).await;
                                } else {
                                    tracing::warn!(
                                        request_id = params.request_id,
                                        "Hook callback response but no bridge"
                                    );
                                }
                            }
                            Ok(InboundMessage::UpdateEnv(_)) => {
                                // Env updates are deferred to between turns
                                // (state is mutably borrowed by turn_future).
                            }
                            Ok(InboundMessage::KeepAlive(_)) => {
                                let ts = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as i64)
                                    .unwrap_or(0);
                                let _ = writer
                                    .write_notification(
                                        &ServerNotification::KeepAlive(
                                            cocode_app_server_protocol::KeepAliveParams {
                                                timestamp: ts,
                                            },
                                        ),
                                    )
                                    .await;
                            }
                            Ok(InboundMessage::TurnInterrupt(_)) => {
                                cancel_token.cancel();
                                break;
                            }
                            Ok(InboundMessage::StopTask(params)) => {
                                // StopTask is safe during turn — targets background agents
                                let mut mgr = subagent_mgr.lock().await;
                                if let Some(instance) = mgr.remove_agent(&params.task_id) {
                                    if let Some(token) = &instance.cancel_token {
                                        token.cancel();
                                    }
                                    tracing::info!(task_id = params.task_id, "Background task cancelled (in-turn)");
                                }
                            }
                            Ok(InboundMessage::SetModel(params)) => {
                                tracing::debug!(model = params.model, "Model change queued until turn end");
                            }
                            Ok(InboundMessage::SetPermissionMode(params)) => {
                                tracing::debug!(mode = params.mode, "Permission mode change queued until turn end");
                            }
                            Ok(InboundMessage::SetThinking(_)) => {
                                tracing::debug!("Thinking config change queued until turn end");
                            }
                            Ok(InboundMessage::RewindFiles(_)) => {
                                tracing::debug!("Rewind files queued until turn end");
                            }
                            Ok(_) => {
                                // Other messages ignored during active turn
                            }
                            Err(_) => {
                                cancel_token.cancel();
                                break;
                            }
                        }
                    }
                }
            }

            // Drain remaining events
            while let Ok(event) = event_rx.try_recv() {
                if matches!(event, LoopEvent::ApprovalRequired { .. }) {
                    continue;
                }
                for notif in mapper.map(event) {
                    writer.write_notification(&notif).await?;
                }
            }

            // Flush accumulated text/reasoning
            for notif in mapper.flush() {
                writer.write_notification(&notif).await?;
            }

            // Drain any in-flight hook callbacks to prevent deadlocks
            if let Some(bridge) = hook_bridge {
                bridge.drain_pending().await;
            }

            // Produce outcome
            match turn_result {
                Some(Ok(result)) => {
                    let usage = cocode_app_server_protocol::Usage {
                        input_tokens: result.usage.input_tokens,
                        output_tokens: result.usage.output_tokens,
                        cache_read_tokens: result.usage.cache_read_tokens,
                        cache_creation_tokens: result.usage.cache_creation_tokens,
                        reasoning_tokens: result.usage.reasoning_tokens,
                    };
                    TurnOutcome::Completed(cocode_app_server_protocol::TurnCompletedParams {
                        turn_id: turn_id.clone(),
                        usage,
                    })
                }
                Some(Err(e)) => TurnOutcome::Failed(format!("{e:#}")),
                None => TurnOutcome::Interrupted,
            }
        };
        // turn_future is now dropped — state is available again

        // Emit turn result
        match outcome {
            TurnOutcome::Completed(params) => {
                // Accumulate usage
                agg_usage.input_tokens += params.usage.input_tokens;
                agg_usage.output_tokens += params.usage.output_tokens;
                if let Some(cr) = params.usage.cache_read_tokens {
                    *agg_usage.cache_read_tokens.get_or_insert(0) += cr;
                }
                if let Some(cc) = params.usage.cache_creation_tokens {
                    *agg_usage.cache_creation_tokens.get_or_insert(0) += cc;
                }
                if let Some(rt) = params.usage.reasoning_tokens {
                    *agg_usage.reasoning_tokens.get_or_insert(0) += rt;
                }
                writer
                    .write_notification(&ServerNotification::TurnCompleted(params))
                    .await?;
            }
            TurnOutcome::Failed(error) => {
                writer
                    .write_notification(&ServerNotification::TurnFailed(TurnFailedParams {
                        error: error.clone(),
                    }))
                    .await?;
                metrics.total_turns = turn_number;
                metrics.usage = agg_usage;
                return Ok(SdkExitReason::Error(error));
            }
            TurnOutcome::Interrupted => {
                metrics.total_turns = turn_number;
                metrics.usage = agg_usage;
                return Ok(SdkExitReason::StdinClosed);
            }
        }

        // Check if max turns reached
        if let Some(max) = start_params.max_turns
            && turn_number >= max
        {
            writer
                .write_notification(&ServerNotification::MaxTurnsReached(
                    MaxTurnsReachedParams {
                        max_turns: start_params.max_turns,
                    },
                ))
                .await?;
            metrics.total_turns = turn_number;
            metrics.usage = agg_usage;
            return Ok(SdkExitReason::MaxTurns);
        }

        // Wait for next turn input or control messages from stdin
        loop {
            match reader.read_message().await {
                Ok(InboundMessage::TurnStart(params)) => {
                    prompt = params.text;
                    break;
                }
                Ok(InboundMessage::TurnInterrupt(_)) => {
                    metrics.total_turns = turn_number;
                    metrics.usage = agg_usage;
                    return Ok(SdkExitReason::StdinClosed);
                }
                Ok(InboundMessage::SetModel(params)) => {
                    state.set_model_override(&params.model);
                }
                Ok(InboundMessage::SetPermissionMode(params)) => {
                    state.set_permission_mode_from_str(&params.mode);
                }
                Ok(InboundMessage::StopTask(params)) => {
                    state.cancel_background_task(&params.task_id).await;
                }
                Ok(InboundMessage::SetThinking(params)) => {
                    use cocode_protocol::ThinkingLevel;
                    let level = match params.thinking.mode {
                        cocode_app_server_protocol::ThinkingMode::Enabled => ThinkingLevel::high(),
                        cocode_app_server_protocol::ThinkingMode::Disabled => ThinkingLevel::none(),
                        cocode_app_server_protocol::ThinkingMode::Adaptive => {
                            ThinkingLevel::medium()
                        }
                    };
                    state.switch_thinking_level(cocode_protocol::ModelRole::Main, level);
                }
                Ok(InboundMessage::RewindFiles(params)) => {
                    tracing::info!(
                        turn_id = params.turn_id,
                        "Rewind files requested (wiring deferred)"
                    );
                }
                Ok(InboundMessage::UpdateEnv(params)) => {
                    state.apply_sdk_env_overrides(&params.env);
                }
                Ok(InboundMessage::KeepAlive(_)) => {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    let _ = writer
                        .write_notification(&ServerNotification::KeepAlive(
                            cocode_app_server_protocol::KeepAliveParams { timestamp: ts },
                        ))
                        .await;
                }
                Ok(_) => {
                    // Ignore unexpected messages between turns
                }
                Err(_) => {
                    // stdin closed — session over
                    metrics.total_turns = turn_number;
                    metrics.usage = agg_usage;
                    return Ok(SdkExitReason::StdinClosed);
                }
            }
        }
    }
}

/// Build a `SessionState` from SDK start parameters.
type HookBridge = Option<std::sync::Arc<session_builder::SdkHookBridge>>;

async fn build_session_state(
    config: &ConfigManager,
    params: &SessionStartRequestParams,
) -> anyhow::Result<(SessionState, HookBridge)> {
    let working_dir = params
        .cwd
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut overrides = ConfigOverrides::default().with_cwd(working_dir.clone());

    // Apply sandbox configuration from SDK params
    if let Some(ref sandbox) = params.sandbox {
        let sandbox_mode = match sandbox.mode {
            cocode_app_server_protocol::SandboxMode::None => {
                cocode_protocol::SandboxMode::FullAccess
            }
            cocode_app_server_protocol::SandboxMode::ReadOnly => {
                cocode_protocol::SandboxMode::ReadOnly
            }
            cocode_app_server_protocol::SandboxMode::Strict => {
                cocode_protocol::SandboxMode::ReadOnly
            }
        };
        overrides.sandbox_mode = Some(sandbox_mode);
    }

    let snapshot = Arc::new(config.build_config(overrides)?);

    let selections = config.build_all_selections();
    let mut session = Session::with_selections(working_dir, selections);

    if let Some(max) = params.max_turns {
        session.set_max_turns(Some(max));
    }

    let mut state = SessionState::new(session, snapshot)
        .await
        .context("failed to create session state")?;

    // Apply system prompt configuration
    if let Some(ref system_prompt) = params.system_prompt {
        match system_prompt {
            cocode_app_server_protocol::SystemPromptConfig::Raw(prompt) => {
                // Raw string = full system prompt override
                state.set_system_prompt_override(prompt.clone());
            }
            cocode_app_server_protocol::SystemPromptConfig::Structured { append, .. } => {
                // Structured = use preset base + optional append
                if let Some(text) = append {
                    state.set_system_prompt_suffix(text.clone());
                }
            }
        }
    } else if let Some(ref suffix) = params.system_prompt_suffix {
        state.set_system_prompt_suffix(suffix.clone());
    }

    // Apply permission mode
    if let Some(ref mode) = params.permission_mode {
        state.set_permission_mode_from_str(mode);
    }

    // Apply model override
    if let Some(ref model) = params.model {
        state.set_model_override(model);
    }

    // Apply SDK-specific parameters (agents, hooks, MCP, etc.)
    let hook_bridge = session_builder::apply_sdk_params(&mut state, params).await?;

    Ok((state, hook_bridge))
}

/// Resume an existing session by ID.
async fn resume_session(
    config: &ConfigManager,
    params: &cocode_app_server_protocol::SessionResumeRequestParams,
) -> anyhow::Result<SessionState> {
    let snapshot = Arc::new(config.build_config(ConfigOverrides::default())?);

    let mut manager = cocode_session::SessionManager::new();
    manager
        .load_session(&params.session_id, snapshot)
        .await
        .context("failed to load session for resume")?;

    manager
        .remove_session(&params.session_id)
        .ok_or_else(|| anyhow::anyhow!("session {} not found after load", params.session_id))
}

/// Initialize stderr-only logging for SDK mode.
fn init_sdk_logging(config: &ConfigManager) {
    use tracing_subscriber::fmt;
    use tracing_subscriber::prelude::*;

    let logging_config = config.logging_config();
    let common_logging = logging_config
        .map(|c| c.to_common_logging())
        .unwrap_or_default();

    let stderr_layer = cocode_utils_common::configure_fmt_layer!(
        fmt::layer().with_writer(std::io::stderr).compact(),
        &common_logging,
        "warn"
    );

    let _ = tracing_subscriber::registry().with(stderr_layer).try_init();
}

#[cfg(test)]
#[path = "event_mapper.test.rs"]
mod event_mapper_tests;
