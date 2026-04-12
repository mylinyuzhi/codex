//! Agent search autocomplete (@agent-* mentions).
//!
//! In-memory search against loaded agent definitions.

use crate::widgets::suggestion_popup::SuggestionItem;

/// A loaded agent definition for autocomplete.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub agent_type: String,
    pub description: Option<String>,
}

/// Manages agent search (synchronous, in-memory).
pub struct AgentSearchManager {
    agents: Vec<AgentInfo>,
}

impl AgentSearchManager {
    /// Create with a list of available agents.
    pub fn new(agents: Vec<AgentInfo>) -> Self {
        Self { agents }
    }

    /// Create empty (agents loaded later).
    pub fn empty() -> Self {
        Self { agents: Vec::new() }
    }

    /// Update the agent list.
    pub fn set_agents(&mut self, agents: Vec<AgentInfo>) {
        self.agents = agents;
    }

    /// Search agents matching the query.
    pub fn search(&self, query: &str) -> Vec<SuggestionItem> {
        let query_lower = query.to_lowercase();
        self.agents
            .iter()
            .filter(|a| a.name.to_lowercase().contains(&query_lower))
            .take(10)
            .map(|a| SuggestionItem {
                label: format!("@agent-{}", a.name),
                description: a.description.clone(),
            })
            .collect()
    }
}

impl Default for AgentSearchManager {
    fn default() -> Self {
        Self::empty()
    }
}
