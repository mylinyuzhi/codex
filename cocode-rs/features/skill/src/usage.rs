//! Skill usage tracking and scoring.
//!
//! Tracks how often each skill is invoked and provides a recency-weighted
//! score for prioritization in skill listings.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Tracks skill invocations for usage-based scoring.
///
/// Thread-safe via internal `Mutex`. Usage data is ephemeral (in-memory only)
/// and resets each session.
#[derive(Debug)]
pub struct SkillUsageTracker {
    data: Mutex<HashMap<String, UsageData>>,
}

#[derive(Debug, Clone)]
struct UsageData {
    count: u64,
    last_used: Instant,
}

impl SkillUsageTracker {
    /// Create a new empty tracker.
    pub fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
        }
    }

    /// Record an invocation of the named skill.
    pub fn track(&self, name: &str) {
        let mut data = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = data.entry(name.to_string()).or_insert_with(|| UsageData {
            count: 0,
            last_used: Instant::now(),
        });
        entry.count += 1;
        entry.last_used = Instant::now();
    }

    /// Compute a recency-weighted score for the named skill.
    ///
    /// Formula: `count * max(0.5^(days/7), 0.1)`
    ///
    /// Returns 0.0 if the skill has never been used.
    pub fn score(&self, name: &str) -> f64 {
        let data = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match data.get(name) {
            Some(usage) => {
                let elapsed_secs = usage.last_used.elapsed().as_secs_f64();
                let days = elapsed_secs / 86400.0;
                let decay = (0.5_f64).powf(days / 7.0).max(0.1);
                usage.count as f64 * decay
            }
            None => 0.0,
        }
    }

    /// Get the invocation count for a skill.
    pub fn count(&self, name: &str) -> u64 {
        let data = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        data.get(name).map_or(0, |d| d.count)
    }
}

impl Default for SkillUsageTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "usage.test.rs"]
mod tests;
