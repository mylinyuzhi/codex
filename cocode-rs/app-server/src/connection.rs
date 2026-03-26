//! Per-connection state tracking.
//!
//! Each client connection (stdio or WebSocket) gets a unique `ConnectionId`
//! and associated lifecycle state (capabilities, notification filters).

use std::collections::HashSet;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

/// Unique identifier for a client connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(i64);

impl ConnectionId {
    /// The ID reserved for the single stdio connection.
    pub const STDIO: Self = Self(0);
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "conn-{}", self.0)
    }
}

/// Atomic counter for generating connection IDs.
pub struct ConnectionIdGenerator {
    counter: AtomicI64,
}

impl ConnectionIdGenerator {
    pub fn new() -> Self {
        Self {
            // Start at 1; 0 is reserved for stdio.
            counter: AtomicI64::new(1),
        }
    }

    pub fn next(&self) -> ConnectionId {
        ConnectionId(self.counter.fetch_add(1, Ordering::Relaxed))
    }
}

/// Outbound channel capacity (messages buffered per connection).
pub const OUTBOUND_CHANNEL_CAPACITY: usize = 128;

/// State associated with a single client connection.
#[derive(Default)]
pub struct ConnectionState {
    /// Whether this connection has completed the `initialize` handshake.
    pub initialized: bool,
    /// Whether the client opted into experimental API features.
    pub experimental_api: bool,
    /// Notification methods the client does not want to receive.
    pub opt_out_notifications: HashSet<String>,
}

impl ConnectionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the given notification method should be sent to this connection.
    ///
    /// Used by the processor to filter notifications based on client opt-outs.
    #[allow(dead_code)]
    pub fn should_send_notification(&self, method: &str) -> bool {
        !self.opt_out_notifications.contains(method)
    }
}
