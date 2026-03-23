//! Approval store for session-level permission memory.

use std::collections::HashSet;

use serde::Deserialize;
use serde::Serialize;

/// Stored approvals for tools.
///
/// Tracks which tool/pattern combinations have been approved during the
/// current session. Supports wildcard matching for prefix patterns.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalStore {
    /// Approved tool patterns (e.g., `"Bash:git *"`).
    approved_patterns: HashSet<String>,
    /// Session-wide approvals (e.g., `"Edit"`).
    session_approvals: HashSet<String>,
}

impl ApprovalStore {
    /// Create a new empty approval store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a tool action is approved.
    ///
    /// Supports wildcard matching: a stored pattern "git *" matches
    /// any value starting with "git " (or equal to "git").
    ///
    /// Precedence: exact key match → session-wide → wildcard patterns.
    pub fn is_approved(&self, tool_name: &str, pattern: &str) -> bool {
        let key = format!("{tool_name}:{pattern}");
        if self.approved_patterns.contains(&key) || self.session_approvals.contains(tool_name) {
            return true;
        }
        // Check wildcard patterns: stored "Bash:git *" matches query "Bash:git push origin main"
        let prefix = format!("{tool_name}:");
        self.approved_patterns.iter().any(|stored| {
            stored
                .strip_prefix(&prefix)
                .is_some_and(|pat| Self::matches_wildcard(pat, pattern))
        })
    }

    /// Check if a wildcard pattern matches a value.
    ///
    /// Supported patterns:
    /// - `"*"` matches everything
    /// - `"git *"` matches `"git"` and `"git push origin main"`
    /// - `"git*"` matches any string starting with `"git"`
    /// - exact string equality otherwise
    fn matches_wildcard(pattern: &str, value: &str) -> bool {
        crate::rule::matches_wildcard_pattern(pattern, value)
    }

    /// Add an approval for a specific pattern.
    pub fn approve_pattern(&mut self, tool_name: &str, pattern: &str) {
        let key = format!("{tool_name}:{pattern}");
        self.approved_patterns.insert(key);
    }

    /// Add a session-wide approval for a tool.
    pub fn approve_session(&mut self, tool_name: &str) {
        self.session_approvals.insert(tool_name.to_string());
    }

    /// Clear all approvals.
    pub fn clear(&mut self) {
        self.approved_patterns.clear();
        self.session_approvals.clear();
    }
}

#[cfg(test)]
#[path = "store.test.rs"]
mod tests;
