//! REPL bridge for headless/non-TUI communication.
//!
//! TS: bridge/replBridge.ts, bridge/replBridgeTransport.ts,
//! bridge/bridgeMessaging.ts
//!
//! Provides input/output message routing and session state
//! synchronization for SDK and daemon callers that bypass the TUI.
//! The REPL bridge sits between the agent loop and a transport
//! (WebSocket, SSE, or NDJSON) and handles bidirectional message
//! serialization, keepalive, and reconnection awareness.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tracing::info;
use tracing::warn;

// ---------------------------------------------------------------------------
// REPL Bridge Messages
// ---------------------------------------------------------------------------

/// Inbound message from an SDK consumer or remote client.
///
/// TS: bridge/bridgeMessaging.ts — handleIngressMessage, isEligibleBridgeMessage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReplInMessage {
    /// User prompt submission.
    UserMessage { text: String },
    /// Control request from SDK (initialize, interrupt, permission, etc.).
    ControlRequest {
        request_id: String,
        #[serde(flatten)]
        request: ControlRequest,
    },
    /// Cancel a previously submitted async message.
    CancelAsyncMessage { message_uuid: String },
    /// Permission response (approve/deny a pending tool use).
    PermissionResponse {
        request_id: String,
        #[serde(flatten)]
        decision: PermissionDecision,
    },
    /// Keepalive ping.
    Ping,
}

/// Outbound message to an SDK consumer or remote client.
///
/// TS: bridge/bridgeMessaging.ts — makeResultMessage, StdoutMessage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReplOutMessage {
    /// Assistant text chunk (streaming).
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
    /// Control request from agent to SDK (permission, input, etc.).
    ControlRequest {
        request_id: String,
        #[serde(flatten)]
        request: SdkControlOutbound,
    },
    /// Control response to an SDK control request.
    ControlResponse {
        request_id: String,
        #[serde(flatten)]
        response: serde_json::Value,
    },
    /// Session result (final output).
    Result {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// Error message.
    Error { message: String },
    /// Keepalive pong.
    Pong,
}

/// SDK control requests (subset relevant to REPL bridge).
///
/// TS: controlSchemas.ts — SDKControlRequest discriminated union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum ControlRequest {
    /// Initialize SDK session.
    Initialize {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system_prompt: Option<String>,
    },
    /// Interrupt current turn.
    Interrupt,
    /// Set model for subsequent turns.
    SetModel {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// Set permission mode.
    SetPermissionMode { mode: String },
    /// Query MCP server status.
    McpStatus,
    /// Get context window usage.
    GetContextUsage,
    /// Rewind files to a previous message.
    RewindFiles {
        user_message_id: String,
        #[serde(default)]
        dry_run: bool,
    },
}

/// Permission decision from SDK consumer.
///
/// TS: controlSchemas.ts — SDKControlPermissionResponse.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
    AllowAlways { scope: String },
}

/// Outbound control requests from agent to SDK.
///
/// TS: controlSchemas.ts — SDKControlPermissionRequest, request_input, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum SdkControlOutbound {
    /// Request permission to use a tool.
    CanUseTool {
        tool_name: String,
        tool_use_id: String,
        input: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// Request user input.
    RequestInput {
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// REPL Bridge State
// ---------------------------------------------------------------------------

/// Connection state of the REPL bridge transport.
///
/// TS: replBridge.ts — BridgeState type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum BridgeState {
    /// Bridge created but not yet connected.
    Idle = 0,
    /// Transport connected and active.
    Connected = 1,
    /// Transport disconnected, attempting reconnection.
    Reconnecting = 2,
    /// Permanently failed (auth rejected, session expired).
    Failed = 3,
}

impl BridgeState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Connected,
            2 => Self::Reconnecting,
            3 => Self::Failed,
            _ => Self::Idle,
        }
    }
}

// ---------------------------------------------------------------------------
// REPL Bridge
// ---------------------------------------------------------------------------

/// Maximum number of messages to buffer when transport is disconnected.
const MAX_OUTBOUND_BUFFER: usize = 1000;

/// REPL bridge for headless/non-TUI communication.
///
/// Routes messages between the agent loop and a transport layer
/// (SSE, WebSocket, NDJSON). Buffers outbound messages during
/// disconnection and drains on reconnect.
///
/// TS: bridge/replBridge.ts — initBridgeCore + ReplBridgeHandle.
pub struct ReplBridge {
    /// Session identifier.
    session_id: String,
    /// Current bridge state (atomic for lock-free reads in `send()`).
    state: AtomicU8,
    /// Watch channel for state change notifications.
    state_notify: watch::Sender<BridgeState>,
    /// Incoming messages from SDK/remote.
    incoming_tx: mpsc::Sender<ReplInMessage>,
    incoming_rx: Option<mpsc::Receiver<ReplInMessage>>,
    /// Outgoing messages to SDK/remote.
    outgoing_tx: mpsc::Sender<ReplOutMessage>,
    outgoing_rx: Option<mpsc::Receiver<ReplOutMessage>>,
    /// Buffered outbound messages during disconnection.
    buffer: Arc<Mutex<VecDeque<ReplOutMessage>>>,
}

impl ReplBridge {
    /// Create a new REPL bridge for the given session.
    pub fn new(session_id: String) -> Self {
        let (incoming_tx, incoming_rx) = mpsc::channel(256);
        let (outgoing_tx, outgoing_rx) = mpsc::channel(256);
        let (state_notify, _) = watch::channel(BridgeState::Idle);

        Self {
            session_id,
            state: AtomicU8::new(BridgeState::Idle as u8),
            state_notify,
            incoming_tx,
            incoming_rx: Some(incoming_rx),
            outgoing_tx,
            outgoing_rx: Some(outgoing_rx),
            buffer: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Get the session identifier.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the current bridge state.
    pub fn state(&self) -> BridgeState {
        BridgeState::from_u8(self.state.load(Ordering::SeqCst))
    }

    /// Subscribe to state changes.
    pub fn watch_state(&self) -> watch::Receiver<BridgeState> {
        self.state_notify.subscribe()
    }

    /// Transition to a new state.
    pub fn set_state(&self, new_state: BridgeState) {
        let old = self.state();
        if old != new_state {
            info!(
                session_id = %self.session_id,
                old = ?old,
                new = ?new_state,
                "REPL bridge state transition"
            );
            self.state.store(new_state as u8, Ordering::SeqCst);
            // Ignore send error -- no receivers is fine.
            let _ = self.state_notify.send(new_state);
        }
    }

    /// Take the incoming message receiver (agent loop consumes this).
    pub fn take_incoming(&mut self) -> Option<mpsc::Receiver<ReplInMessage>> {
        self.incoming_rx.take()
    }

    /// Take the outgoing message receiver (transport consumes this).
    pub fn take_outgoing(&mut self) -> Option<mpsc::Receiver<ReplOutMessage>> {
        self.outgoing_rx.take()
    }

    /// Get a sender for injecting incoming messages.
    pub fn incoming_sender(&self) -> mpsc::Sender<ReplInMessage> {
        self.incoming_tx.clone()
    }

    /// Send a message to the SDK/remote client.
    ///
    /// If the transport is disconnected, buffers the message up to
    /// `MAX_OUTBOUND_BUFFER`. Returns `Ok(())` even when buffered.
    ///
    /// TS: replBridge.ts — writeMessages / writeSdkMessages.
    pub async fn send(&self, msg: ReplOutMessage) -> anyhow::Result<()> {
        if self.state() == BridgeState::Connected {
            self.outgoing_tx
                .send(msg)
                .await
                .map_err(|_| anyhow::anyhow!("outgoing channel closed"))?;
        } else {
            let mut buf = self.buffer.lock().await;
            if buf.len() >= MAX_OUTBOUND_BUFFER {
                warn!(
                    session_id = %self.session_id,
                    buffer_size = buf.len(),
                    "REPL bridge outbound buffer full, dropping oldest"
                );
                buf.pop_front();
            }
            buf.push_back(msg);
        }
        Ok(())
    }

    /// Drain buffered messages after reconnection.
    ///
    /// TS: replBridge.ts — drain on transport connect callback.
    pub async fn drain_buffer(&self) -> anyhow::Result<()> {
        let messages: Vec<ReplOutMessage> = {
            let mut buf = self.buffer.lock().await;
            buf.drain(..).collect()
        };

        let count = messages.len();
        for msg in messages {
            self.outgoing_tx
                .send(msg)
                .await
                .map_err(|_| anyhow::anyhow!("outgoing channel closed"))?;
        }

        if count > 0 {
            info!(
                session_id = %self.session_id,
                drained = count,
                "REPL bridge drained buffered messages"
            );
        }

        Ok(())
    }

    /// Send a result message and transition to idle.
    ///
    /// TS: replBridge.ts — sendResult().
    pub async fn send_result(&self, text: String) -> anyhow::Result<()> {
        self.send(ReplOutMessage::Result {
            text,
            session_id: Some(self.session_id.clone()),
        })
        .await
    }

    /// Send an error message.
    pub async fn send_error(&self, message: String) -> anyhow::Result<()> {
        self.send(ReplOutMessage::Error { message }).await
    }

    /// Number of messages currently buffered.
    pub async fn buffer_len(&self) -> usize {
        self.buffer.lock().await.len()
    }
}

/// Decode an NDJSON line into a REPL inbound message.
pub fn decode_repl_ndjson(line: &str) -> anyhow::Result<ReplInMessage> {
    let msg: ReplInMessage = serde_json::from_str(line.trim())?;
    Ok(msg)
}

/// Encode a REPL outbound message as NDJSON.
pub fn encode_repl_ndjson(msg: &ReplOutMessage) -> anyhow::Result<String> {
    let json = serde_json::to_string(msg)?;
    Ok(format!("{json}\n"))
}

#[cfg(test)]
#[path = "repl.test.rs"]
mod tests;
