//! Agent search manager for @agent-* autocomplete.
//!
//! This module provides agent search functionality for the @agent-type
//! mention autocomplete feature, using fuzzy matching to find agents by type.

use cocode_subagent::AgentDefinition;
use cocode_utils_common::fuzzy_match;

use crate::state::AgentSuggestionItem;

/// Maximum number of suggestions to return.
const MAX_SUGGESTIONS: i32 = 10;

/// Information about an agent for searching.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    /// Agent type identifier (e.g., "explore").
    pub agent_type: String,
    /// Human-readable name (e.g., "Explore").
    pub name: String,
    /// Short description.
    pub description: String,
}

impl From<&AgentDefinition> for AgentInfo {
    fn from(def: &AgentDefinition) -> Self {
        Self {
            agent_type: def.agent_type.clone(),
            name: def.name.clone(),
            description: def.description.clone(),
        }
    }
}

/// Manages agent search with fuzzy matching.
///
/// This struct handles:
/// - Loading agents from definitions
/// - Fuzzy search by agent type
#[derive(Debug, Default)]
pub struct AgentSearchManager {
    /// Loaded agent info for searching.
    agents: Vec<AgentInfo>,
}

impl AgentSearchManager {
    /// Create a new empty agent search manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any agents have been loaded.
    pub fn has_agents(&self) -> bool {
        !self.agents.is_empty()
    }

    /// Load agents from an iterator.
    pub fn load_agents(&mut self, agents: impl Iterator<Item = AgentInfo>) {
        self.agents = agents.collect();
    }

    /// Search for agents matching the query.
    ///
    /// The query is the text after `@` (e.g., `"agent-exp"`, `"agent-"`, `"agent"`).
    /// We extract the part after `agent-` (if present) for fuzzy matching against
    /// `agent_type`.
    pub fn search(&self, query: &str) -> Vec<AgentSuggestionItem> {
        // Extract the fuzzy target: part after "agent-" (or empty if just "agent")
        let fuzzy_target = if let Some(rest) = query.strip_prefix("agent-") {
            rest
        } else {
            // Query is just "agent" or partial — show all agents
            ""
        };

        if fuzzy_target.is_empty() {
            // Return all agents sorted by name
            let mut suggestions: Vec<_> = self
                .agents
                .iter()
                .map(|agent| AgentSuggestionItem {
                    agent_type: agent.agent_type.clone(),
                    name: agent.name.clone(),
                    description: agent.description.clone(),
                    score: i32::MAX,
                    match_indices: vec![],
                })
                .collect();
            suggestions.sort_by(|a, b| a.agent_type.cmp(&b.agent_type));
            suggestions.truncate(MAX_SUGGESTIONS as usize);
            return suggestions;
        }

        let mut results = Vec::new();

        for agent in &self.agents {
            if let Some((indices, score)) = fuzzy_match(&agent.agent_type, fuzzy_target) {
                results.push(AgentSuggestionItem {
                    agent_type: agent.agent_type.clone(),
                    name: agent.name.clone(),
                    description: agent.description.clone(),
                    score,
                    match_indices: indices,
                });
            }
        }

        // Sort by score (ascending = better)
        results.sort_by_key(|r| r.score);

        // Limit results
        results.truncate(MAX_SUGGESTIONS as usize);

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_agents() -> Vec<AgentInfo> {
        vec![
            AgentInfo {
                agent_type: "explore".to_string(),
                name: "Explore".to_string(),
                description: "Read-only codebase exploration".to_string(),
            },
            AgentInfo {
                agent_type: "bash".to_string(),
                name: "Bash".to_string(),
                description: "Command execution specialist".to_string(),
            },
            AgentInfo {
                agent_type: "general-purpose".to_string(),
                name: "General Purpose".to_string(),
                description: "General-purpose agent".to_string(),
            },
            AgentInfo {
                agent_type: "plan".to_string(),
                name: "Plan".to_string(),
                description: "Software architect agent".to_string(),
            },
        ]
    }

    #[test]
    fn test_search_agent_prefix_only() {
        let mut manager = AgentSearchManager::new();
        manager.load_agents(create_test_agents().into_iter());

        // "agent" or "agent-" should return all agents
        let results = manager.search("agent");
        assert_eq!(results.len(), 4);

        let results = manager.search("agent-");
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn test_search_exact_match() {
        let mut manager = AgentSearchManager::new();
        manager.load_agents(create_test_agents().into_iter());

        let results = manager.search("agent-explore");
        assert!(!results.is_empty());
        assert_eq!(results[0].agent_type, "explore");
    }

    #[test]
    fn test_search_prefix_match() {
        let mut manager = AgentSearchManager::new();
        manager.load_agents(create_test_agents().into_iter());

        let results = manager.search("agent-exp");
        assert!(!results.is_empty());
        assert_eq!(results[0].agent_type, "explore");
    }

    #[test]
    fn test_search_fuzzy_match() {
        let mut manager = AgentSearchManager::new();
        manager.load_agents(create_test_agents().into_iter());

        let results = manager.search("agent-expl");
        assert!(!results.is_empty());
        assert_eq!(results[0].agent_type, "explore");
    }

    #[test]
    fn test_search_no_match() {
        let mut manager = AgentSearchManager::new();
        manager.load_agents(create_test_agents().into_iter());

        let results = manager.search("agent-xyz");
        assert!(results.is_empty());
    }
}
