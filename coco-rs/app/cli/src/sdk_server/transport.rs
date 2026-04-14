//! SDK transport layer — NDJSON framing over byte streams.
//!
//! Defines the `SdkTransport` async trait and two implementations:
//!
//! - [`StdioTransport`]: reads `JsonRpcMessage` lines from `tokio::io::stdin`,
//!   writes them to `tokio::io::stdout`. This is the primary transport for
//!   SDK clients that spawn `coco --sdk-mode` as a subprocess.
//! - [`InMemoryTransport`]: in-memory duplex pipes using tokio channels,
//!   used for unit tests and integration harnesses.
//!
//! The transport layer is **protocol-agnostic**: it deals in
//! `JsonRpcMessage` envelopes only. The dispatch loop on top of it
//! (Phase 2.C.1 `SdkServer`) owns the `ClientRequest` → handler routing.
//!
//! TS reference: `src/cli/structuredIO.ts` — `StructuredIO` class. TS
//! uses raw strings + ad-hoc JSON parsing; coco-rs types the wire
//! format as `JsonRpcMessage` from the start.
//!
//! See `event-system-design.md` §5 and §12.

use std::sync::Arc;

use coco_types::JsonRpcMessage;
use thiserror::Error;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::trace;
use tracing::warn;

/// Errors raised by the transport layer.
#[derive(Debug, Error)]
pub enum TransportError {
    /// The transport was closed (clean shutdown).
    #[error("transport closed")]
    Closed,

    /// Underlying I/O failure (broken pipe, disconnect, etc.).
    #[error("transport I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Received a line that could not be parsed as JSON.
    #[error("parse error: {0}")]
    Parse(#[from] serde_json::Error),

    /// Send failed because the receiver has been dropped.
    #[error("channel send error: peer dropped")]
    PeerDropped,
}

/// Async-trait for SDK transports.
///
/// Implementations frame `JsonRpcMessage` values onto and off of a byte
/// stream. Framing is **NDJSON**: one compact JSON value per line
/// terminated by `\n`.
///
/// Concurrency model: `recv()` and `send()` may be called from the same
/// task or different tasks. Implementations must be `Send + Sync` so they
/// can be shared via `Arc`.
#[async_trait::async_trait]
pub trait SdkTransport: Send + Sync {
    /// Read the next message. Returns `Ok(None)` on clean EOF.
    async fn recv(&self) -> Result<Option<JsonRpcMessage>, TransportError>;

    /// Write a message to the peer.
    async fn send(&self, msg: JsonRpcMessage) -> Result<(), TransportError>;

    /// Close the transport. Subsequent `send()` calls return
    /// `TransportError::Closed`. Pending `recv()` may still return messages
    /// that were buffered before close.
    async fn close(&self) -> Result<(), TransportError>;

    /// Whether the transport is still open.
    fn is_open(&self) -> bool;
}

// ---------------------------------------------------------------------------
// StdioTransport — reads from stdin, writes to stdout
// ---------------------------------------------------------------------------

/// NDJSON transport over `tokio::io::stdin()` / `tokio::io::stdout()`.
///
/// Used by `coco --sdk-mode` when launched as a subprocess by the Python
/// SDK (or any other client that talks JSON-RPC over stdio).
///
/// Design notes:
/// - **Writer is serialized** behind a tokio `Mutex` so concurrent `send()`
///   calls from the dispatch loop and notification forwarder can't interleave
///   partial lines on stdout.
/// - **Reader** reads directly under the transport's lock held for the
///   duration of one line read. Since SDK usage is request/response with
///   at most one in-flight `recv()`, this is simpler than a dedicated
///   reader task + channel.
pub struct StdioTransport {
    reader: Mutex<BufReader<tokio::io::Stdin>>,
    writer: Mutex<tokio::io::Stdout>,
    open: std::sync::atomic::AtomicBool,
}

impl StdioTransport {
    /// Create a new stdio transport bound to the process's stdin/stdout.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            reader: Mutex::new(BufReader::new(tokio::io::stdin())),
            writer: Mutex::new(tokio::io::stdout()),
            open: std::sync::atomic::AtomicBool::new(true),
        })
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self {
            reader: Mutex::new(BufReader::new(tokio::io::stdin())),
            writer: Mutex::new(tokio::io::stdout()),
            open: std::sync::atomic::AtomicBool::new(true),
        }
    }
}

#[async_trait::async_trait]
impl SdkTransport for StdioTransport {
    async fn recv(&self) -> Result<Option<JsonRpcMessage>, TransportError> {
        if !self.is_open() {
            return Err(TransportError::Closed);
        }
        let mut reader = self.reader.lock().await;
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    // Clean EOF — peer closed stdin.
                    debug!("stdio transport: EOF on stdin");
                    return Ok(None);
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        // Skip blank lines (common with pretty-printed input).
                        continue;
                    }
                    trace!(line = %trimmed, "stdio transport: recv");
                    let msg = serde_json::from_str::<JsonRpcMessage>(trimmed)?;
                    return Ok(Some(msg));
                }
                Err(e) => {
                    warn!(error = %e, "stdio transport: read error");
                    return Err(TransportError::Io(e));
                }
            }
        }
    }

    async fn send(&self, msg: JsonRpcMessage) -> Result<(), TransportError> {
        if !self.is_open() {
            return Err(TransportError::Closed);
        }
        // Serialize compactly (no pretty indentation) — one line per message.
        let json = serde_json::to_string(&msg)?;
        let mut writer = self.writer.lock().await;
        writer.write_all(json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        trace!("stdio transport: sent {} bytes", json.len() + 1);
        Ok(())
    }

    async fn close(&self) -> Result<(), TransportError> {
        self.open.store(false, std::sync::atomic::Ordering::SeqCst);
        // Flush any pending writes.
        let mut writer = self.writer.lock().await;
        let _ = writer.flush().await;
        Ok(())
    }

    fn is_open(&self) -> bool {
        self.open.load(std::sync::atomic::Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// InMemoryTransport — paired duplex channel for unit tests
// ---------------------------------------------------------------------------

/// Two-way in-memory transport pair. Primarily used in tests so the SDK
/// server can be driven without spawning a subprocess or touching stdio.
///
/// [`InMemoryTransport::pair`] returns two connected ends where each end's
/// `send()` is received by the other end's `recv()`.
pub struct InMemoryTransport {
    inbox: Mutex<mpsc::Receiver<JsonRpcMessage>>,
    outbox: mpsc::Sender<JsonRpcMessage>,
    open: std::sync::atomic::AtomicBool,
}

impl InMemoryTransport {
    /// Create a pair of connected in-memory transports.
    ///
    /// Returns `(server_end, client_end)`:
    /// - `server_end` is passed to `SdkServer` so it reads ClientRequests
    ///   from the client and writes responses/notifications back.
    /// - `client_end` is used by the test harness to drive the server.
    pub fn pair(capacity: usize) -> (Arc<Self>, Arc<Self>) {
        let (a_tx, a_rx) = mpsc::channel::<JsonRpcMessage>(capacity);
        let (b_tx, b_rx) = mpsc::channel::<JsonRpcMessage>(capacity);

        // Server reads from a (client writes here), writes to b.
        let server = Arc::new(Self {
            inbox: Mutex::new(a_rx),
            outbox: b_tx,
            open: std::sync::atomic::AtomicBool::new(true),
        });
        // Client reads from b (server writes here), writes to a.
        let client = Arc::new(Self {
            inbox: Mutex::new(b_rx),
            outbox: a_tx,
            open: std::sync::atomic::AtomicBool::new(true),
        });
        (server, client)
    }
}

#[async_trait::async_trait]
impl SdkTransport for InMemoryTransport {
    async fn recv(&self) -> Result<Option<JsonRpcMessage>, TransportError> {
        if !self.is_open() {
            return Err(TransportError::Closed);
        }
        let mut rx = self.inbox.lock().await;
        Ok(rx.recv().await)
    }

    async fn send(&self, msg: JsonRpcMessage) -> Result<(), TransportError> {
        if !self.is_open() {
            return Err(TransportError::Closed);
        }
        self.outbox
            .send(msg)
            .await
            .map_err(|_| TransportError::PeerDropped)
    }

    async fn close(&self) -> Result<(), TransportError> {
        self.open.store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn is_open(&self) -> bool {
        self.open.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
#[path = "transport.test.rs"]
mod tests;
