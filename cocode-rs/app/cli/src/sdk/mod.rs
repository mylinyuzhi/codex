//! SDK mode — non-interactive NDJSON interface for programmatic access.
//!
//! When `--sdk-mode` is passed, the CLI:
//! 1. Reads a `SessionStartRequestParams` from stdin (first JSON line)
//! 2. Maps `LoopEvent` → `ServerNotification` → NDJSON to stdout
//! 3. Reads `ClientRequest` from stdin (subsequent lines)
//! 4. Routes approval responses and control messages back to the agent loop

mod control;
mod event_mapper;
mod stdio;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use cocode_app_server_protocol::ServerNotification;
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

use self::control::ApprovalHandler;
use self::event_mapper::EventMapper;
use self::stdio::InboundMessage;
use self::stdio::NdjsonTransport;

/// Run the CLI in SDK mode.
///
/// Reads configuration from stdin, runs agent turns, and streams
/// events as NDJSON to stdout.
pub async fn run_sdk_mode(config: &ConfigManager) -> anyhow::Result<()> {
    // Initialize stderr-only logging (stdout is reserved for NDJSON)
    init_sdk_logging(config);

    let mut transport = NdjsonTransport::new();

    // Step 1: Read session start request from stdin
    let start_params = match transport.read_message().await? {
        InboundMessage::SessionStart(params) => params,
        other => anyhow::bail!("expected session/start as first message, got {other:?}"),
    };

    info!(prompt_len = start_params.prompt.len(), "SDK session start");

    // Step 2: Build session state
    let mut state = build_session_state(config, &start_params).await?;

    // Emit session/started
    let session_id = state.session.id.clone();
    transport
        .write_notification(&ServerNotification::SessionStarted(SessionStartedParams {
            session_id: session_id.clone(),
        }))
        .await?;

    // Step 3: Run turn loop
    let result = run_sdk_turn_loop(&mut state, &start_params, &mut transport).await;

    // Emit final result or error
    match result {
        Ok(()) => {}
        Err(e) => {
            transport
                .write_notification(&ServerNotification::TurnFailed(TurnFailedParams {
                    error: format!("{e:#}"),
                }))
                .await?;
        }
    }

    Ok(())
}

/// Run the main turn loop for SDK mode.
async fn run_sdk_turn_loop(
    state: &mut SessionState,
    start_params: &SessionStartRequestParams,
    transport: &mut NdjsonTransport,
) -> anyhow::Result<()> {
    // First turn uses the prompt from session/start
    let mut prompt = start_params.prompt.clone();
    let mut turn_number: i32 = 0;

    loop {
        turn_number += 1;

        // Create event channel for streaming
        let (event_tx, mut event_rx) = mpsc::channel::<LoopEvent>(256);

        // Emit turn/started
        let turn_id = format!("turn_{turn_number}");
        transport
            .write_notification(&ServerNotification::TurnStarted(TurnStartedParams {
                turn_id: turn_id.clone(),
                turn_number,
            }))
            .await?;

        // Run the turn with streaming event channel.
        let turn_handle = state.run_turn_streaming(&prompt, event_tx).await;

        // Process events from the turn (streaming to stdout)
        // Note: The turn runs synchronously above, so events are produced
        // before we get here. For true streaming, we'd need to spawn the
        // turn as a task. For now, this is a batch approach that still
        // maps events correctly.
        let mut mapper = EventMapper::new(turn_id.clone());
        let mut approval_handler = ApprovalHandler::new();

        // Drain remaining events from the channel
        while let Ok(event) = event_rx.try_recv() {
            // Check if this is an approval request that needs routing
            if let LoopEvent::ApprovalRequired { ref request } = event {
                let approval_notif = approval_handler.create_approval_request(request);
                transport.write_server_request(&approval_notif).await?;

                // Read approval response from stdin
                // (In a full implementation, this would be async with
                // the event stream. For now, we block waiting for response.)
                if let Ok(InboundMessage::ApprovalResolve(resolve)) = transport.read_message().await
                {
                    approval_handler.resolve(&resolve);
                }
                continue;
            }

            let notifications = mapper.map(event);
            for notif in notifications {
                transport.write_notification(&notif).await?;
            }
        }

        // Flush accumulated text/reasoning items before turn result
        for notif in mapper.flush() {
            transport.write_notification(&notif).await?;
        }

        // Emit turn result
        match turn_handle {
            Ok(result) => {
                let usage = cocode_app_server_protocol::Usage {
                    input_tokens: result.usage.input_tokens,
                    output_tokens: result.usage.output_tokens,
                    cache_read_tokens: result.usage.cache_read_tokens,
                    cache_creation_tokens: result.usage.cache_creation_tokens,
                    reasoning_tokens: result.usage.reasoning_tokens,
                };
                transport
                    .write_notification(&ServerNotification::TurnCompleted(
                        cocode_app_server_protocol::TurnCompletedParams { turn_id, usage },
                    ))
                    .await?;
            }
            Err(e) => {
                transport
                    .write_notification(&ServerNotification::TurnFailed(TurnFailedParams {
                        error: format!("{e:#}"),
                    }))
                    .await?;
                break;
            }
        }

        // Check if max turns reached
        if let Some(max) = start_params.max_turns
            && turn_number >= max
        {
            break;
        }

        // Wait for next turn input from stdin
        match transport.read_message().await {
            Ok(InboundMessage::TurnStart(params)) => {
                prompt = params.text;
            }
            Ok(InboundMessage::TurnInterrupt(_)) => {
                break;
            }
            Ok(_) => {
                // Ignore unexpected messages
                break;
            }
            Err(_) => {
                // stdin closed — session over
                break;
            }
        }
    }

    Ok(())
}

/// Build a `SessionState` from SDK start parameters.
async fn build_session_state(
    config: &ConfigManager,
    params: &SessionStartRequestParams,
) -> anyhow::Result<SessionState> {
    let working_dir = params
        .cwd
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let snapshot =
        Arc::new(config.build_config(ConfigOverrides::default().with_cwd(working_dir.clone()))?);

    let selections = config.build_all_selections();
    let mut session = Session::with_selections(working_dir, selections);

    if let Some(max) = params.max_turns {
        session.set_max_turns(Some(max));
    }

    let mut state = SessionState::new(session, snapshot)
        .await
        .context("failed to create session state")?;

    if let Some(ref suffix) = params.system_prompt_suffix {
        state.set_system_prompt_suffix(suffix.clone());
    }

    Ok(state)
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
