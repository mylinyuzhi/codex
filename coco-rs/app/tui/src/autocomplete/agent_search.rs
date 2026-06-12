//! Agent search source for the unified `@` popup.
//!
//! Pure in-memory projection of `coco_subagent::LoadedAgentDefinition`
//! into the lighter [`AgentInfo`] form consumed by the suggestion
//! pipeline. The popup feeds these through
//! [`super::unified::seed_agent_items`] which appends the `(agent)`
//! suffix and stamps `SuggestionMeta::Agent { color }` so the renderer
//! can draw the icon + color and the insertion path can pick the right
//! splice format.

use coco_types::AgentColorName;
use coco_types::AgentDefinition;

use crate::widgets::suggestion_popup::SuggestionItem;
use crate::widgets::suggestion_popup::SuggestionMeta;

/// A loaded agent definition projection used for `@` autocomplete.
///
/// Carries the minimum the popup needs to render and the insertion path needs
/// to emit a `(agent)` suffix splice. Full definitions stay in
/// `coco_subagent::AgentDefinitionStore`; this is a UI-side cache.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    /// User-facing agent name (`agentType`). Also embedded in the
    /// `"<name> (agent)"` label by [`super::unified::seed_agent_items`].
    pub name: String,
    /// Original `agentType` identifier — kept distinct from `name` for
    /// future cases where they diverge (display alias vs. type id).
    pub agent_type: String,
    /// Single-line `whenToUse` description.
    pub description: Option<String>,
    /// Optional badge color picked from the eight-color palette.
    pub color: Option<AgentColorName>,
}

impl AgentInfo {
    /// Project the full `coco_types::AgentDefinition` down to the UI
    /// surface. Used by the session bootstrap (`tui_runner::run_tui`)
    /// and by `/agents reload` to seed `session.available_agents`.
    pub fn from_definition(def: &AgentDefinition) -> Self {
        Self {
            name: def.name.clone(),
            agent_type: def.name.clone(),
            description: def.description.clone(),
            color: def.color,
        }
    }
}

/// Synchronous filter over a static `AgentInfo` slice.
///
/// Retained for callsites that want the raw single-source search (no
/// merging). The unified popup goes through
/// [`super::unified::seed_agent_items`] instead.
pub struct AgentSearchManager {
    agents: Vec<AgentInfo>,
}

impl AgentSearchManager {
    pub fn new(agents: Vec<AgentInfo>) -> Self {
        Self { agents }
    }

    pub fn empty() -> Self {
        Self { agents: Vec::new() }
    }

    pub fn set_agents(&mut self, agents: Vec<AgentInfo>) {
        self.agents = agents;
    }

    /// Substring filter, case-insensitive. Returns items already
    /// formatted with the `(agent)` suffix on the label so direct
    /// callsites get the same look as the unified popup.
    pub fn search(&self, query: &str) -> Vec<SuggestionItem> {
        let query_lower = query.to_lowercase();
        self.agents
            .iter()
            .filter(|a| a.name.to_lowercase().contains(&query_lower))
            .take(10)
            .map(|a| SuggestionItem {
                label: format!("{} (agent)", a.name),
                description: a.description.clone(),
                metadata: Some(SuggestionMeta::Agent { color: a.color }),
            })
            .collect()
    }
}

impl Default for AgentSearchManager {
    fn default() -> Self {
        Self::empty()
    }
}
