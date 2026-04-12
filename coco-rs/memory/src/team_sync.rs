//! Team memory synchronization across agents.
//!
//! TS: services/teamMemorySync/ (2.2K LOC) — syncs memories between teammates.

use crate::MemoryEntry;
use std::collections::HashMap;

/// Memory sync state between agents.
#[derive(Debug, Clone, Default)]
pub struct TeamMemorySyncState {
    /// Memories known to each agent (agent_id → set of memory names).
    pub agent_memories: HashMap<String, Vec<String>>,
    /// Pending sync operations.
    pub pending_syncs: Vec<MemorySyncOp>,
}

/// A memory sync operation.
#[derive(Debug, Clone)]
pub enum MemorySyncOp {
    /// Send a memory to another agent.
    Send {
        from_agent: String,
        to_agent: String,
        memory: MemoryEntry,
    },
    /// Delete a memory from an agent.
    Delete {
        agent_id: String,
        memory_name: String,
    },
    /// Update a memory for an agent.
    Update {
        agent_id: String,
        memory: MemoryEntry,
    },
}

impl TeamMemorySyncState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register memories known to an agent.
    pub fn register_agent_memories(&mut self, agent_id: &str, memories: Vec<String>) {
        self.agent_memories.insert(agent_id.to_string(), memories);
    }

    /// Calculate which memories need to be synced to an agent.
    pub fn get_missing_memories(
        &self,
        agent_id: &str,
        all_memories: &[MemoryEntry],
    ) -> Vec<MemoryEntry> {
        let known = self.agent_memories.get(agent_id);
        all_memories
            .iter()
            .filter(|m| known.is_none_or(|k| !k.contains(&m.name)))
            .cloned()
            .collect()
    }

    /// Queue a sync operation.
    pub fn queue_sync(&mut self, op: MemorySyncOp) {
        self.pending_syncs.push(op);
    }

    /// Drain pending sync operations.
    pub fn drain_pending(&mut self) -> Vec<MemorySyncOp> {
        std::mem::take(&mut self.pending_syncs)
    }
}

#[cfg(test)]
#[path = "team_sync.test.rs"]
mod tests;
