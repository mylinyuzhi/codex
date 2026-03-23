//! Permission rule evaluation for tool calls.
//!
//! [`PermissionRuleEvaluator`] evaluates a set of rules against tool calls to
//! produce permission decisions. Rules are matched by tool name pattern and
//! optional file path glob, with priority based on [`RuleSource`] and action
//! severity (deny > ask > allow).

use std::path::Path;

use cocode_config::PermissionsConfig;
use cocode_protocol::PermissionDecision;
use cocode_protocol::RuleSource;

use crate::rule::PermissionRule;
use crate::rule::RuleAction;

/// Evaluates permission rules against tool calls.
///
/// Rules are evaluated in staged pipeline order: deny rules first, then ask,
/// then allow. Within each stage, the source with the highest priority (lowest
/// ordinal: User > Project > Local > ... > Session) wins.
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

    /// Get a reference to the loaded rules.
    pub fn rules(&self) -> &[PermissionRule] {
        &self.rules
    }

    /// Check if there are no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Evaluate deny and ask rules in sequence (stages 1+2 of the pipeline).
    ///
    /// Returns `Some(decision)` if a deny or ask rule matched, `None` if
    /// neither matched (passthrough to tool-specific and allow checks).
    /// Use `decision.result.is_denied()` to distinguish deny from ask.
    pub fn evaluate_deny_ask(
        &self,
        tool_name: &str,
        file_path: Option<&Path>,
        command_input: Option<&str>,
    ) -> Option<PermissionDecision> {
        if let Some(d) =
            self.evaluate_behavior(tool_name, file_path, RuleAction::Deny, command_input)
        {
            return Some(d);
        }
        self.evaluate_behavior(tool_name, file_path, RuleAction::Ask, command_input)
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

    /// Evaluate rules for a specific behavior (deny/ask/allow).
    ///
    /// Used by the permission pipeline for staged evaluation:
    /// 1. Check DENY rules -> if match -> Deny
    /// 2. Check ASK rules -> if match -> NeedsApproval
    /// 3. (tool-specific check)
    /// 4. Check ALLOW rules -> if match -> Allow
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
        // For deny/ask rules: also try the normalized command to prevent
        // bypass via env var or wrapper prefixes (e.g., `LANG=C rm -rf /`).
        // For allow rules: only match the raw command to avoid over-allowance.
        let normalized = command_input.map(crate::normalize::normalize_command);
        let use_normalized = matches!(action, RuleAction::Deny | RuleAction::Ask);

        self.rules
            .iter()
            .filter(|r| r.action == action)
            .filter(|r| {
                Self::matches_tool_with_input(&r.tool_pattern, tool_name, command_input)
                    || (use_normalized
                        && Self::matches_tool_with_input(&r.tool_pattern, tool_name, normalized))
            })
            .filter(|r| Self::matches_file(&r.file_pattern, file_path))
            .min_by_key(|r| r.source) // Highest-priority source wins
            .map(|rule| Self::make_decision(rule, tool_name))
    }

    /// Check if `pattern` matches `tool_name`, optionally checking
    /// a command pattern against `command_input`.
    ///
    /// Pattern formats:
    /// - `"Bash"` -> matches tool name "Bash"
    /// - `"Bash:git *"` -> matches Bash tool + commands starting with "git "
    /// - `"Bash(npm run *)"` -> parenthesized form, same as colon
    /// - `"*"` -> matches all tools
    pub(crate) fn matches_tool_with_input(
        pattern: &str,
        tool_name: &str,
        command_input: Option<&str>,
    ) -> bool {
        if pattern == "*" {
            return true;
        }

        // Parse "Tool:command_pattern" or "Tool(command_pattern)" forms.
        // Empty patterns ("Bash:", "Bash()") are treated as bare tool matches.
        let (tool_part, cmd_pattern) = if let Some((tool, cmd)) = pattern.split_once(':') {
            (tool, if cmd.is_empty() { None } else { Some(cmd) })
        } else if let Some((tool, cmd)) = pattern.strip_suffix(')').and_then(|s| s.split_once('('))
        {
            (tool, if cmd.is_empty() { None } else { Some(cmd) })
        } else {
            (pattern, None)
        };

        // Use wildcard matching for tool names to support MCP server
        // wildcards like "mcp__github__*" matching "mcp__github__get_issues".
        if !crate::rule::matches_wildcard_pattern(tool_part, tool_name) {
            return false;
        }

        // If there's a command pattern, check it against the input
        match (cmd_pattern, command_input) {
            (None, _) => true,        // No command pattern — tool name match is sufficient
            (Some(_), None) => false, // Pattern requires input — can't match without it
            (Some(pat), Some(cmd)) => Self::matches_command_pattern(pat, cmd),
        }
    }

    /// Check if a command matches a wildcard pattern.
    ///
    /// Supports trailing `*` wildcards:
    /// - `"git *"` matches "git status", "git push"
    /// - `"npm run *"` matches "npm run test", "npm run build"
    /// - `"exact-command"` matches exactly
    pub(crate) fn matches_command_pattern(pattern: &str, command: &str) -> bool {
        crate::rule::matches_wildcard_pattern(pattern, command)
    }

    /// Check if `file_path` matches `pattern`.
    pub(crate) fn matches_file(pattern: &Option<String>, file_path: Option<&Path>) -> bool {
        match (pattern, file_path) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(pat), Some(path)) => {
                let path_str = path.to_string_lossy();
                if pat == "*" {
                    return true;
                }
                // Try glob matching first (handles *.rs, src/**/*.ts, etc.)
                if pat.contains('*') || pat.contains('?') || pat.contains('[') {
                    return Self::glob_match(pat, &path_str);
                }
                // Plain string: substring match.
                path_str.contains(pat)
            }
        }
    }

    /// Simple glob matching for file patterns.
    ///
    /// Supports:
    /// - `*` matches any sequence except `/`
    /// - `**` matches any sequence including `/`
    /// - `?` matches any single character except `/`
    fn glob_match(pattern: &str, path: &str) -> bool {
        // Split on ** first for recursive matching
        if let Some((before, after)) = pattern.split_once("**/") {
            let before = before.trim_end_matches('/');
            if !before.is_empty() && !path.starts_with(before) {
                return false;
            }
            let search_in = if before.is_empty() {
                path
            } else {
                path.strip_prefix(before).unwrap_or(path)
            };
            if after.is_empty() {
                return true;
            }
            // Match after-pattern against each path suffix starting at a '/'
            for (i, _) in search_in.match_indices('/') {
                let suffix = &search_in[i + 1..];
                if Self::simple_glob_match(after, suffix) {
                    return true;
                }
            }
            let trimmed = search_in.trim_start_matches('/');
            return Self::simple_glob_match(after, trimmed);
        }

        Self::simple_glob_match(pattern, path)
    }

    /// Simple glob matching without `**` (only `*` and `?`).
    fn simple_glob_match(pattern: &str, text: &str) -> bool {
        let pat_chars: Vec<char> = pattern.chars().collect();
        let text_chars: Vec<char> = text.chars().collect();
        let (mut px, mut tx) = (0usize, 0usize);
        let (mut star_px, mut star_tx) = (usize::MAX, 0usize);

        while tx < text_chars.len() {
            if px < pat_chars.len() && (pat_chars[px] == '?' || pat_chars[px] == text_chars[tx]) {
                px += 1;
                tx += 1;
            } else if px < pat_chars.len() && pat_chars[px] == '*' {
                star_px = px;
                star_tx = tx;
                px += 1;
            } else if star_px != usize::MAX {
                px = star_px + 1;
                star_tx += 1;
                tx = star_tx;
            } else {
                return false;
            }
        }

        while px < pat_chars.len() && pat_chars[px] == '*' {
            px += 1;
        }

        px == pat_chars.len()
    }

    fn make_decision(rule: &PermissionRule, tool_name: &str) -> PermissionDecision {
        match rule.action {
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

            RuleAction::Ask => PermissionDecision::ask(format!(
                "Ask rule from {source} for {tool_name}",
                source = rule.source
            ))
            .with_source(rule.source)
            .with_pattern(rule.tool_pattern.clone()),
        }
    }
}

#[cfg(test)]
#[path = "evaluator.test.rs"]
mod tests;
