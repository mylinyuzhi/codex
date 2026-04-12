//! CLI transport layer for SDK communication.
//!
//! TS: cli/transports/SSETransport.ts, WebSocketTransport.ts,
//! HybridTransport.ts, transportUtils.ts
//!
//! Defines the `Transport` trait and three implementations:
//! - `StdioTransport`: NDJSON over stdin/stdout (primary SDK transport)
//! - `SseTransport`: Server-Sent Events (stub for remote bridge)
//! - `WebSocketTransport`: WebSocket (stub for remote bridge)

use std::collections::VecDeque;

use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::sync::mpsc;
use tracing::info;
use tracing::warn;

// ---------------------------------------------------------------------------
// Transport Events
// ---------------------------------------------------------------------------

/// Event sent from agent to SDK consumer.
///
/// TS: StdoutMessage — the wire format for all outbound SDK messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportEvent {
    /// Assistant text chunk (streaming delta).
    StreamEvent { content: String },
    /// Complete assistant message.
    AssistantMessage { text: String },
    /// Tool execution started.
    ToolUseStart {
        tool_use_id: String,
        tool_name: String,
    },
    /// Tool execution completed.
    ToolUseEnd {
        tool_use_id: String,
        tool_name: String,
        is_error: bool,
    },
    /// System information.
    SystemInfo { message: String },
    /// Session metadata.
    SessionMeta {
        session_id: String,
        model: String,
        cwd: String,
    },
    /// Usage statistics for the turn.
    Usage {
        input_tokens: i64,
        output_tokens: i64,
        cost_usd: f64,
    },
    /// Session result (final output).
    Result { text: String, turns: i32 },
    /// Error message.
    Error { message: String },
    /// Keepalive.
    KeepAlive,
}

/// Input from SDK consumer to agent.
///
/// TS: cli/transports/Transport.ts — onData callback payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportInput {
    /// User prompt submission.
    UserMessage { text: String },
    /// Interrupt current turn.
    Interrupt,
    /// Permission decision for a pending tool use.
    PermissionResponse { request_id: String, approved: bool },
    /// Keepalive pong.
    Pong,
}

// ---------------------------------------------------------------------------
// Transport State
// ---------------------------------------------------------------------------

/// Connection state of a transport.
///
/// TS: WebSocketTransport — state machine: idle → connected → reconnecting → closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    /// Not yet connected.
    Idle,
    /// Connected and active.
    Connected,
    /// Disconnected, attempting reconnection.
    Reconnecting,
    /// Connection closing gracefully.
    Closing,
    /// Permanently closed.
    Closed,
}

impl TransportState {
    /// Human-readable label for the state.
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
            Self::Closing => "closing",
            Self::Closed => "closed",
        }
    }
}

// ---------------------------------------------------------------------------
// Transport Trait
// ---------------------------------------------------------------------------

/// Trait for sending events and receiving input across different transports.
///
/// TS: cli/transports/Transport.ts — Transport interface.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Send an event to the SDK consumer.
    async fn send_event(&self, event: TransportEvent) -> anyhow::Result<()>;

    /// Send a batch of events atomically.
    async fn send_batch(&self, events: Vec<TransportEvent>) -> anyhow::Result<()> {
        for event in events {
            self.send_event(event).await?;
        }
        Ok(())
    }

    /// Current connection state.
    fn state(&self) -> TransportState;

    /// Whether the transport is connected.
    fn is_connected(&self) -> bool {
        self.state() == TransportState::Connected
    }

    /// Close the transport.
    async fn close(&self) -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------------
// Stdio Transport (NDJSON)
// ---------------------------------------------------------------------------

/// NDJSON transport over stdin/stdout.
///
/// This is the primary transport for SDK consumers using `--json` mode.
/// Each message is a single JSON line terminated by `\n`.
///
/// TS: cli/structuredIO.ts — NDJSON protocol for print mode.
pub struct StdioTransport {
    /// Sender for outbound events (written to stdout).
    stdout_tx: mpsc::Sender<String>,
    /// Receiver for inbound input (read from stdin). Taken by `run_reader`.
    stdin_rx: Option<mpsc::Receiver<TransportInput>>,
    /// Sender for injecting input (used by the stdin reader task).
    stdin_tx: mpsc::Sender<TransportInput>,
    /// Transport state.
    state: std::sync::atomic::AtomicU8,
}

impl StdioTransport {
    /// Create a new stdio transport. Spawns a background writer task.
    pub fn new() -> Self {
        let (stdout_tx, mut stdout_rx) = mpsc::channel::<String>(256);
        let (stdin_tx, stdin_rx) = mpsc::channel(256);

        // Writer task: serialize events to stdout
        tokio::spawn(async move {
            let mut stdout = tokio::io::stdout();
            while let Some(line) = stdout_rx.recv().await {
                if stdout.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if stdout.flush().await.is_err() {
                    break;
                }
            }
        });

        Self {
            stdout_tx,
            stdin_rx: Some(stdin_rx),
            stdin_tx,
            state: std::sync::atomic::AtomicU8::new(TransportState::Connected as u8),
        }
    }

    /// Take the input receiver (agent loop consumes this).
    pub fn take_input_receiver(&mut self) -> Option<mpsc::Receiver<TransportInput>> {
        self.stdin_rx.take()
    }

    /// Get a sender for injecting input (for testing).
    pub fn input_sender(&self) -> mpsc::Sender<TransportInput> {
        self.stdin_tx.clone()
    }

    /// Spawn a reader task that parses NDJSON from stdin.
    pub fn spawn_reader(&self) {
        let tx = self.stdin_tx.clone();
        tokio::spawn(async move {
            let stdin = tokio::io::stdin();
            let mut reader = BufReader::new(stdin);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<TransportInput>(trimmed) {
                            Ok(input) => {
                                if tx.send(input).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "failed to parse stdin input");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "stdin read error");
                        break;
                    }
                }
            }
        });
    }

    fn load_state(&self) -> TransportState {
        match self.state.load(std::sync::atomic::Ordering::SeqCst) {
            1 => TransportState::Connected,
            4 => TransportState::Closed,
            _ => TransportState::Idle,
        }
    }
}

#[async_trait::async_trait]
impl Transport for StdioTransport {
    async fn send_event(&self, event: TransportEvent) -> anyhow::Result<()> {
        let json = serde_json::to_string(&event)?;
        let line = format!("{json}\n");
        self.stdout_tx
            .send(line)
            .await
            .map_err(|_| anyhow::anyhow!("stdout channel closed"))?;
        Ok(())
    }

    fn state(&self) -> TransportState {
        self.load_state()
    }

    async fn close(&self) -> anyhow::Result<()> {
        self.state.store(
            TransportState::Closed as u8,
            std::sync::atomic::Ordering::SeqCst,
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SSE Transport (Stub)
// ---------------------------------------------------------------------------

/// SSE transport configuration.
///
/// TS: SSETransport.ts — configuration constants.
#[derive(Debug, Clone)]
pub struct SseConfig {
    /// Base URL for the SSE endpoint.
    pub url: String,
    /// Authorization headers.
    pub headers: std::collections::HashMap<String, String>,
    /// Reconnection base delay in milliseconds.
    pub reconnect_base_delay_ms: i64,
    /// Reconnection maximum delay in milliseconds.
    pub reconnect_max_delay_ms: i64,
    /// Liveness timeout in milliseconds (server keepalive detection).
    pub liveness_timeout_ms: i64,
}

impl Default for SseConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            headers: std::collections::HashMap::new(),
            reconnect_base_delay_ms: 1000,
            reconnect_max_delay_ms: 30_000,
            liveness_timeout_ms: 45_000,
        }
    }
}

/// SSE frame parsed from the event stream.
///
/// TS: SSETransport.ts — SSEFrame type and parseSSEFrames.
#[derive(Debug, Clone, Default)]
pub struct SseFrame {
    pub event: Option<String>,
    pub id: Option<String>,
    pub data: Option<String>,
}

/// Parse SSE frames from a text buffer.
///
/// Returns parsed frames and the remaining (incomplete) buffer text.
///
/// TS: SSETransport.ts — parseSSEFrames (exported for testing).
pub fn parse_sse_frames(buffer: &str) -> (Vec<SseFrame>, String) {
    let mut frames = Vec::new();
    let mut pos = 0;

    while let Some(idx) = buffer[pos..].find("\n\n") {
        let raw_frame = &buffer[pos..pos + idx];
        pos += idx + 2;

        let trimmed = raw_frame.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut frame = SseFrame::default();
        let mut is_comment_only = true;

        for line in trimmed.lines() {
            if line.starts_with(':') {
                // SSE comment (e.g. `:keepalive`)
                continue;
            }
            is_comment_only = false;

            if let Some(value) = line.strip_prefix("event:") {
                frame.event = Some(value.trim().to_string());
            } else if let Some(value) = line.strip_prefix("id:") {
                frame.id = Some(value.trim().to_string());
            } else if let Some(value) = line.strip_prefix("data:") {
                let data_val = value.trim_start().to_string();
                frame.data = Some(match frame.data.take() {
                    Some(existing) => format!("{existing}\n{data_val}"),
                    None => data_val,
                });
            }
        }

        if !is_comment_only {
            frames.push(frame);
        }
    }

    let remaining = buffer[pos..].to_string();
    (frames, remaining)
}

/// Server-Sent Events transport (stub).
///
/// TS: SSETransport.ts — full implementation with reconnection,
/// POST for writes, last-event-id tracking.
///
/// This stub provides the type structure and SSE frame parsing.
/// Full HTTP streaming and reconnection logic will be added when
/// the remote bridge feature is enabled.
pub struct SseTransport {
    config: SseConfig,
    /// Sender for dispatching received events (used when read loop is implemented).
    _event_tx: mpsc::Sender<TransportEvent>,
    state: std::sync::atomic::AtomicU8,
    /// Outbound message buffer for POST delivery.
    outbound: tokio::sync::Mutex<VecDeque<TransportEvent>>,
    /// Last received SSE event ID for reconnection.
    last_event_id: tokio::sync::Mutex<Option<String>>,
}

impl SseTransport {
    /// Create a new SSE transport with the given configuration.
    pub fn new(config: SseConfig) -> (Self, mpsc::Receiver<TransportEvent>) {
        let (event_tx, event_rx) = mpsc::channel(256);
        let transport = Self {
            config,
            _event_tx: event_tx,
            state: std::sync::atomic::AtomicU8::new(TransportState::Idle as u8),
            outbound: tokio::sync::Mutex::new(VecDeque::new()),
            last_event_id: tokio::sync::Mutex::new(None),
        };
        (transport, event_rx)
    }

    /// Get the configured URL.
    pub fn url(&self) -> &str {
        &self.config.url
    }

    /// Get the last received event ID.
    pub async fn last_event_id(&self) -> Option<String> {
        self.last_event_id.lock().await.clone()
    }

    fn load_state(&self) -> TransportState {
        match self.state.load(std::sync::atomic::Ordering::SeqCst) {
            1 => TransportState::Connected,
            2 => TransportState::Reconnecting,
            4 => TransportState::Closed,
            _ => TransportState::Idle,
        }
    }
}

#[async_trait::async_trait]
impl Transport for SseTransport {
    async fn send_event(&self, event: TransportEvent) -> anyhow::Result<()> {
        // Stub: buffer events for POST delivery
        self.outbound.lock().await.push_back(event);
        Ok(())
    }

    fn state(&self) -> TransportState {
        self.load_state()
    }

    async fn close(&self) -> anyhow::Result<()> {
        self.state.store(
            TransportState::Closed as u8,
            std::sync::atomic::Ordering::SeqCst,
        );
        info!("SSE transport closed");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// WebSocket Transport (Stub)
// ---------------------------------------------------------------------------

/// WebSocket transport configuration.
///
/// TS: WebSocketTransport.ts — constructor options.
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// WebSocket URL (ws:// or wss://).
    pub url: String,
    /// Authorization headers for the upgrade request.
    pub headers: std::collections::HashMap<String, String>,
    /// Ping interval in milliseconds.
    pub ping_interval_ms: i64,
    /// Reconnection base delay.
    pub reconnect_base_delay_ms: i64,
    /// Reconnection maximum delay.
    pub reconnect_max_delay_ms: i64,
    /// Reconnection give-up timeout.
    pub reconnect_give_up_ms: i64,
    /// Whether to attempt automatic reconnection.
    pub auto_reconnect: bool,
    /// Keepalive interval in milliseconds.
    pub keepalive_interval_ms: i64,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            headers: std::collections::HashMap::new(),
            ping_interval_ms: 10_000,
            reconnect_base_delay_ms: 1_000,
            reconnect_max_delay_ms: 30_000,
            reconnect_give_up_ms: 600_000,
            auto_reconnect: true,
            keepalive_interval_ms: 300_000,
        }
    }
}

/// WebSocket close codes indicating permanent server rejection.
///
/// TS: WebSocketTransport.ts — PERMANENT_CLOSE_CODES.
pub const PERMANENT_CLOSE_CODES: &[i32] = &[
    1002, // protocol error
    4001, // session expired / not found
    4003, // unauthorized
];

/// WebSocket transport (stub).
///
/// TS: WebSocketTransport.ts — full implementation with reconnection,
/// keepalive, sleep detection, circular buffer.
///
/// This stub provides the type structure and configuration.
/// Full WebSocket logic will be added when the remote bridge
/// feature is enabled.
pub struct WebSocketTransport {
    config: WebSocketConfig,
    /// Sender for dispatching received events (used when WS read loop is implemented).
    _event_tx: mpsc::Sender<TransportEvent>,
    state: std::sync::atomic::AtomicU8,
    /// Outbound message buffer.
    outbound: tokio::sync::Mutex<VecDeque<TransportEvent>>,
    /// Maximum messages to buffer when disconnected.
    max_buffer_size: usize,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport with the given configuration.
    pub fn new(config: WebSocketConfig) -> (Self, mpsc::Receiver<TransportEvent>) {
        let (event_tx, event_rx) = mpsc::channel(256);
        let transport = Self {
            config,
            _event_tx: event_tx,
            state: std::sync::atomic::AtomicU8::new(TransportState::Idle as u8),
            outbound: tokio::sync::Mutex::new(VecDeque::new()),
            max_buffer_size: 1000,
        };
        (transport, event_rx)
    }

    /// Get the configured URL.
    pub fn url(&self) -> &str {
        &self.config.url
    }

    /// Whether this close code indicates permanent rejection.
    pub fn is_permanent_close(code: i32) -> bool {
        PERMANENT_CLOSE_CODES.contains(&code)
    }

    fn load_state(&self) -> TransportState {
        match self.state.load(std::sync::atomic::Ordering::SeqCst) {
            1 => TransportState::Connected,
            2 => TransportState::Reconnecting,
            4 => TransportState::Closed,
            _ => TransportState::Idle,
        }
    }
}

#[async_trait::async_trait]
impl Transport for WebSocketTransport {
    async fn send_event(&self, event: TransportEvent) -> anyhow::Result<()> {
        let mut buf = self.outbound.lock().await;
        if buf.len() >= self.max_buffer_size {
            buf.pop_front();
        }
        buf.push_back(event);
        Ok(())
    }

    fn state(&self) -> TransportState {
        self.load_state()
    }

    async fn close(&self) -> anyhow::Result<()> {
        self.state.store(
            TransportState::Closed as u8,
            std::sync::atomic::Ordering::SeqCst,
        );
        info!("WebSocket transport closed");
        Ok(())
    }
}

#[cfg(test)]
#[path = "transport.test.rs"]
mod tests;
