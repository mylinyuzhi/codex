//! Event broker — pause/resume stdin for external editor integration.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Controls whether terminal events are consumed.
///
/// When paused (e.g., during external editor), the event stream
/// stops reading from stdin to avoid stealing the editor's input.
#[derive(Debug, Clone)]
pub struct EventBroker {
    paused: Arc<AtomicBool>,
}

impl EventBroker {
    /// Create a new event broker (initially active).
    pub fn new() -> Self {
        Self {
            paused: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Pause event consumption.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    /// Resume event consumption.
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    /// Whether events are currently paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}

impl Default for EventBroker {
    fn default() -> Self {
        Self::new()
    }
}
