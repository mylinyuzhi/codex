//! JSON-RPC message processor.
//!
//! Dispatches inbound messages to the appropriate handler and manages
//! per-connection session state including live agent turns.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;

use cocode_app_server_protocol::ClientRequest;
use cocode_app_server_protocol::InitializeResult;
use cocode_app_server_protocol::JsonRpcMessage;
use cocode_app_server_protocol::JsonRpcRequest;
use cocode_app_server_protocol::RequestId;
use cocode_app_server_protocol::ServerNotification;
use cocode_app_server_protocol::SessionListResult;
use cocode_config::ConfigManager;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::connection::ConnectionId;
use crate::connection::ConnectionState;
use crate::error_code::ALREADY_INITIALIZED_ERROR_CODE;
use crate::error_code::INVALID_PARAMS_ERROR_CODE;
use crate::error_code::NOT_INITIALIZED_ERROR_CODE;
use crate::error_code::OVERLOADED_ERROR_CODE;
use crate::session_factory;
use crate::session_factory::SessionHandle;
use crate::turn_runner;
use crate::turn_runner::TurnOutcome;

/// Messages the processor sends to the transport for a specific connection.
pub enum OutboundMessage {
    /// Fire-and-forget notification.
    Notification(ServerNotification),
    /// Response to a client request.
    Response {
        id: RequestId,
        result: serde_json::Value,
    },
    /// Error response to a client request.
    Error {
        id: RequestId,
        code: i64,
        message: String,
    },
    /// Server-initiated request requiring a client response.
    ServerRequest {
        id: RequestId,
        request: cocode_app_server_protocol::ServerRequest,
    },
}

/// Manages message dispatch and per-connection session state.
pub struct Processor {
    /// Per-connection outbound channels and lifecycle state.
    connections: HashMap<ConnectionId, ConnectionEntry>,
    /// Active sessions per connection.
    sessions: HashMap<ConnectionId, SessionHandle>,
    /// Shared config manager for creating sessions.
    config: ConfigManager,
    /// Counter for server-initiated request IDs.
    request_counter: Arc<AtomicI64>,
    /// Whether the processor is draining (rejecting new sessions).
    draining: bool,
}

/// Combined per-connection state: outbound writer + connection flags.
struct ConnectionEntry {
    writer: mpsc::Sender<OutboundMessage>,
    state: ConnectionState,
}

impl Processor {
    pub fn new(config: ConfigManager) -> Self {
        Self {
            connections: HashMap::new(),
            sessions: HashMap::new(),
            config,
            request_counter: Arc::new(AtomicI64::new(1)),
            draining: false,
        }
    }

    pub fn on_connection_opened(
        &mut self,
        conn_id: ConnectionId,
        writer: mpsc::Sender<OutboundMessage>,
    ) {
        self.connections.insert(
            conn_id,
            ConnectionEntry {
                writer,
                state: ConnectionState::new(),
            },
        );
    }

    pub fn on_connection_closed(&mut self, conn_id: ConnectionId) {
        self.connections.remove(&conn_id);
        self.sessions.remove(&conn_id);
    }

    /// Initiate graceful drain: cancel active sessions, reject new ones.
    pub fn initiate_drain(&mut self) {
        self.draining = true;
        for handle in self.sessions.values() {
            handle.state.cancel_token().cancel();
        }
        info!(sessions = self.sessions.len(), "Drain initiated");
    }

    /// Number of active sessions (for shutdown coordination).
    pub fn active_session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Number of connected clients.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Whether a connection exists.
    pub fn has_connection(&self, conn_id: ConnectionId) -> bool {
        self.connections.contains_key(&conn_id)
    }

    /// Handle an inbound JSON-RPC message from a connection.
    pub async fn handle_message(&mut self, conn_id: ConnectionId, message: JsonRpcMessage) {
        match message {
            JsonRpcMessage::Request(request) => {
                self.handle_request(conn_id, request).await;
            }
            JsonRpcMessage::Notification(notification) => {
                debug!(%conn_id, method = notification.method, "Received notification");
            }
            JsonRpcMessage::Response(response) => {
                // Server request responses are routed through ApprovalResolve /
                // UserInputResolve, not through the JSON-RPC response path.
                debug!(%conn_id, id = %response.id, "Received response");
            }
            JsonRpcMessage::Error(error) => {
                warn!(
                    %conn_id, id = %error.id,
                    code = error.error.code, message = error.error.message,
                    "Received error"
                );
            }
        }
    }

    async fn handle_request(&mut self, conn_id: ConnectionId, request: JsonRpcRequest) {
        let id = request.id.clone();

        let client_request = match self.parse_client_request(&request) {
            Ok(req) => req,
            Err(err_msg) => {
                self.send_error(conn_id, id, INVALID_PARAMS_ERROR_CODE, err_msg)
                    .await;
                return;
            }
        };

        let bypass_init = matches!(
            client_request,
            ClientRequest::Initialize(_)
                | ClientRequest::SessionStart(_)
                | ClientRequest::SessionResume(_)
        );
        if !bypass_init && !self.is_initialized(conn_id) {
            self.send_error(
                conn_id,
                id,
                NOT_INITIALIZED_ERROR_CODE,
                "Not initialized: send initialize or session/start first".into(),
            )
            .await;
            return;
        }

        self.dispatch_request(conn_id, id, client_request).await;
    }

    async fn dispatch_request(
        &mut self,
        conn_id: ConnectionId,
        id: RequestId,
        request: ClientRequest,
    ) {
        match request {
            ClientRequest::Initialize(params) => {
                if self.is_initialized(conn_id) {
                    self.send_error(
                        conn_id,
                        id,
                        ALREADY_INITIALIZED_ERROR_CODE,
                        "Already initialized".into(),
                    )
                    .await;
                    return;
                }
                if let Some(entry) = self.connections.get_mut(&conn_id) {
                    entry.state.initialized = true;
                    if let Some(ref caps) = params.capabilities {
                        entry.state.experimental_api = caps.experimental_api;
                        if let Some(ref opt_outs) = caps.opt_out_notification_methods {
                            entry.state.opt_out_notifications = opt_outs.iter().cloned().collect();
                        }
                    }
                }
                let result = InitializeResult {
                    protocol_version: "2".to_string(),
                    platform_family: if cfg!(unix) { "unix" } else { "windows" }.to_string(),
                    platform_os: std::env::consts::OS.to_string(),
                };
                self.send_response(
                    conn_id,
                    id,
                    serde_json::to_value(result).unwrap_or_default(),
                )
                .await;
                info!(%conn_id, "Connection initialized");
            }

            ClientRequest::SessionStart(params) => {
                self.mark_initialized(conn_id);
                if self.draining {
                    self.send_error(
                        conn_id,
                        id,
                        OVERLOADED_ERROR_CODE,
                        "Server is draining; retry later".into(),
                    )
                    .await;
                    return;
                }
                match session_factory::create_session(&self.config, &params).await {
                    Ok((state, hook_bridge)) => {
                        let session_id = state.session.id.clone();
                        let model = state.model().to_string();
                        self.sessions.insert(
                            conn_id,
                            SessionHandle {
                                state,
                                hook_bridge,
                                permission_bridge: None,
                                turn_number: 0,
                            },
                        );
                        self.send_response(
                            conn_id,
                            id,
                            serde_json::json!({"session_id": &session_id}),
                        )
                        .await;
                        self.send_notification(
                            conn_id,
                            ServerNotification::SessionStarted(
                                cocode_app_server_protocol::SessionStartedParams {
                                    session_id,
                                    protocol_version: "2".to_string(),
                                    models: Some(vec![model]),
                                    commands: None,
                                },
                            ),
                        );
                        info!(%conn_id, "Session started");
                        if !params.prompt.is_empty() {
                            self.run_turn(conn_id, params.prompt).await;
                        }
                    }
                    Err(e) => {
                        self.send_error(
                            conn_id,
                            id,
                            INVALID_PARAMS_ERROR_CODE,
                            format!("Failed to create session: {e:#}"),
                        )
                        .await;
                    }
                }
            }

            ClientRequest::SessionResume(_params) => {
                self.mark_initialized(conn_id);
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::TurnStart(params) => {
                if !self.sessions.contains_key(&conn_id) {
                    self.send_error(
                        conn_id,
                        id,
                        INVALID_PARAMS_ERROR_CODE,
                        "No active session".into(),
                    )
                    .await;
                    return;
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "accepted"}))
                    .await;
                self.run_turn(conn_id, params.text).await;
            }

            ClientRequest::TurnInterrupt(_) => {
                if let Some(handle) = self.sessions.get(&conn_id) {
                    handle.state.cancel_token().cancel();
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::ApprovalResolve(params) => {
                if let Some(handle) = self.sessions.get(&conn_id)
                    && let Some(ref bridge) = handle.permission_bridge
                {
                    bridge.resolve(&params.request_id, &params.decision).await;
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::UserInputResolve(params) => {
                if let Some(handle) = self.sessions.get(&conn_id) {
                    let delivered = handle
                        .state
                        .question_responder()
                        .respond(&params.request_id, params.response);
                    if !delivered {
                        warn!(
                            request_id = params.request_id,
                            "Response for unknown request"
                        );
                    }
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::SetModel(params) => {
                if let Some(handle) = self.sessions.get_mut(&conn_id) {
                    handle.state.set_model_override(&params.model);
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::SetPermissionMode(params) => {
                if let Some(handle) = self.sessions.get_mut(&conn_id) {
                    handle.state.set_permission_mode_from_str(&params.mode);
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::StopTask(params) => {
                if let Some(handle) = self.sessions.get(&conn_id) {
                    handle.state.cancel_background_task(&params.task_id).await;
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::SetThinking(params) => {
                if let Some(handle) = self.sessions.get_mut(&conn_id) {
                    use cocode_protocol::ThinkingLevel;
                    let level = match params.thinking.mode {
                        cocode_app_server_protocol::ThinkingMode::Enabled => ThinkingLevel::high(),
                        cocode_app_server_protocol::ThinkingMode::Disabled => ThinkingLevel::none(),
                        cocode_app_server_protocol::ThinkingMode::Adaptive => {
                            ThinkingLevel::medium()
                        }
                    };
                    handle
                        .state
                        .switch_thinking_level(cocode_protocol::ModelRole::Main, level);
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::UpdateEnv(params) => {
                if let Some(handle) = self.sessions.get_mut(&conn_id) {
                    handle.state.apply_sdk_env_overrides(&params.env);
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::KeepAlive(_) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                self.send_notification(
                    conn_id,
                    ServerNotification::KeepAlive(cocode_app_server_protocol::KeepAliveParams {
                        timestamp: ts,
                    }),
                );
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::SessionList(params) => {
                let limit = params.limit.unwrap_or(50);
                debug!(%conn_id, limit, "Session list requested");
                let result = SessionListResult {
                    sessions: vec![],
                    next_cursor: None,
                };
                self.send_response(
                    conn_id,
                    id,
                    serde_json::to_value(result).unwrap_or_default(),
                )
                .await;
            }

            ClientRequest::SessionRead(params) => {
                debug!(%conn_id, session_id = params.session_id, "Session read");
                self.send_response(conn_id, id, serde_json::json!({"items": []}))
                    .await;
            }

            ClientRequest::SessionArchive(params) => {
                debug!(%conn_id, session_id = params.session_id, "Session archive");
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::ConfigRead(_) => {
                self.send_response(conn_id, id, serde_json::json!({"config": {}}))
                    .await;
            }

            ClientRequest::ConfigWrite(params) => {
                debug!(%conn_id, key = params.key, "Config write");
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::HookCallbackResponse(params) => {
                if let Some(handle) = self.sessions.get(&conn_id)
                    && let Some(ref bridge) = handle.hook_bridge
                {
                    bridge.resolve(&params.request_id, params.output).await;
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::RewindFiles(_) => {
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::McpRouteMessageResponse(params) => {
                debug!(%conn_id, "MCP route message response (wiring deferred)");
                let _ = params;
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }

            ClientRequest::CancelRequest(params) => {
                if let Some(handle) = self.sessions.get(&conn_id)
                    && let Some(ref bridge) = handle.hook_bridge
                {
                    bridge
                        .resolve(&params.request_id, serde_json::Value::Null)
                        .await;
                }
                self.send_response(conn_id, id, serde_json::json!({"status": "ok"}))
                    .await;
            }
        }
    }

    /// Run a turn on the active session for a connection.
    async fn run_turn(&mut self, conn_id: ConnectionId, prompt: String) {
        let outbound = match self.connections.get(&conn_id) {
            Some(entry) => entry.writer.clone(),
            None => return,
        };

        let handle = match self.sessions.get_mut(&conn_id) {
            Some(h) => h,
            None => return,
        };

        handle.turn_number += 1;
        let turn_id = format!("turn_{}", handle.turn_number);
        let turn_number = handle.turn_number;

        let turn_result = turn_runner::run_turn(
            &mut handle.state,
            &prompt,
            turn_runner::TurnConfig {
                turn_id,
                turn_number,
                outbound: &outbound,
                hook_bridge: &handle.hook_bridge,
                request_counter: &self.request_counter,
            },
        )
        .await;

        // Store bridge so ApprovalResolve can reach it; clear after turn
        if let Some(h) = self.sessions.get_mut(&conn_id) {
            h.permission_bridge = Some(turn_result.permission_bridge);
        }

        let result_msg = match turn_result.outcome {
            TurnOutcome::Completed(params) => {
                OutboundMessage::Notification(ServerNotification::TurnCompleted(params))
            }
            TurnOutcome::Failed(error) => {
                OutboundMessage::Notification(ServerNotification::TurnFailed(
                    cocode_app_server_protocol::TurnFailedParams { error },
                ))
            }
            TurnOutcome::Interrupted => {
                OutboundMessage::Notification(ServerNotification::TurnInterrupted(
                    cocode_app_server_protocol::TurnInterruptedParams { turn_id: None },
                ))
            }
        };
        if outbound.send(result_msg).await.is_err() {
            warn!(%conn_id, "Outbound channel closed while sending turn result");
        }
    }

    fn parse_client_request(&self, request: &JsonRpcRequest) -> Result<ClientRequest, String> {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "method".into(),
            serde_json::Value::String(request.method.clone()),
        );
        if let Some(params) = &request.params {
            obj.insert("params".into(), params.clone());
        }
        serde_json::from_value::<ClientRequest>(serde_json::Value::Object(obj))
            .map_err(|e| format!("failed to parse request '{}': {e}", request.method))
    }

    fn is_initialized(&self, conn_id: ConnectionId) -> bool {
        self.connections
            .get(&conn_id)
            .is_some_and(|e| e.state.initialized)
    }

    fn mark_initialized(&mut self, conn_id: ConnectionId) {
        if let Some(entry) = self.connections.get_mut(&conn_id) {
            entry.state.initialized = true;
        }
    }

    /// Send a response (critical — awaits delivery).
    async fn send_response(&self, conn_id: ConnectionId, id: RequestId, result: serde_json::Value) {
        let Some(entry) = self.connections.get(&conn_id) else {
            return;
        };
        if entry
            .writer
            .send(OutboundMessage::Response { id, result })
            .await
            .is_err()
        {
            warn!(%conn_id, "Outbound channel closed while sending response");
        }
    }

    /// Send an error response (critical — awaits delivery).
    async fn send_error(&self, conn_id: ConnectionId, id: RequestId, code: i64, message: String) {
        let Some(entry) = self.connections.get(&conn_id) else {
            return;
        };
        if entry
            .writer
            .send(OutboundMessage::Error { id, code, message })
            .await
            .is_err()
        {
            warn!(%conn_id, "Outbound channel closed while sending error");
        }
    }

    /// Send a notification (non-critical — uses try_send with warning).
    fn send_notification(&self, conn_id: ConnectionId, notification: ServerNotification) {
        let Some(entry) = self.connections.get(&conn_id) else {
            return;
        };
        if entry
            .writer
            .try_send(OutboundMessage::Notification(notification))
            .is_err()
        {
            warn!(%conn_id, "Outbound channel full, dropping notification");
        }
    }
}
