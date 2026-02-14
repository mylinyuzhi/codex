//! Permission rule evaluation for tool calls.
//!
//! Provides [`PermissionRuleEvaluator`] which evaluates a set of rules
//! against tool calls to produce permission decisions. Rules are matched
//! by tool name pattern and optional file path glob, with priority based
//! on [`RuleSource`] and action severity (deny > ask > allow).

use std::path::Path;

use cocode_config::PermissionsConfig;
use cocode_protocol::PermissionDecision;
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
#[derive(Debug, Clone)]
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

/// Evaluates permission rules against tool calls.
///
/// Rules are evaluated in priority order: source priority first (Session > Command > ... > User),
/// then action severity (Deny > Ask > Allow). The first matching rule wins.
#[derive(Debug, Clone, Default)]
pub struct PermissionRuleEvaluator {
    rules: Vec<PermissionRule>,
}

impl PermissionRuleEvaluator {
    /// Create an empty evaluator.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Create an evaluator with pre-loaded rules.
    pub fn with_rules(rules: Vec<PermissionRule>) -> Self {
        Self { rules }
    }

    /// Add a single rule.
    pub fn add_rule(&mut self, rule: PermissionRule) {
        self.rules.push(rule);
    }

    /// Build permission rules from a `PermissionsConfig` with a given source.
    pub fn rules_from_config(
        config: &PermissionsConfig,
        source: RuleSource,
    ) -> Vec<PermissionRule> {
        let mut rules = Vec::new();
        for pattern in &config.allow {
            rules.push(PermissionRule {
                source,
                tool_pattern: pattern.clone(),
                file_pattern: None,
                action: RuleAction::Allow,
            });
        }
        for pattern in &config.deny {
            rules.push(PermissionRule {
                source,
                tool_pattern: pattern.clone(),
                file_pattern: None,
                action: RuleAction::Deny,
            });
        }
        for pattern in &config.ask {
            rules.push(PermissionRule {
                source,
                tool_pattern: pattern.clone(),
                file_pattern: None,
                action: RuleAction::Ask,
            });
        }
        rules
    }

    /// Evaluate rules for a tool call.
    ///
    /// Returns `None` if no rule matches (fall through to the tool's own check).
    pub fn evaluate(
        &self,
        tool_name: &str,
        file_path: Option<&Path>,
    ) -> Option<PermissionDecision> {
        let mut matching_rules: Vec<&PermissionRule> = self
            .rules
            .iter()
            .filter(|r| Self::matches_tool(&r.tool_pattern, tool_name))
            .filter(|r| Self::matches_file(&r.file_pattern, file_path))
            .collect();

        // Sort by source priority (lower ordinal = higher priority), then by
        // action severity (Deny=0 < Ask=1 < Allow=2, so most restrictive first).
        matching_rules.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then(Self::action_priority(&a.action).cmp(&Self::action_priority(&b.action)))
        });

        matching_rules.first().map(|rule| match rule.action {
            RuleAction::Allow => PermissionDecision::allowed(format!(
                "Allowed by {source} rule for {tool_name}",
                source = rule.source
            ))
            .with_source(rule.source)
            .with_pattern(rule.tool_pattern.clone()),

            RuleAction::Deny => PermissionDecision::denied(format!(
                "Denied by {source} rule for {tool_name}",
                source = rule.source
            ))
            .with_source(rule.source)
            .with_pattern(rule.tool_pattern.clone()),

            RuleAction::Ask => {
                // Ask means "fall through to tool's own permission check".
                // We return Allowed here so the tool's check_permission() is
                // the one that decides whether to ask or allow.
                PermissionDecision::allowed(format!(
                    "Ask rule from {source} — delegating to tool check",
                    source = rule.source
                ))
                .with_source(rule.source)
                .with_pattern(rule.tool_pattern.clone())
            }
        })
    }

    /// Evaluate rules for a specific behavior (deny/ask/allow).
    ///
    /// Used by the permission pipeline for staged evaluation:
    /// 1. Check DENY rules → if match → Deny
    /// 2. Check ASK rules → if match → NeedsApproval
    /// 3. (tool-specific check)
    /// 4. Check ALLOW rules → if match → Allow
    ///
    /// `command_input` is the actual command string for Bash-type tools,
    /// used to match patterns like `"Bash:git *"` or `"Bash(npm run *)"`.
    ///
    /// Returns the highest-priority matching rule for the given action,
    /// or `None` if no rule of that action type matches.
    pub fn evaluate_behavior(
        &self,
        tool_name: &str,
        file_path: Option<&Path>,
        action: RuleAction,
        command_input: Option<&str>,
    ) -> Option<PermissionDecision> {
        self.rules
            .iter()
            .filter(|r| r.action == action)
            .filter(|r| Self::matches_tool_with_input(&r.tool_pattern, tool_name, command_input))
            .filter(|r| Self::matches_file(&r.file_pattern, file_path))
            .min_by_key(|r| r.source) // Highest-priority source wins
            .map(|rule| match rule.action {
                RuleAction::Allow => PermissionDecision::allowed(format!(
                    "Allowed by {source} rule for {tool_name}",
                    source = rule.source
                ))
                .with_source(rule.source)
                .with_pattern(rule.tool_pattern.clone()),

                RuleAction::Deny => PermissionDecision::denied(format!(
                    "Denied by {source} rule for {tool_name}",
                    source = rule.source
                ))
                .with_source(rule.source)
                .with_pattern(rule.tool_pattern.clone()),

                RuleAction::Ask => PermissionDecision::allowed(format!(
                    "Ask rule from {source} for {tool_name}",
                    source = rule.source
                ))
                .with_source(rule.source)
                .with_pattern(rule.tool_pattern.clone()),
            })
    }

    /// Check if `pattern` matches `tool_name`.
    fn matches_tool(pattern: &str, tool_name: &str) -> bool {
        Self::matches_tool_with_input(pattern, tool_name, None)
    }

    /// Check if `pattern` matches `tool_name`, optionally checking
    /// a command pattern against `command_input`.
    ///
    /// Pattern formats:
    /// - `"Bash"` → matches tool name "Bash"
    /// - `"Bash:git *"` → matches Bash tool + commands starting with "git "
    /// - `"Bash(npm run *)"` → parenthesized form, same as colon
    /// - `"*"` → matches all tools
    #[allow(clippy::unwrap_used)]
    fn matches_tool_with_input(
        pattern: &str,
        tool_name: &str,
        command_input: Option<&str>,
    ) -> bool {
        if pattern == "*" {
            return true;
        }

        // Parse "Tool:command_pattern" or "Tool(command_pattern)" forms
        let (tool_part, cmd_pattern) = if pattern.contains(':') {
            let parts: Vec<&str> = pattern.splitn(2, ':').collect();
            (parts[0], Some(parts[1]))
        } else if pattern.ends_with(')') && pattern.contains('(') {
            let paren_idx = pattern.find('(').unwrap();
            let tool = &pattern[..paren_idx];
            let cmd = &pattern[paren_idx + 1..pattern.len() - 1];
            (tool, Some(cmd))
        } else {
            (pattern, None)
        };

        if tool_part != tool_name {
            return false;
        }

        // If there's a command pattern, check it against the input
        match (cmd_pattern, command_input) {
            (None, _) => true,       // No command pattern — tool name match is sufficient
            (Some(_), None) => true, // Has pattern but no input to check — match on tool name
            (Some(pat), Some(cmd)) => Self::matches_command_pattern(pat, cmd),
        }
    }

    /// Check if a command matches a wildcard pattern.
    ///
    /// Supports trailing `*` wildcards:
    /// - `"git *"` matches "git status", "git push"
    /// - `"npm run *"` matches "npm run test", "npm run build"
    /// - `"exact-command"` matches exactly
    fn matches_command_pattern(pattern: &str, command: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix(" *") {
            command == prefix || command.starts_with(&format!("{prefix} "))
        } else if let Some(prefix) = pattern.strip_suffix('*') {
            command.starts_with(prefix)
        } else {
            command == pattern
        }
    }

    /// Check if `file_path` matches `pattern`.
    fn matches_file(pattern: &Option<String>, file_path: Option<&Path>) -> bool {
        match (pattern, file_path) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(pat), Some(path)) => {
                let path_str = path.to_string_lossy();
                if pat == "*" {
                    return true;
                }
                // Extension match: "*.rs"
                if pat.starts_with("*.") {
                    let ext = &pat[1..];
                    return path_str.ends_with(ext);
                }
                // Double-star glob: "src/**/*.ts"
                if pat.contains("**") {
                    let parts: Vec<&str> = pat.split("**").collect();
                    if parts.len() == 2 {
                        let prefix = parts[0].trim_end_matches('/');
                        let suffix = parts[1].trim_start_matches('/');
                        let prefix_ok = prefix.is_empty() || path_str.starts_with(prefix);
                        let suffix_ok = if suffix.is_empty() {
                            true
                        } else if suffix.starts_with("*.") {
                            // Extension glob in suffix: "*.ts" matches ".ts" extension
                            let ext = &suffix[1..]; // ".ts"
                            path_str.ends_with(ext)
                        } else {
                            path_str.ends_with(suffix)
                        };
                        return prefix_ok && suffix_ok;
                    }
                }
                // Substring match fallback.
                path_str.contains(pat)
            }
        }
    }

    /// Lower number = higher priority (more restrictive).
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn action_priority(action: &RuleAction) -> i32 {
        match action {
            RuleAction::Deny => 0,
            RuleAction::Ask => 1,
            RuleAction::Allow => 2,
        }
    }
}

#[cfg(test)]
#[path = "permission_rules.test.rs"]
mod tests;
