//! Immutable per-turn view of the active agent catalog.
//!
//! Consumers (`AgentTool::prompt`, `/agents` commands, runtime spawn) take a
//! snapshot once and read from it without locking. Snapshots preserve the
//! order in which agents would be displayed (by source priority, then name).
//!
//! `AgentDefinitionStore::snapshot()` returns `Arc<AgentCatalogSnapshot>`,
//! so per-turn reads are pointer clones. Failed-load diagnostics live on
//! `AgentLoadReport`, not the snapshot — the snapshot is the *active*
//! catalog view, not a load journal.

use std::collections::BTreeMap;

use coco_types::AgentDefinition;

use crate::definition_store::LoadedAgentDefinition;

#[derive(Debug, Clone)]
pub struct AgentCatalogSnapshot {
    /// Active definitions keyed by canonical `agent_type`.
    /// Alphabetically ordered via `BTreeMap` keying — deterministic across
    /// platforms and reload cycles. Note byte-wise lex order means
    /// PascalCase entries (`Explore`, `Plan`) sort before lowercase
    /// entries (`build`, `claude-code-guide`).
    active: BTreeMap<String, AgentDefinition>,
    /// All loaded definitions (including those overridden by higher-priority
    /// sources). Used by `/agents show` to display source chains.
    all: Vec<LoadedAgentDefinition>,
}

impl AgentCatalogSnapshot {
    pub fn new(active: BTreeMap<String, AgentDefinition>, all: Vec<LoadedAgentDefinition>) -> Self {
        Self { active, all }
    }

    /// All active agents in deterministic order.
    pub fn active(&self) -> impl Iterator<Item = &AgentDefinition> {
        self.active.values()
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Look up an active agent by its canonical `agent_type`.
    pub fn find_active(&self, agent_type: &str) -> Option<&AgentDefinition> {
        self.active.get(agent_type)
    }

    /// Every loaded definition (including overridden ones), in load order.
    pub fn all(&self) -> &[LoadedAgentDefinition] {
        &self.all
    }
}
