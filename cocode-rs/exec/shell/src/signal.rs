//! Background signal mechanism for mid-execution bash command transitions.
//!
//! This module provides a mechanism to transition a foreground bash command
//! to background execution mid-way through its run. This is used to implement
//! the Ctrl+B "background this command" feature in the TUI.
//!
//! ## Lifecycle
//!
//! 1. When a foreground bash command starts, call [`register_backgroundable_bash`]
//! 2. The execution uses `tokio::select!` to wait for both:
//!    - The command completing normally
//!    - The background signal being triggered
//! 3. If user presses Ctrl+B, call [`trigger_bash_background`]
//! 4. The command transitions to background mode
//! 5. On completion (either path), call [`unregister_backgroundable_bash`]

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::RwLock;

use tokio::sync::oneshot;

/// Global map of signal IDs to their background signal senders.
static BASH_BACKGROUND_SIGNALS: LazyLock<RwLock<HashMap<String, oneshot::Sender<()>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Register a bash command as backgroundable and get the receiver for the signal.
///
/// The returned receiver will fire when [`trigger_bash_background`] is
/// called for this signal ID.
#[allow(clippy::expect_used)]
pub fn register_backgroundable_bash(id: String) -> oneshot::Receiver<()> {
    let (tx, rx) = oneshot::channel();

    let mut map = BASH_BACKGROUND_SIGNALS.write().expect("lock poisoned");
    map.insert(id, tx);

    rx
}

/// Trigger a background transition for the given bash command.
///
/// If the command is registered and the signal channel is still open, this will
/// send the background signal and return `true`. Otherwise returns `false`.
#[allow(clippy::expect_used)]
pub fn trigger_bash_background(id: &str) -> bool {
    let mut map = BASH_BACKGROUND_SIGNALS.write().expect("lock poisoned");

    if let Some(tx) = map.remove(id) {
        tx.send(()).is_ok()
    } else {
        false
    }
}

/// Unregister a bash command from the backgroundable map.
///
/// This should be called when a command completes (either normally or via
/// background transition) to clean up the signal sender.
#[allow(clippy::expect_used)]
pub fn unregister_backgroundable_bash(id: &str) {
    let mut map = BASH_BACKGROUND_SIGNALS.write().expect("lock poisoned");
    map.remove(id);
}

/// Get the list of currently backgroundable bash command IDs.
///
/// This is useful for the TUI to know which bash commands can be
/// sent to background via Ctrl+B.
#[allow(clippy::expect_used)]
pub fn backgroundable_bash_ids() -> Vec<String> {
    let map = BASH_BACKGROUND_SIGNALS.read().expect("lock poisoned");
    map.keys().cloned().collect()
}

#[cfg(test)]
#[path = "signal.test.rs"]
mod tests;
