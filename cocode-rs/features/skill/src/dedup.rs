//! Skill deduplication.
//!
//! When skills are loaded from multiple sources, duplicate names may appear.
//! This module provides name-based deduplication that keeps the first
//! occurrence of each skill name (respecting source precedence order).

use crate::outcome::SkillLoadOutcome;
use std::collections::HashSet;

/// Tracks seen skill names for deduplication.
///
/// # Example
///
/// ```
/// # use cocode_skill::SkillDeduplicator;
/// let mut dedup = SkillDeduplicator::new();
/// assert!(!dedup.is_duplicate("commit"));
/// assert!(dedup.is_duplicate("commit")); // second time is duplicate
/// ```
pub struct SkillDeduplicator {
    seen: HashSet<String>,
}

impl SkillDeduplicator {
    /// Creates a new empty deduplicator.
    pub fn new() -> Self {
        Self {
            seen: HashSet::new(),
        }
    }

    /// Returns `true` if the name has already been seen.
    ///
    /// If the name is new, records it and returns `false`.
    /// If the name was already recorded, returns `true`.
    pub fn is_duplicate(&mut self, name: &str) -> bool {
        !self.seen.insert(name.to_string())
    }

    /// Returns the number of unique names seen so far.
    pub fn len(&self) -> i32 {
        self.seen.len() as i32
    }

    /// Returns `true` if no names have been recorded.
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

impl Default for SkillDeduplicator {
    fn default() -> Self {
        Self::new()
    }
}

/// Deduplicates a list of skill load outcomes by name.
///
/// Keeps the first successful occurrence of each skill name. Failed
/// outcomes are always kept (they have no name to dedup on). Later
/// duplicates are logged at debug level and dropped.
pub fn dedup_skills(skills: Vec<SkillLoadOutcome>) -> Vec<SkillLoadOutcome> {
    let mut dedup = SkillDeduplicator::new();
    let mut result = Vec::with_capacity(skills.len());

    for outcome in skills {
        match outcome.skill_name() {
            Some(name) => {
                if dedup.is_duplicate(name) {
                    tracing::debug!(name = name, "dropping duplicate skill");
                } else {
                    result.push(outcome);
                }
            }
            None => {
                // Failed outcomes are always kept for diagnostics
                result.push(outcome);
            }
        }
    }

    result
}

#[cfg(test)]
#[path = "dedup.test.rs"]
mod tests;
