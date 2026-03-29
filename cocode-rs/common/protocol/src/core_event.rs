//! Unified event envelope wrapping protocol, stream, and TUI events.
//!
//! `CoreEvent` is the internal event type emitted by the agent loop. Consumers
//! choose which layers they care about:
//! - SDK/app-server: `Protocol` only (via `ServerNotification`)
//! - TUI: `Protocol` + `Tui` + `Stream` (full set)
//! - Logging: all variants for observability

use crate::server_notification::*;
use crate::stream_event::StreamEvent;
use crate::tui_event::TuiEvent;

/// Unified event envelope for all event categories.
#[derive(Debug, Clone)]
pub enum CoreEvent {
    /// Client-facing protocol events (SDK, app-server, TUI).
    Protocol(ServerNotification),
    /// Raw streaming deltas requiring stateful accumulation.
    Stream(StreamEvent),
    /// TUI-only events (dropped by SDK/app-server consumers).
    Tui(TuiEvent),
}
