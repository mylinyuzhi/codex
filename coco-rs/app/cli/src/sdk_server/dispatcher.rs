//! SDK server dispatch loop.
//!
//! The `SdkServer` reads `JsonRpcMessage` requests from a transport,
//! dispatches them to per-method handlers, and writes responses +
//! forwarded CoreEvent notifications back to the transport.
//!
//! TS reference: `src/cli/structuredIO.ts` + `src/cli/print.ts` — the
//! `runHeadless` loop reads stdin, routes control requests, and enqueues
//! messages to stdout.

use std::sync::Arc;

use coco_query::StreamAccumulator;
use coco_types::AgentStreamEvent;
use coco_types::ClientRequest;
use coco_types::CoreEvent;
use coco_types::JsonRpcError;
use coco_types::JsonRpcMessage;
use coco_types::JsonRpcNotification;
use coco_types::JsonRpcRequest;
use coco_types::JsonRpcResponse;
use coco_types::RequestId;
use coco_types::ServerNotification;
use coco_types::error_codes;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::sdk_server::handlers::HandlerContext;
use crate::sdk_server::handlers::HandlerResult;
use crate::sdk_server::handlers::SdkServerState;
use crate::sdk_server::handlers::TurnRunner;
use crate::sdk_server::handlers::dispatch_client_request;
use crate::sdk_server::transport::SdkTransport;
use crate::sdk_server::transport::TransportError;

/// The SDK server — owns the transport, dispatches ClientRequests, and
/// forwards CoreEvent notifications to the client.
///
/// Lifecycle:
/// 1. Construct with `SdkServer::new(transport)`.
/// 2. Call `run()` which loops until the transport closes or an explicit
///    shutdown ClientRequest is received.
/// 3. Each iteration reads one `JsonRpcMessage`, dispatches it, writes the
///    response. Notifications from the agent loop are forwarded via a
///    background task set up in `run()`.
pub struct SdkServer {
    transport: Arc<dyn SdkTransport>,
    /// Shared session state across dispatched requests.
    state: Arc<SdkServerState>,
}

impl SdkServer {
    /// Create a new SDK server bound to a transport.
    ///
    /// The transport is published onto `state.transport` immediately so
    /// code paths that read it (e.g. [`crate::sdk_server::SdkPermissionBridge`])
    /// see a populated slot without waiting for [`Self::run`] to start.
    /// This avoids a startup race where a bridge consulted between
    /// `new()` and `run()` would erroneously see `None`.
    pub fn new(transport: Arc<dyn SdkTransport>) -> Self {
        let state = Arc::new(SdkServerState::default());
        // Pre-populate the transport slot. At construction time nothing
        // else has a lock on the state, so `try_write` is guaranteed to
        // succeed. We panic if it doesn't — that would indicate a
        // programmer error (e.g. the state was pre-shared).
        {
            let Ok(mut slot) = state.transport.try_write() else {
                panic!("SdkServer::new: state was already locked at construction time");
            };
            *slot = Some(transport.clone());
        }
        Self { transport, state }
    }

    /// Inject a custom [`TurnRunner`] synchronously during builder
    /// construction. Mutates the existing shared state in place (via
    /// `try_write`). Call this before `run()` to wire the production
    /// `QueryEngine`-backed runner, or to install a mock runner in
    /// tests. Without this, `turn/start` fails with `NotImplementedRunner`.
    ///
    /// Panics if the `turn_runner` lock is already held — that would
    /// indicate a programmer error (the state was pre-shared and a
    /// reader is active during construction).
    pub fn with_turn_runner(self, runner: Arc<dyn TurnRunner>) -> Self {
        let Ok(mut slot) = self.state.turn_runner.try_write() else {
            panic!("with_turn_runner: state was already locked at construction time");
        };
        *slot = runner;
        drop(slot);
        self
    }

    /// Install a disk-backed [`coco_session::SessionManager`] so the
    /// `session/list`, `session/read`, `session/resume` handlers can
    /// browse and restore historical sessions. Without this, those
    /// handlers reply with `METHOD_NOT_FOUND`.
    pub fn with_session_manager(self, manager: Arc<coco_session::SessionManager>) -> Self {
        let Ok(mut slot) = self.state.session_manager.try_write() else {
            panic!("with_session_manager: state was already locked at construction time");
        };
        *slot = Some(manager);
        drop(slot);
        self
    }

    /// Install a [`coco_context::FileHistoryState`] + config home so
    /// the `control/rewindFiles` handler can preview and apply file
    /// rewinds. Without this, the handler errors with
    /// `INVALID_REQUEST` ("file history not enabled").
    pub fn with_file_history(
        self,
        history: Arc<tokio::sync::RwLock<coco_context::FileHistoryState>>,
        config_home: std::path::PathBuf,
    ) -> Self {
        {
            let Ok(mut slot) = self.state.file_history.try_write() else {
                panic!("with_file_history: state was already locked at construction time");
            };
            *slot = Some(history);
        }
        {
            let Ok(mut slot) = self.state.file_history_config_home.try_write() else {
                panic!("with_file_history: state was already locked at construction time");
            };
            *slot = Some(config_home);
        }
        self
    }

    /// Install an [`coco_mcp::McpConnectionManager`] so the
    /// `mcp/setServers`, `mcp/reconnect`, `mcp/toggle` handlers can
    /// register configs and drive connection lifecycle. Without this,
    /// those handlers reply with `INVALID_REQUEST` ("MCP manager not
    /// enabled").
    pub fn with_mcp_manager(
        self,
        manager: Arc<tokio::sync::Mutex<coco_mcp::McpConnectionManager>>,
    ) -> Self {
        let Ok(mut slot) = self.state.mcp_manager.try_write() else {
            panic!("with_mcp_manager: state was already locked at construction time");
        };
        *slot = Some(manager);
        drop(slot);
        self
    }

    /// Install an [`InitializeBootstrap`] provider so `handle_initialize`
    /// returns real data (commands, agents, account, output styles) instead
    /// of empty / default values. Without this, `initialize` still succeeds
    /// with a TS-conformant shape but empty lists.
    pub fn with_initialize_bootstrap(
        self,
        bootstrap: Arc<dyn crate::sdk_server::handlers::InitializeBootstrap>,
    ) -> Self {
        let Ok(mut slot) = self.state.initialize_bootstrap.try_write() else {
            panic!("with_initialize_bootstrap: state was already locked at construction time");
        };
        *slot = Some(bootstrap);
        drop(slot);
        self
    }

    /// Asynchronously replace the installed [`TurnRunner`]. Used by
    /// code paths that need to construct the runner after cloning the
    /// shared state (e.g. the approval-bridge wiring in
    /// `run_sdk_mode`, where the bridge needs a reference to live
    /// state before the runner exists).
    pub async fn set_turn_runner(&self, runner: Arc<dyn TurnRunner>) {
        let mut slot = self.state.turn_runner.write().await;
        *slot = runner;
    }

    /// Access the underlying transport — used by code paths that need
    /// to issue outbound `ServerRequest` messages (e.g. the approval
    /// bridge).
    pub fn transport(&self) -> Arc<dyn SdkTransport> {
        self.transport.clone()
    }

    /// Access the shared state. Used by tests (and in the future, the CLI
    /// wiring) to register pending approvals / user inputs before sending
    /// the matching ServerRequest on the wire.
    pub fn state(&self) -> Arc<SdkServerState> {
        self.state.clone()
    }

    /// Run the dispatch loop. Returns when:
    /// - The transport receives EOF (clean peer disconnect).
    /// - An unrecoverable transport I/O error occurs.
    /// - An explicit shutdown request arrives (future).
    ///
    /// The loop is single-request-at-a-time: each `recv()` is processed
    /// before the next is read. Concurrent request pipelining can be
    /// added later via `tokio::spawn` per request.
    pub async fn run(&self) -> Result<(), TransportError> {
        info!("SdkServer starting dispatch loop");

        // Channel for CoreEvent notifications forwarded from handlers to
        // the transport. Capacity matches the engine's event channel so
        // upstream backpressure flows naturally.
        //
        // Note: `state.transport` is already populated by `new()` so
        // `send_server_request` and the approval bridge work correctly
        // from the moment the server exists, not just from the moment
        // `run()` executes its first await point.
        let (notif_tx, mut notif_rx) = mpsc::channel::<CoreEvent>(256);
        let (reply_tx, mut reply_rx) = mpsc::channel::<JsonRpcMessage>(256);

        // Background task: single-writer transport serializer. Drains both
        // the notification channel and the per-request reply channel,
        // preferring notifications first so every notification queued
        // before a reply is written to the transport first.
        //
        // Routing all outbound transport writes through this one task
        // gives the SDK a total order on wire messages — critical for
        // `session/archive`, where the aggregated `SessionResult` event
        // must land BEFORE the archive's own JSON-RPC reply.
        let notif_transport = self.transport.clone();
        let writer_task = tokio::spawn(async move {
            // Per-turn StreamAccumulator. Converts AgentStreamEvent sequences
            // into semantic ServerNotification::ItemStarted/Updated/Completed
            // + AgentMessageDelta/ReasoningDelta protocol events. Reset on
            // each TurnStarted, flushed on TurnCompleted/Failed/Interrupted.
            //
            // TS: the SDK path uses normalizeMessage() + sdkEventQueue; we
            // use the same StreamAccumulator that the design doc specifies.
            let mut accumulator: Option<StreamAccumulator> = None;
            // Buffer stream events that arrive before TurnStarted.
            let mut pre_turn_buffer: Vec<AgentStreamEvent> = Vec::new();

            /// Send a single JsonRpcNotification to the transport.
            /// Returns false if the transport is closed.
            async fn send_notif(transport: &dyn SdkTransport, notif: JsonRpcNotification) -> bool {
                if let Err(e) = transport.send(JsonRpcMessage::Notification(notif)).await {
                    warn!(error = %e, "notification forward failed");
                    return false;
                }
                true
            }

            /// Forward a Vec of ServerNotifications produced by the
            /// StreamAccumulator. Returns false if any send fails.
            async fn send_accumulated(
                transport: &dyn SdkTransport,
                notifications: Vec<ServerNotification>,
            ) -> bool {
                for sn in notifications {
                    if let Some(notif) = server_notification_to_jsonrpc(sn) {
                        if !send_notif(transport, notif).await {
                            return false;
                        }
                    }
                }
                true
            }

            loop {
                tokio::select! {
                    biased;
                    event = notif_rx.recv() => {
                        match event {
                            Some(CoreEvent::Protocol(notif)) => {
                                // Lifecycle hooks for accumulator scoping.
                                match &notif {
                                    ServerNotification::TurnStarted(p) => {
                                        let turn_id = p.turn_id.clone().unwrap_or_default();
                                        let mut acc = StreamAccumulator::new(turn_id);
                                        // Drain any stream events that arrived before TurnStarted.
                                        let buffered: Vec<_> = pre_turn_buffer
                                            .drain(..)
                                            .flat_map(|evt| acc.process(evt))
                                            .collect();
                                        if !send_accumulated(&*notif_transport, buffered).await {
                                            break;
                                        }
                                        accumulator = Some(acc);
                                    }
                                    ServerNotification::TurnCompleted(_)
                                    | ServerNotification::TurnFailed(_)
                                    | ServerNotification::TurnInterrupted(_) => {
                                        // Flush trailing items BEFORE the terminator.
                                        if let Some(ref mut acc) = accumulator {
                                            let flushed = acc.flush();
                                            if !send_accumulated(&*notif_transport, flushed).await {
                                                break;
                                            }
                                        }
                                        accumulator = None;
                                        pre_turn_buffer.clear();
                                    }
                                    _ => {}
                                }
                                // Forward the protocol event itself.
                                if let Some(jrpc) = server_notification_to_jsonrpc(notif) {
                                    if !send_notif(&*notif_transport, jrpc).await {
                                        break;
                                    }
                                }
                            }
                            Some(CoreEvent::Stream(stream_evt)) => {
                                // Feed the accumulator, which converts raw
                                // stream events into semantic item lifecycle
                                // ServerNotifications.
                                let notifications = if let Some(ref mut acc) = accumulator {
                                    acc.process(stream_evt)
                                } else {
                                    // Buffer until TurnStarted provides a real turn_id.
                                    debug!("stream event before TurnStarted; buffering");
                                    pre_turn_buffer.push(stream_evt);
                                    Vec::new()
                                };
                                if !send_accumulated(&*notif_transport, notifications).await {
                                    break;
                                }
                            }
                            Some(CoreEvent::Tui(_)) => {
                                // TUI-only events are dropped by non-TUI consumers.
                            }
                            None => {
                                // Notification channel closed — drain replies then exit.
                                while let Some(reply) = reply_rx.recv().await {
                                    if let Err(e) = notif_transport.send(reply).await {
                                        warn!(error = %e, "reply forward failed");
                                        break;
                                    }
                                }
                                break;
                            }
                        }
                    }
                    reply = reply_rx.recv() => {
                        match reply {
                            Some(reply) => {
                                if let Err(e) = notif_transport.send(reply).await {
                                    warn!(error = %e, "reply forward failed");
                                    break;
                                }
                            }
                            None => {
                                // Reply channel closed; notifications still
                                // coming — keep looping to drain them.
                            }
                        }
                    }
                }
            }
            debug!("transport writer exited");
        });

        // Main dispatch loop.
        loop {
            match self.transport.recv().await {
                Ok(Some(msg)) => {
                    self.handle_message(msg, notif_tx.clone(), &reply_tx).await;
                }
                Ok(None) => {
                    info!("SdkServer: transport EOF, shutting down");
                    break;
                }
                Err(TransportError::Closed) => {
                    info!("SdkServer: transport closed");
                    break;
                }
                Err(e) => {
                    warn!(error = %e, "SdkServer: transport error");
                    break;
                }
            }
        }

        // Drop the notification + reply senders to signal the writer
        // task to exit. It drains any pending items first.
        drop(notif_tx);
        drop(reply_tx);
        let _ = writer_task.await;

        Ok(())
    }

    /// Handle one incoming message.
    ///
    /// - `Request` → dispatched to the matching handler.
    /// - `Response` / `Error` → routed to a pending ServerRequest via
    ///   [`SdkServerState::resolve_server_request`]. If no pending
    ///   request matches, the message is logged and dropped.
    /// - `Notification` → logged and dropped (coco-rs SDK clients do
    ///   not emit notifications to the server).
    async fn handle_message(
        &self,
        msg: JsonRpcMessage,
        notif_tx: mpsc::Sender<CoreEvent>,
        reply_tx: &mpsc::Sender<JsonRpcMessage>,
    ) {
        match msg {
            JsonRpcMessage::Request(req) => {
                let request_id = req.request_id.clone();
                let reply = self.handle_request(req, notif_tx).await;
                // Route via the writer task so replies are ordered AFTER
                // any notifications the handler emitted on `notif_tx`.
                if let Err(e) = reply_tx.send(reply).await {
                    warn!(error = %e, request_id = %request_id.as_display(), "reply send failed");
                }
            }
            msg @ (JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_)) => {
                // Route to a pending ServerRequest if one matches.
                let routed = self.state.resolve_server_request(msg).await;
                if !routed {
                    debug!("SdkServer: received unmatched response/error, dropping");
                }
            }
            JsonRpcMessage::Notification(n) => {
                debug!(method = %n.method, "SdkServer: ignoring client notification");
            }
        }
    }

    /// Dispatch a single request and produce the reply message.
    ///
    /// Parses the `method` string to a typed `ClientRequest` variant and
    /// delegates to `handlers::dispatch_client_request`. If parsing fails,
    /// returns a JSON-RPC MethodNotFound or InvalidParams error.
    async fn handle_request(
        &self,
        req: JsonRpcRequest,
        notif_tx: mpsc::Sender<CoreEvent>,
    ) -> JsonRpcMessage {
        let request_id = req.request_id.clone();

        // Reconstruct a ClientRequest from the method + params.
        //
        // `ClientRequest` is `#[serde(tag = "method", content = "params")]`
        // which means:
        //   - Tuple variants (with params) serialize as
        //     `{"method": "initialize", "params": {...}}`.
        //   - Unit variants (no params) serialize as `{"method": "keepAlive"}`
        //     with NO `params` key.
        //
        // We don't know in advance which shape the method name wants, so we
        // try WITH params first and fall back to WITHOUT on parse failure
        // (which catches unit variants that came in with an empty params
        // object from the client). This matches what JSON-RPC clients do
        // in practice — always send `params: {}` for parameterless calls.
        let with_params = serde_json::json!({
            "method": req.method,
            "params": req.params,
        });
        let without_params = serde_json::json!({ "method": req.method });

        let client_req: ClientRequest =
            match serde_json::from_value::<ClientRequest>(with_params.clone()) {
                Ok(r) => r,
                Err(e_with) => {
                    // Retry without params for unit-variant methods.
                    match serde_json::from_value::<ClientRequest>(without_params) {
                        Ok(r) => r,
                        Err(_) => {
                            warn!(
                                method = %req.method,
                                error = %e_with,
                                "SdkServer: failed to parse ClientRequest"
                            );
                            return error_reply(
                                request_id,
                                error_codes::INVALID_PARAMS,
                                format!("invalid params for method {}: {}", req.method, e_with),
                            );
                        }
                    }
                }
            };

        let ctx = HandlerContext {
            notif_tx,
            state: self.state.clone(),
        };
        match dispatch_client_request(client_req, ctx).await {
            HandlerResult::Ok(result) => success_reply(request_id, result),
            HandlerResult::Err {
                code,
                message,
                data,
            } => error_reply_with_data(request_id, code, message, data),
            HandlerResult::NotImplemented(method) => error_reply(
                request_id,
                error_codes::METHOD_NOT_FOUND,
                format!("method {method} is not implemented yet"),
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Reply builders
// ---------------------------------------------------------------------------

fn success_reply(request_id: RequestId, result: Value) -> JsonRpcMessage {
    JsonRpcMessage::Response(JsonRpcResponse { request_id, result })
}

fn error_reply(request_id: RequestId, code: i32, message: String) -> JsonRpcMessage {
    JsonRpcMessage::Error(JsonRpcError {
        request_id,
        code,
        message,
        data: None,
    })
}

fn error_reply_with_data(
    request_id: RequestId,
    code: i32,
    message: String,
    data: Option<Value>,
) -> JsonRpcMessage {
    JsonRpcMessage::Error(JsonRpcError {
        request_id,
        code,
        message,
        data,
    })
}

// ---------------------------------------------------------------------------
// CoreEvent → JsonRpcNotification
// ---------------------------------------------------------------------------

/// Translate a `CoreEvent` into a `JsonRpcNotification` suitable for the
/// wire. Returns `None` for `CoreEvent::Tui(_)` (dropped by non-TUI
/// consumers) and `CoreEvent::Stream(_)` (handled by the writer task's
/// `StreamAccumulator`, not this function).
///
/// See `event-system-design.md` §12.
///
/// Only used in tests — the production writer task handles dispatch inline.
#[cfg(test)]
fn core_event_to_notification(event: CoreEvent) -> Option<JsonRpcNotification> {
    match event {
        CoreEvent::Protocol(notif) => server_notification_to_jsonrpc(notif),
        CoreEvent::Stream(_) => None,
        CoreEvent::Tui(_) => None,
    }
}

/// Serialize a `ServerNotification` as a `JsonRpcNotification` directly.
/// Exposed for handlers that want to emit synthetic protocol notifications
/// without going through CoreEvent.
///
/// Uses `ServerNotification::method()` for the wire method and moves the
/// params subtree out of the intermediate `Value` object so the hot path
/// (per-token `AgentMessageDelta` / `ReasoningDelta`) doesn't deep-clone
/// the params payload on every emission.
pub fn server_notification_to_jsonrpc(notif: ServerNotification) -> Option<JsonRpcNotification> {
    let method = notif.method().to_string();
    let value = serde_json::to_value(&notif).ok()?;
    let params = match value {
        Value::Object(mut map) => map.remove("params").unwrap_or(Value::Null),
        _ => return None,
    };
    Some(JsonRpcNotification { method, params })
}

#[cfg(test)]
#[path = "dispatcher.test.rs"]
mod tests;
