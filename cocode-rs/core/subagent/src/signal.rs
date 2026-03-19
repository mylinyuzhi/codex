//! Background signal mechanism for mid-execution agent transitions.
//!
//! This module provides a mechanism to transition a foreground agent to
//! background execution mid-way through its run. This is used to implement
//! the Ctrl+B "background this agent" feature in the TUI.
//!
//! ## Lifecycle
//!
//! 1. When a foreground agent starts, call [`register_backgroundable_agent`]
//! 2. The agent execution uses `tokio::select!` to wait for both:
//!    - The agent completing normally
//!    - The background signal being triggered
//! 3. If user presses Ctrl+B, call [`trigger_background_transition`]
//! 4. The agent transitions to background mode
//! 5. On completion (either path), call [`unregister_backgroundable_agent`]

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;

use once_cell::sync::Lazy;
use tokio::sync::oneshot;

/// Global map of agent IDs to their background signal senders.
///
/// This is process-global (not session-scoped) because agent IDs are UUIDs,
/// which are globally unique across all sessions within the same process.
/// A session-scoped design would be cleaner but is not required for correctness
/// as long as agent IDs remain unique, which UUID v4 guarantees.
static BACKGROUND_SIGNAL_MAP: Lazy<RwLock<HashMap<String, oneshot::Sender<()>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Acquire a read lock on the background signal map, recovering from poison.
fn read_map() -> RwLockReadGuard<'static, HashMap<String, oneshot::Sender<()>>> {
    BACKGROUND_SIGNAL_MAP.read().unwrap_or_else(|e| {
        tracing::warn!("Background signal map read lock was poisoned, recovering");
        e.into_inner()
    })
}

/// Acquire a write lock on the background signal map, recovering from poison.
fn write_map() -> RwLockWriteGuard<'static, HashMap<String, oneshot::Sender<()>>> {
    BACKGROUND_SIGNAL_MAP.write().unwrap_or_else(|e| {
        tracing::warn!("Background signal map write lock was poisoned, recovering");
        e.into_inner()
    })
}

/// Register an agent as backgroundable and get the receiver for the signal.
///
/// The returned receiver will fire when [`trigger_background_transition`] is
/// called for this agent ID.
///
/// # Arguments
///
/// * `agent_id` - Unique identifier for the agent
///
/// # Returns
///
/// A oneshot receiver that will receive a signal when backgrounding is requested.
pub fn register_backgroundable_agent(agent_id: String) -> oneshot::Receiver<()> {
    let (tx, rx) = oneshot::channel();

    let mut map = write_map();
    map.insert(agent_id, tx);

    rx
}

/// Trigger a background transition for the given agent.
///
/// If the agent is registered and the signal channel is still open, this will
/// send the background signal and return `true`. Otherwise returns `false`.
///
/// # Arguments
///
/// * `agent_id` - The agent ID to transition to background
///
/// # Returns
///
/// `true` if the signal was sent successfully, `false` if the agent is not
/// registered or the channel was already closed.
pub fn trigger_background_transition(agent_id: &str) -> bool {
    let mut map = write_map();

    if let Some(tx) = map.remove(agent_id) {
        // Send the signal - if the receiver is already dropped, that's fine
        tx.send(()).is_ok()
    } else {
        false
    }
}

/// Unregister an agent from the backgroundable map.
///
/// This should be called when an agent completes (either normally or via
/// background transition) to clean up the signal sender.
///
/// # Arguments
///
/// * `agent_id` - The agent ID to unregister
pub fn unregister_backgroundable_agent(agent_id: &str) {
    let mut map = write_map();
    map.remove(agent_id);
}

/// Check if an agent is currently registered as backgroundable.
///
/// # Arguments
///
/// * `agent_id` - The agent ID to check
///
/// # Returns
///
/// `true` if the agent is registered and can receive a background signal.
pub fn is_agent_backgroundable(agent_id: &str) -> bool {
    let map = read_map();
    map.contains_key(agent_id)
}

/// Get the list of currently backgroundable agent IDs.
///
/// This is useful for UI elements that need to show which agents can be
/// sent to background.
pub fn backgroundable_agent_ids() -> Vec<String> {
    let map = read_map();
    map.keys().cloned().collect()
}

#[cfg(test)]
#[path = "signal.test.rs"]
mod tests;
