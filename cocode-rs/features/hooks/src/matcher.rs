//! Hook matchers for filtering which invocations trigger a hook.
//!
//! Matchers inspect a string value (typically a tool name) to decide whether
//! the hook applies.

use serde::Deserialize;
use serde::Serialize;

use crate::error::HookError;
use crate::error::hook_error::*;

/// A matcher that determines whether a hook should fire for a given value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookMatcher {
    /// Matches an exact string value.
    Exact { value: String },

    /// Matches using a glob-style wildcard pattern.
    /// Supports `*` (any characters) and `?` (single character).
    Wildcard { pattern: String },

    /// Matches if any of the inner matchers match.
    Or { matchers: Vec<HookMatcher> },

    /// Matches using a regular expression.
    Regex { pattern: String },

    /// Matches everything.
    All,
}

impl HookMatcher {
    /// Returns `true` if the given value matches this matcher.
    ///
    /// For `Regex` patterns that fail to compile, returns `false` and logs a
    /// warning.
    pub fn matches(&self, value: &str) -> bool {
        match self {
            Self::Exact { value: expected } => value == expected,
            Self::Wildcard { pattern } => wildcard_matches(pattern, value),
            Self::Or { matchers } => matchers.iter().any(|m| m.matches(value)),
            Self::Regex { pattern } => match regex::Regex::new(pattern) {
                Ok(re) => re.is_match(value),
                Err(e) => {
                    tracing::warn!("Invalid regex pattern '{pattern}': {e}");
                    false
                }
            },
            Self::All => true,
        }
    }

    /// Validates this matcher, returning an error if it contains invalid
    /// patterns.
    pub fn validate(&self) -> Result<(), HookError> {
        match self {
            Self::Regex { pattern } => {
                regex::Regex::new(pattern).map_err(|e| {
                    InvalidMatcherSnafu {
                        message: format!("invalid regex '{pattern}': {e}"),
                    }
                    .build()
                })?;
                Ok(())
            }
            Self::Or { matchers } => {
                for m in matchers {
                    m.validate()?;
                }
                Ok(())
            }
            Self::Exact { .. } | Self::Wildcard { .. } | Self::All => Ok(()),
        }
    }
}

/// Simple glob-style wildcard matching.
///
/// `*` matches zero or more characters and `?` matches exactly one character.
fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let value_chars: Vec<char> = value.chars().collect();
    wildcard_recursive(&pattern_chars, &value_chars, 0, 0)
}

fn wildcard_recursive(pattern: &[char], value: &[char], pi: usize, vi: usize) -> bool {
    if pi == pattern.len() {
        return vi == value.len();
    }

    if pattern[pi] == '*' {
        // Try matching zero or more characters
        let mut v = vi;
        while v <= value.len() {
            if wildcard_recursive(pattern, value, pi + 1, v) {
                return true;
            }
            v += 1;
        }
        return false;
    }

    if vi < value.len() && (pattern[pi] == '?' || pattern[pi] == value[vi]) {
        return wildcard_recursive(pattern, value, pi + 1, vi + 1);
    }

    false
}

#[cfg(test)]
#[path = "matcher.test.rs"]
mod tests;
