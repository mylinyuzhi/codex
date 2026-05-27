//! Unified `@` suggestion ranking and merging.
//!
//! TS: `src/hooks/unifiedSuggestions.ts`. Produces a single ranked list
//! (cap 15) combining agents, file paths, and MCP resources. Per TS:
//!   - agents are scored by Fuse.js with weight 3 on `agentType`, weight 2
//!     on `displayText` — fuse scores are typically < 0.5 for decent
//!     matches, so agents dominate the top of the list versus the file
//!     prefix score (default 0.5)
//!   - files are pre-scored by nucleo (Rust) and merged below agents
//!   - MCP resources rank with the agent pool (not implemented here yet —
//!     follow-up for the MCP resource source)
//!
//! Rust port simplification: rather than re-implement Fuse weighted
//! scoring, we exploit the observed dominance and always place agents
//! before files. Within each pool the original order (agents:
//! substring-match preserves session-config order; files: nucleo score
//! descending) is kept. Total cap matches TS (`MAX_UNIFIED_SUGGESTIONS`).

use super::agent_search::AgentInfo;
use crate::widgets::suggestion_popup::SuggestionItem;
use crate::widgets::suggestion_popup::SuggestionMeta;

/// TS `MAX_UNIFIED_SUGGESTIONS` (`unifiedSuggestions.ts:70`).
const MAX_UNIFIED: usize = 15;

/// Build the agent half of the unified popup. Synchronous — agents are
/// already loaded in `session.available_agents` at session start.
///
/// Each row's label embeds the TS `displayText` format `"<name> (agent)"`
/// so the visual line shows the kind without a separate column. The
/// `SuggestionMeta::Agent { color }` carries the kind to the row
/// renderer (icon prefix) and to insertion (which strips the suffix
/// before splicing if needed — the submit parser at
/// `coco_context::user_input::extract_mentions:152` accepts the suffix
/// form directly, so we insert it verbatim).
pub fn seed_agent_items(agents: &[AgentInfo], query: &str) -> Vec<SuggestionItem> {
    let query_lower = query.to_lowercase();
    agents
        .iter()
        .filter(|a| query_lower.is_empty() || a.name.to_lowercase().contains(&query_lower))
        .take(MAX_UNIFIED)
        .map(|a| SuggestionItem {
            label: format!("{} (agent)", a.name),
            description: a.description.clone(),
            metadata: Some(SuggestionMeta::Agent { color: a.color }),
        })
        .collect()
}

/// Merge an async file-search result with already-seeded agent items.
///
/// Order: agents first (TS parity — agent fuse scores dominate file
/// scores), files appended. Total list is capped at [`MAX_UNIFIED`] so
/// the popup never overflows its vertical slot.
pub fn merge_file_results(
    mut agents: Vec<SuggestionItem>,
    files: Vec<SuggestionItem>,
) -> Vec<SuggestionItem> {
    let room = MAX_UNIFIED.saturating_sub(agents.len());
    agents.extend(files.into_iter().take(room));
    agents
}

#[cfg(test)]
#[path = "unified.test.rs"]
mod tests;
