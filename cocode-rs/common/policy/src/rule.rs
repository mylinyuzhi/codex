//! Permission rule types.

use cocode_protocol::RuleSource;

/// Action to take when a permission rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    /// Deny the operation.
    Deny,
    /// Ask the user for permission.
    Ask,
    /// Allow the operation.
    Allow,
}

/// A single permission rule.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRule {
    /// Source of the rule (determines priority).
    pub source: RuleSource,
    /// Tool name pattern to match (e.g. `"Edit"`, `"Bash:git *"`, `"*"`).
    pub tool_pattern: String,
    /// Optional file path glob (e.g. `"*.rs"`, `"src/**/*.ts"`).
    pub file_pattern: Option<String>,
    /// Action to take when matched.
    pub action: RuleAction,
}

/// Check if a wildcard pattern matches a value.
///
/// Supports trailing `*` wildcards:
/// - `"*"` matches everything
/// - `"git *"` matches `"git"` and `"git push origin main"`
/// - `"npm*"` matches any string starting with `"npm"`
/// - exact string equality otherwise
pub(crate) fn matches_wildcard_pattern(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(" *") {
        value == prefix || value.starts_with(&format!("{prefix} "))
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}
