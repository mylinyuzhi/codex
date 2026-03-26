//! Transport layer: Axum WebSocket server and stdio NDJSON.
//!
//! Handles low-level I/O for both transport modes and emits
//! `TransportEvent`s for the processor loop to consume.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::extract::State;
use axum::extract::ws::Message as WsMessage;
use axum::extract::ws::WebSocket;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::Request;
use axum::http::StatusCode;
use axum::http::header::ORIGIN;
use axum::middleware;
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::any;
use axum::routing::get;
use cocode_app_server_protocol::JsonRpcErrorData;
use cocode_app_server_protocol::JsonRpcMessage;
use cocode_app_server_protocol::JsonRpcNotification;
use cocode_app_server_protocol::JsonRpcRequest;
use cocode_app_server_protocol::JsonRpcResponse;

use crate::processor::OutboundMessage;
use futures::SinkExt;
use futures::StreamExt;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::connection::ConnectionId;
use crate::connection::ConnectionIdGenerator;
use crate::connection::OUTBOUND_CHANNEL_CAPACITY;

/// Events from the transport layer to the processor.
pub enum TransportEvent {
    /// A new client connected.
    ConnectionOpened {
        connection_id: ConnectionId,
        writer: mpsc::Sender<OutboundMessage>,
    },
    /// A client disconnected.
    ConnectionClosed { connection_id: ConnectionId },
    /// An inbound JSON-RPC message from a client.
    IncomingMessage {
        connection_id: ConnectionId,
        message: JsonRpcMessage,
    },
}

/// Transport mode for the app-server (parsed from `--listen` URL).
#[derive(Debug, Clone)]
pub enum AppServerTransport {
    /// NDJSON over stdin/stdout (single client).
    Stdio,
    /// WebSocket on the given bind address.
    WebSocket { bind_address: SocketAddr },
}

impl AppServerTransport {
    pub const DEFAULT_LISTEN_URL: &'static str = "stdio://";
}

impl std::str::FromStr for AppServerTransport {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "stdio://" || s == "stdio" {
            return Ok(Self::Stdio);
        }
        if let Some(addr) = s.strip_prefix("ws://") {
            let socket_addr: SocketAddr = addr
                .parse()
                .map_err(|e| format!("invalid WebSocket address '{addr}': {e}"))?;
            return Ok(Self::WebSocket {
                bind_address: socket_addr,
            });
        }
        Err(format!(
            "unsupported transport URL: '{s}' (expected stdio:// or ws://IP:PORT)"
        ))
    }
}

// ---------------------------------------------------------------------------
// stdio transport
// ---------------------------------------------------------------------------

/// Start the stdio transport (single connection, NDJSON).
pub async fn start_stdio_transport(
    event_tx: mpsc::Sender<TransportEvent>,
) -> anyhow::Result<Vec<JoinHandle<()>>> {
    let (msg_tx, mut msg_rx) = mpsc::channel::<OutboundMessage>(OUTBOUND_CHANNEL_CAPACITY);

    // Notify processor that stdio connection is open
    event_tx
        .send(TransportEvent::ConnectionOpened {
            connection_id: ConnectionId::STDIO,
            writer: msg_tx,
        })
        .await
        .map_err(|_| anyhow::anyhow!("processor channel closed"))?;

    // Spawn stdin reader
    let event_tx_clone = event_tx.clone();
    let reader_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(tokio::io::stdin());
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    info!("stdin closed (EOF)");
                    break;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<JsonRpcMessage>(trimmed) {
                        Ok(msg) => {
                            if event_tx_clone
                                .send(TransportEvent::IncomingMessage {
                                    connection_id: ConnectionId::STDIO,
                                    message: msg,
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("failed to parse stdin JSON: {e}");
                        }
                    }
                }
                Err(e) => {
                    error!("stdin read error: {e}");
                    break;
                }
            }
        }
        let _ = event_tx_clone
            .send(TransportEvent::ConnectionClosed {
                connection_id: ConnectionId::STDIO,
            })
            .await;
    });

    // Spawn stdout writer
    let writer_handle = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(msg) = msg_rx.recv().await {
            let json = match serialize_outbound(&msg) {
                Ok(j) => j,
                Err(e) => {
                    warn!("failed to serialize outbound message: {e}");
                    continue;
                }
            };
            if stdout.write_all(json.as_bytes()).await.is_err()
                || stdout.write_all(b"\n").await.is_err()
                || stdout.flush().await.is_err()
            {
                break;
            }
        }
    });

    Ok(vec![reader_handle, writer_handle])
}

// ---------------------------------------------------------------------------
// WebSocket transport
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct WsState {
    event_tx: mpsc::Sender<TransportEvent>,
    id_gen: std::sync::Arc<ConnectionIdGenerator>,
}

/// Start the WebSocket acceptor on the given address.
pub async fn start_websocket_transport(
    bind_address: SocketAddr,
    event_tx: mpsc::Sender<TransportEvent>,
    shutdown: CancellationToken,
) -> anyhow::Result<JoinHandle<()>> {
    let state = WsState {
        event_tx,
        id_gen: std::sync::Arc::new(ConnectionIdGenerator::new()),
    };

    let app = Router::new()
        .route("/", any(ws_upgrade_handler))
        .route("/readyz", get(health_handler))
        .route("/healthz", get(health_handler))
        .layer(middleware::from_fn(reject_origin_header))
        .with_state(state);

    let listener = TcpListener::bind(bind_address).await?;
    let local_addr = listener.local_addr()?;
    print_ws_banner(local_addr);

    let handle = tokio::spawn(async move {
        let server = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown.cancelled_owned());
        if let Err(e) = server.await {
            error!("WebSocket server error: {e}");
        }
        info!("WebSocket server shut down");
    });

    Ok(handle)
}

async fn health_handler() -> StatusCode {
    StatusCode::OK
}

/// Reject HTTP requests with an `Origin` header (browser CORS protection).
async fn reject_origin_header(req: Request<Body>, next: Next) -> Response {
    if req.headers().contains_key(ORIGIN) {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(req).await
}

async fn ws_upgrade_handler(
    ws: WebSocketUpgrade,
    State(state): State<WsState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state, addr))
}

async fn handle_ws_connection(socket: WebSocket, state: WsState, addr: SocketAddr) {
    let conn_id = state.id_gen.next();
    info!(%conn_id, %addr, "WebSocket connection opened");

    let (msg_tx, mut msg_rx) = mpsc::channel::<OutboundMessage>(OUTBOUND_CHANNEL_CAPACITY);

    // Notify processor of new connection
    if state
        .event_tx
        .send(TransportEvent::ConnectionOpened {
            connection_id: conn_id,
            writer: msg_tx,
        })
        .await
        .is_err()
    {
        return;
    }

    let (mut ws_sink, mut ws_stream) = socket.split();
    let event_tx = state.event_tx.clone();

    // Outbound: forward messages to WebSocket frames
    let outbound = tokio::spawn(async move {
        while let Some(msg) = msg_rx.recv().await {
            let json = match serialize_outbound(&msg) {
                Ok(j) => j,
                Err(e) => {
                    warn!("failed to serialize outbound message: {e}");
                    continue;
                }
            };
            if ws_sink.send(WsMessage::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // Inbound: read WebSocket frames and forward as TransportEvents
    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(WsMessage::Text(text)) => match serde_json::from_str::<JsonRpcMessage>(&text) {
                Ok(parsed) => {
                    if event_tx
                        .send(TransportEvent::IncomingMessage {
                            connection_id: conn_id,
                            message: parsed,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    warn!(%conn_id, "invalid JSON from WebSocket: {e}");
                }
            },
            Ok(WsMessage::Close(_)) => break,
            Ok(_) => {} // Ignore ping/pong/binary
            Err(e) => {
                debug!(%conn_id, "WebSocket read error: {e}");
                break;
            }
        }
    }

    outbound.abort();
    let _ = event_tx
        .send(TransportEvent::ConnectionClosed {
            connection_id: conn_id,
        })
        .await;
    info!(%conn_id, %addr, "WebSocket connection closed");
}

/// Serialize an `OutboundMessage` into a JSON string suitable for the wire.
fn serialize_outbound(msg: &OutboundMessage) -> Result<String, serde_json::Error> {
    match msg {
        OutboundMessage::Notification(notif) => {
            let value = serde_json::to_value(notif)?;
            let rpc = JsonRpcNotification {
                method: value["method"].as_str().unwrap_or("unknown").to_string(),
                params: value.get("params").cloned(),
            };
            serde_json::to_string(&rpc)
        }
        OutboundMessage::Response { id, result } => {
            let rpc = JsonRpcResponse {
                id: id.clone(),
                result: result.clone(),
            };
            serde_json::to_string(&rpc)
        }
        OutboundMessage::Error { id, code, message } => {
            let rpc = cocode_app_server_protocol::JsonRpcError {
                id: id.clone(),
                error: JsonRpcErrorData {
                    code: *code,
                    message: message.clone(),
                    data: None,
                },
            };
            serde_json::to_string(&rpc)
        }
        OutboundMessage::ServerRequest { id, request } => {
            let value = serde_json::to_value(request)?;
            let rpc = JsonRpcRequest {
                id: id.clone(),
                method: value["method"].as_str().unwrap_or("unknown").to_string(),
                params: value.get("params").cloned(),
            };
            serde_json::to_string(&rpc)
        }
    }
}

fn print_ws_banner(addr: SocketAddr) {
    eprintln!("cocode app-server (WebSockets)");
    eprintln!("  listening on: ws://{addr}");
    eprintln!("  readyz:       http://{addr}/readyz");
    eprintln!("  healthz:      http://{addr}/healthz");
    if addr.ip().is_loopback() {
        eprintln!("  note: binds localhost only (use SSH port-forwarding for remote access)");
    } else {
        eprintln!("  note: raw WS server; consider TLS/auth for production use");
    }
}
