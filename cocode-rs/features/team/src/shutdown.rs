//! Shutdown protocol for graceful agent termination.
//!
//! Tracks the request → acknowledge → complete lifecycle of shutdown
//! requests between team lead and teammates.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::Result;

/// State of a shutdown request.
#[derive(Debug, Clone)]
pub enum ShutdownState {
    /// Shutdown has been requested.
    Requested {
        /// When the request was made (Unix seconds).
        at: i64,
        /// Who requested the shutdown.
        from: String,
    },
    /// Agent has acknowledged the shutdown request.
    Acknowledged {
        /// When the acknowledgement was received (Unix seconds).
        at: i64,
    },
    /// Shutdown is complete.
    Completed {
        /// When the agent finished shutting down (Unix seconds).
        at: i64,
    },
}

/// Tracks shutdown requests and their lifecycle.
#[derive(Debug, Clone)]
pub struct ShutdownTracker {
    /// Map of agent_id → shutdown state.
    pending: Arc<Mutex<HashMap<String, ShutdownState>>>,
}

impl ShutdownTracker {
    /// Create a new shutdown tracker.
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Record a shutdown request for an agent.
    pub async fn request(&self, agent_id: &str, from: &str) -> Result<()> {
        let now = now_secs();
        let mut pending = self.pending.lock().await;
        pending.insert(
            agent_id.to_string(),
            ShutdownState::Requested {
                at: now,
                from: from.to_string(),
            },
        );
        Ok(())
    }

    /// Record that an agent acknowledged the shutdown.
    pub async fn acknowledge(&self, agent_id: &str) -> Result<()> {
        let now = now_secs();
        let mut pending = self.pending.lock().await;
        pending.insert(
            agent_id.to_string(),
            ShutdownState::Acknowledged { at: now },
        );
        Ok(())
    }

    /// Record that an agent completed its shutdown.
    pub async fn complete(&self, agent_id: &str) -> Result<()> {
        let now = now_secs();
        let mut pending = self.pending.lock().await;
        pending.insert(agent_id.to_string(), ShutdownState::Completed { at: now });
        Ok(())
    }

    /// Check if a shutdown is pending for an agent.
    pub async fn is_pending(&self, agent_id: &str) -> bool {
        let pending = self.pending.lock().await;
        matches!(
            pending.get(agent_id),
            Some(ShutdownState::Requested { .. } | ShutdownState::Acknowledged { .. })
        )
    }

    /// Check if all given members have completed shutdown.
    pub async fn all_complete(&self, members: &[String]) -> bool {
        let pending = self.pending.lock().await;
        members
            .iter()
            .all(|id| matches!(pending.get(id), Some(ShutdownState::Completed { .. })))
    }

    /// Get the current state for an agent, if any.
    pub async fn get_state(&self, agent_id: &str) -> Option<ShutdownState> {
        let pending = self.pending.lock().await;
        pending.get(agent_id).cloned()
    }

    /// Remove tracking for an agent (cleanup after team deletion).
    pub async fn remove(&self, agent_id: &str) {
        let mut pending = self.pending.lock().await;
        pending.remove(agent_id);
    }
}

impl Default for ShutdownTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "shutdown.test.rs"]
mod tests;
