//! Auto-dream memory consolidation.
//!
//! TS: services/autoDream/ (550 LOC) — periodic memory consolidation.
//! Runs after conversations to merge and clean up memory entries.

use crate::MemoryEntry;
use crate::MemoryEntryType;

/// Auto-dream configuration.
#[derive(Debug, Clone)]
pub struct AutoDreamConfig {
    /// Minimum number of new entries before triggering consolidation.
    pub min_new_entries: usize,
    /// Maximum memory entries before forced consolidation.
    pub max_entries: usize,
    /// Whether to consolidate project memories with user memories.
    pub cross_type_consolidation: bool,
}

impl Default for AutoDreamConfig {
    fn default() -> Self {
        Self {
            min_new_entries: 3,
            max_entries: 50,
            cross_type_consolidation: false,
        }
    }
}

/// Check if memory consolidation should run.
pub fn should_consolidate(
    entries: &[MemoryEntry],
    new_count: usize,
    config: &AutoDreamConfig,
) -> bool {
    new_count >= config.min_new_entries || entries.len() > config.max_entries
}

/// Find duplicate or overlapping entries that could be merged.
pub fn find_merge_candidates(entries: &[MemoryEntry]) -> Vec<(usize, usize)> {
    let mut candidates = Vec::new();
    for i in 0..entries.len() {
        for j in i + 1..entries.len() {
            if entries[i].memory_type == entries[j].memory_type
                && entries_overlap(&entries[i], &entries[j])
            {
                candidates.push((i, j));
            }
        }
    }
    candidates
}

/// Check if two memory entries overlap significantly.
fn entries_overlap(a: &MemoryEntry, b: &MemoryEntry) -> bool {
    // Simple heuristic: check if names are similar or content overlaps
    if a.name == b.name {
        return true;
    }
    // Check word overlap in descriptions
    let a_words: std::collections::HashSet<&str> = a.description.split_whitespace().collect();
    let b_words: std::collections::HashSet<&str> = b.description.split_whitespace().collect();
    let overlap = a_words.intersection(&b_words).count();
    let min_len = a_words.len().min(b_words.len());
    min_len > 0 && overlap * 2 > min_len
}

/// Find stale entries that may need updating.
pub fn find_stale_entries(entries: &[MemoryEntry]) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.memory_type == MemoryEntryType::Project)
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
#[path = "auto_dream.test.rs"]
mod tests;
