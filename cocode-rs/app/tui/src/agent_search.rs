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
#[path = "agent_search.test.rs"]
mod tests;
