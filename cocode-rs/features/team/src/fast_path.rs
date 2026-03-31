//! In-process fast path for inter-agent message delivery.
//!
//! When agents run as tokio tasks in the same process, messages can be
//! delivered via `mpsc` channels instead of filesystem I/O. The fast path
//! is used alongside the filesystem mailbox (dual-write) to maintain an
//! audit trail while enabling sub-millisecond delivery.
//!
//! Aligned with Claude Code's `pendingUserMessages` AppState fast path.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::sync::mpsc;

use crate::types::AgentMessage;

/// Channel buffer size per agent (bounded to prevent unbounded growth).
const CHANNEL_BUFFER: usize = 64;

/// In-process message channel registry.
///
/// Agents register when they start and unregister when they stop.
/// Senders can check if an agent has a channel before falling back
/// to the filesystem mailbox.
#[derive(Debug, Clone)]
pub struct FastPath {
    /// Map from composite key `{team_name}:{agent_id}` to sender channel.
    channels: Arc<Mutex<HashMap<String, mpsc::Sender<AgentMessage>>>>,
}

impl FastPath {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register an in-process agent and get its receiver channel.
    ///
    /// The receiver should be polled in the agent's main loop.
    pub async fn register(&self, team_name: &str, agent_id: &str) -> mpsc::Receiver<AgentMessage> {
        let (tx, rx) = mpsc::channel(CHANNEL_BUFFER);
        let key = Self::key(team_name, agent_id);
        self.channels.lock().await.insert(key, tx);
        rx
    }

    /// Unregister an agent (on shutdown or removal).
    pub async fn unregister(&self, team_name: &str, agent_id: &str) {
        let key = Self::key(team_name, agent_id);
        self.channels.lock().await.remove(&key);
    }

    /// Try to send a message via the fast path.
    ///
    /// Returns `true` if the agent has a registered channel and the message
    /// was accepted. Returns `false` if no channel exists (caller should
    /// fall back to the filesystem mailbox) or the channel is full/closed.
    pub async fn try_send(&self, team_name: &str, agent_id: &str, msg: AgentMessage) -> bool {
        let key = Self::key(team_name, agent_id);
        let channels = self.channels.lock().await;
        match channels.get(&key) {
            Some(tx) => tx.try_send(msg).is_ok(),
            None => false,
        }
    }

    /// Broadcast a message to all registered agents in a team (except sender).
    ///
    /// Returns the number of agents that received the message via fast path.
    pub async fn broadcast(
        &self,
        team_name: &str,
        msg: &AgentMessage,
        member_ids: &[String],
    ) -> usize {
        let prefix = format!("{team_name}:");
        let channels = self.channels.lock().await;
        let mut delivered = 0;

        for member_id in member_ids {
            if *member_id == msg.from {
                continue;
            }
            let key = format!("{prefix}{member_id}");
            if let Some(tx) = channels.get(&key) {
                let mut member_msg = msg.clone();
                member_msg.to = member_id.clone();
                member_msg.id = uuid::Uuid::new_v4().to_string();
                if tx.try_send(member_msg).is_ok() {
                    delivered += 1;
                }
            }
        }
        delivered
    }

    /// Check if an agent has a registered fast-path channel.
    pub async fn has_agent(&self, team_name: &str, agent_id: &str) -> bool {
        let key = Self::key(team_name, agent_id);
        self.channels.lock().await.contains_key(&key)
    }

    /// Number of registered agents.
    pub async fn agent_count(&self) -> usize {
        self.channels.lock().await.len()
    }

    fn key(team_name: &str, agent_id: &str) -> String {
        format!("{team_name}:{agent_id}")
    }
}

impl Default for FastPath {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "fast_path.test.rs"]
mod tests;
