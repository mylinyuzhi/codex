//! Turn execution engine.
//!
//! Runs a single agent turn, streaming events through the connection's
//! outbound channel via `EventMapper`. Handles approval/question routing
//! and hook callbacks.

use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use cocode_app_server_protocol::RequestId;
use cocode_app_server_protocol::RequestUserInputParams;
use cocode_app_server_protocol::ServerNotification;
use cocode_app_server_protocol::ServerRequest;
use cocode_app_server_protocol::TurnCompletedParams;
use cocode_app_server_protocol::TurnStartedParams;
use cocode_app_server_protocol::Usage;
use cocode_protocol::LoopEvent;
use cocode_session::SessionState;
use tokio::sync::mpsc;
use tracing::info;
use tracing::warn;

use crate::event_mapper::EventMapper;
use crate::permission::SdkPermissionBridge;
use crate::processor::OutboundMessage;
use crate::session_builder::SdkHookBridge;

/// Result of a turn execution.
pub enum TurnOutcome {
    /// Turn completed successfully with usage.
    Completed(TurnCompletedParams),
    /// Turn failed with an error message.
    Failed(String),
    /// Turn was interrupted (cancelled).
    Interrupted,
}

/// Configuration for a single turn execution.
pub struct TurnConfig<'a> {
    pub turn_id: String,
    pub turn_number: i32,
    pub outbound: &'a mpsc::Sender<OutboundMessage>,
    pub hook_bridge: &'a Option<Arc<SdkHookBridge>>,
    pub request_counter: &'a AtomicI64,
}

/// Result of running a turn, including the permission bridge for
/// approval routing while the turn is in progress.
pub struct TurnResult {
    pub outcome: TurnOutcome,
    /// The permission bridge used during this turn. The caller stores
    /// this on the `SessionHandle` so `ApprovalResolve` messages from
    /// the client can reach it.
    pub permission_bridge: Arc<SdkPermissionBridge>,
}

/// Run a single turn and stream all events to the outbound channel.
///
/// Creates the permission bridge internally using the real event channel,
/// and returns it so the caller can route approval responses.
pub async fn run_turn(
    state: &mut SessionState,
    prompt: &str,
    config: TurnConfig<'_>,
) -> TurnResult {
    let TurnConfig {
        turn_id,
        turn_number,
        outbound,
        hook_bridge,
        request_counter,
    } = config;

    let _span = tracing::info_span!("turn", %turn_id, turn_number).entered();

    if outbound
        .send(OutboundMessage::Notification(
            ServerNotification::TurnStarted(TurnStartedParams {
                turn_id: turn_id.clone(),
                turn_number,
            }),
        ))
        .await
        .is_err()
    {
        warn!("Outbound channel closed before turn started");
        // Return a no-op bridge (receiver dropped, but turn is interrupted anyway)
        let (dummy_tx, _) = mpsc::channel::<LoopEvent>(1);
        return TurnResult {
            outcome: TurnOutcome::Interrupted,
            permission_bridge: Arc::new(SdkPermissionBridge::new(dummy_tx)),
        };
    }

    let (event_tx, mut event_rx) = mpsc::channel::<LoopEvent>(256);

    // Create bridge with the real event_tx so approval events reach the select! loop
    let bridge = Arc::new(SdkPermissionBridge::new(event_tx.clone()));
    state.set_permission_requester(bridge.clone());

    let turn_future = state.run_turn_streaming(prompt, event_tx);
    tokio::pin!(turn_future);

    let mut mapper = EventMapper::new(turn_id.clone());
    #[allow(unused_assignments)]
    let mut turn_result = None;

    // Concurrent event loop
    loop {
        tokio::select! {
            result = &mut turn_future => {
                turn_result = Some(result);
                break;
            }
            Some(req) = async {
                match hook_bridge {
                    Some(b) => b.recv_request().await,
                    None => std::future::pending().await,
                }
            } => {
                send_server_request(
                    outbound,
                    request_counter,
                    ServerRequest::HookCallback(
                        cocode_app_server_protocol::HookCallbackParams {
                            request_id: req.request_id,
                            callback_id: req.callback_id,
                            event_type: req.event_type,
                            input: req.input,
                        },
                    ),
                );
            }
            Some(event) = event_rx.recv() => {
                if let LoopEvent::ApprovalRequired { ref request } = event {
                    let server_req = SdkPermissionBridge::create_server_request(request);
                    send_server_request(outbound, request_counter, server_req);
                    continue;
                }
                if let LoopEvent::QuestionAsked { request_id, questions } = event {
                    send_server_request(outbound, request_counter, ServerRequest::RequestUserInput(
                        RequestUserInputParams {
                            request_id,
                            message: "Agent is asking a question".into(),
                            questions: Some(questions),
                        },
                    ));
                    continue;
                }
                if let LoopEvent::ElicitationRequested { request_id, server_name, message, schema, .. } = event {
                    send_server_request(outbound, request_counter, ServerRequest::RequestUserInput(
                        RequestUserInputParams {
                            request_id,
                            message: format!("[{server_name}] {message}"),
                            questions: schema,
                        },
                    ));
                    continue;
                }
                for notif in mapper.map(event) {
                    if outbound.try_send(OutboundMessage::Notification(notif)).is_err() {
                        warn!("Outbound channel full, dropping event notification");
                    }
                }
            }
        }
    }

    while let Ok(event) = event_rx.try_recv() {
        if matches!(event, LoopEvent::ApprovalRequired { .. }) {
            continue;
        }
        for notif in mapper.map(event) {
            if outbound
                .try_send(OutboundMessage::Notification(notif))
                .is_err()
            {
                warn!("Outbound channel full during drain");
            }
        }
    }

    for notif in mapper.flush() {
        if outbound
            .try_send(OutboundMessage::Notification(notif))
            .is_err()
        {
            warn!("Outbound channel full during flush");
        }
    }

    if let Some(bridge) = hook_bridge {
        bridge.drain_pending().await;
    }

    let outcome = match turn_result {
        Some(Ok(result)) => {
            let usage = Usage {
                input_tokens: result.usage.input_tokens,
                output_tokens: result.usage.output_tokens,
                cache_read_tokens: result.usage.cache_read_tokens,
                cache_creation_tokens: result.usage.cache_creation_tokens,
                reasoning_tokens: result.usage.reasoning_tokens,
            };
            let params = TurnCompletedParams { turn_id, usage };
            info!(turns = result.turns_completed, "Turn completed");
            TurnOutcome::Completed(params)
        }
        Some(Err(e)) => TurnOutcome::Failed(format!("{e:#}")),
        None => TurnOutcome::Interrupted,
    };

    TurnResult {
        outcome,
        permission_bridge: bridge,
    }
}

/// Send a server request to the client via the outbound channel.
///
/// Assigns an auto-incrementing request ID. The client's response is
/// routed back through `ApprovalResolve` or `UserInputResolve`.
fn send_server_request(
    outbound: &mpsc::Sender<OutboundMessage>,
    counter: &AtomicI64,
    request: ServerRequest,
) {
    let id = RequestId::Integer(counter.fetch_add(1, Ordering::Relaxed));
    if outbound
        .try_send(OutboundMessage::ServerRequest { id, request })
        .is_err()
    {
        warn!("Outbound channel full, dropping server request");
    }
}
