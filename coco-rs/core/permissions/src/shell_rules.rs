//! Shell permission rule parsing and matching.
//!
//! TS: utils/permissions/shellRuleMatching.ts
//!
//! Three rule types for permission matching:
//! - Exact: "git commit" matches only "git commit"
//! - Prefix: "git " or "git:*" matches "git commit", "git push", etc.
//! - Wildcard: "git *" matches any git subcommand (with escape support)

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex;

/// A parsed shell permission rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShellPermissionRule {
    /// Exact command match.
    Exact { command: String },
    /// Prefix match (command starts with this string).
    Prefix { prefix: String },
    /// Wildcard match using glob-like patterns with escape support.
    Wildcard { pattern: String },
}

/// Extract prefix from legacy `:*` syntax (e.g., "npm:*" → "npm").
///
/// TS: `permissionRuleExtractPrefix()` in shellRuleMatching.ts
fn extract_legacy_prefix(rule: &str) -> Option<&str> {
    rule.strip_suffix(":*")
}

/// Check if a pattern contains unescaped wildcards (not legacy `:*` syntax).
///
/// A `*` is unescaped if preceded by an even number of backslashes (including 0).
///
/// TS: `hasWildcards()` in shellRuleMatching.ts
fn has_wildcards(pattern: &str) -> bool {
    // Legacy :* is not a wildcard
    if pattern.ends_with(":*") {
        return false;
    }

    let bytes = pattern.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'*' {
            let mut backslash_count = 0;
            let mut j = i;
            while j > 0 {
                j -= 1;
                if bytes[j] == b'\\' {
                    backslash_count += 1;
                } else {
                    break;
                }
            }
            // Even number of backslashes → unescaped star
            if backslash_count % 2 == 0 {
                return true;
            }
        }
    }
    false
}

impl ShellPermissionRule {
    /// Parse a rule string into a ShellPermissionRule.
    ///
    /// TS: `parsePermissionRule()` in shellRuleMatching.ts
    ///
    /// Rules:
    /// - Ends with `:*` → Prefix (legacy syntax, e.g. "npm:*" → prefix "npm")
    /// - Contains unescaped `*` → Wildcard
    /// - Ends with space → Prefix
    /// - Otherwise → Exact
    pub fn parse(rule: &str) -> Self {
        // Legacy :* prefix syntax first (backwards compatibility)
        if let Some(prefix) = extract_legacy_prefix(rule) {
            return Self::Prefix {
                prefix: prefix.to_string(),
            };
        }

        // New wildcard syntax (contains unescaped *)
        if has_wildcards(rule) {
            return Self::Wildcard {
                pattern: rule.to_string(),
            };
        }

        // Trailing space → prefix
        if rule.ends_with(' ') {
            return Self::Prefix {
                prefix: rule.to_string(),
            };
        }

        Self::Exact {
            command: rule.to_string(),
        }
    }

    /// Check if a command matches this rule.
    pub fn matches(&self, command: &str) -> bool {
        match self {
            Self::Exact { command: expected } => command == expected,
            Self::Prefix { prefix } => command.starts_with(prefix.as_str()),
            Self::Wildcard { pattern } => match_wildcard_pattern(pattern, command),
        }
    }
}

/// Process-wide cache of compiled wildcard regexes keyed by raw rule pattern.
///
/// Permission checks run on the hot path (every bash invocation); recompiling
/// the regex each time adds up. `None` is cached for patterns that fail to
/// compile so we don't retry.
///
/// Bounded at `WILDCARD_REGEX_CACHE_MAX` entries. Once full, new patterns are
/// compiled on-demand but NOT cached — this caps memory at O(cap × avg regex
/// size) even if a compromised plugin feeds unbounded unique patterns. In
/// practice rule sets are small (~dozens), so the cap is rarely hit.
static WILDCARD_REGEX_CACHE: LazyLock<Mutex<HashMap<String, Option<regex::Regex>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const WILDCARD_REGEX_CACHE_MAX: usize = 1024;

/// Match a command against a wildcard pattern with escape support.
///
/// TS: `matchWildcardPattern()` in shellRuleMatching.ts
///
/// - `*` matches any sequence of characters (including newlines)
/// - `\*` matches a literal `*`
/// - `\\` matches a literal `\`
/// - Trailing ` *` (space + single wildcard) is optional — `git *` matches bare `git`
fn match_wildcard_pattern(pattern: &str, command: &str) -> bool {
    // PoisonError handling: if a thread panicked while holding the cache lock,
    // the cache contents are still consistent (we only store Option<Regex> and
    // never break invariants mid-write). Recover via `into_inner()`.
    {
        let cache = WILDCARD_REGEX_CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = cache.get(pattern) {
            return entry.as_ref().is_some_and(|re| re.is_match(command));
        }
    }

    let compiled = compile_wildcard_regex(pattern);
    let result = compiled.as_ref().is_some_and(|re| re.is_match(command));
    let mut cache = WILDCARD_REGEX_CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cache.len() < WILDCARD_REGEX_CACHE_MAX {
        cache.insert(pattern.to_string(), compiled);
    }
    // Cache full: skip insertion — the pattern will be recompiled on each call,
    // which is acceptable degradation compared to unbounded memory growth.
    result
}

/// Build a regex from a wildcard rule pattern. Returns `None` if compilation
/// fails (invalid pattern — we log once and treat as no-match).
fn compile_wildcard_regex(pattern: &str) -> Option<regex::Regex> {
    let trimmed = pattern.trim();

    // Phase 1: Process escape sequences, collecting regex-ready segments
    // Use sentinel markers for escaped chars to avoid interference with regex escaping
    const ESCAPED_STAR: &str = "\x00ES\x00";
    const ESCAPED_BACKSLASH: &str = "\x00EB\x00";

    let mut processed = String::with_capacity(trimmed.len());
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    let mut unescaped_star_count = 0;

    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'*' => {
                    processed.push_str(ESCAPED_STAR);
                    i += 2;
                    continue;
                }
                b'\\' => {
                    processed.push_str(ESCAPED_BACKSLASH);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        if bytes[i] == b'*' {
            unescaped_star_count += 1;
        }
        processed.push(bytes[i] as char);
        i += 1;
    }

    // Phase 2: Escape regex special chars (except *)
    let escaped = regex_escape_except_star(&processed);

    // Phase 3: Convert unescaped * to .*
    let with_wildcards = escaped.replace('*', ".*");

    // Phase 4: Restore escaped literals
    let mut regex_pattern = with_wildcards
        .replace(ESCAPED_STAR, r"\*")
        .replace(ESCAPED_BACKSLASH, r"\\");

    // Phase 5: Trailing ` *` with single wildcard → optional
    // "git *" matches both "git add" and bare "git"
    if regex_pattern.ends_with(" .*") && unescaped_star_count == 1 {
        let len = regex_pattern.len();
        regex_pattern.replace_range(len - 3.., "( .*)?");
    }

    // Phase 6: Match entire string with dotAll semantics
    let full_pattern = format!("(?s)^{regex_pattern}$");
    match regex::Regex::new(&full_pattern) {
        Ok(re) => Some(re),
        Err(e) => {
            tracing::warn!(
                pattern = %pattern,
                "invalid wildcard pattern, regex compilation failed: {e}"
            );
            None
        }
    }
}

/// Escape regex special characters except `*`.
fn regex_escape_except_star(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '.' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\' | '\''
            | '"' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Check if a Bash rule_content matches a command.
/// This is the entry point for content-specific permission matching (step 3).
pub fn matches_bash_rule(rule_content: &str, command: &str) -> bool {
    let rule = ShellPermissionRule::parse(rule_content);
    rule.matches(command)
}

/// Check if a rule_content represents a dangerous bash permission.
///
/// Delegates to `setup::is_dangerous_bash_permission` for the full pattern list.
pub fn is_dangerous_bash_permission(rule_content: &str) -> bool {
    use coco_types::ToolName;
    crate::setup::is_dangerous_bash_permission(
        ToolName::Bash.as_str(),
        Some(rule_content),
        /*is_ant_user*/ false,
    )
}

/// Generate a permission update suggestion for an exact command.
///
/// TS: `suggestionForExactCommand()` in shellRuleMatching.ts
pub fn suggestion_for_exact_command(
    tool_name: &str,
    command: &str,
) -> coco_types::PermissionUpdate {
    coco_types::PermissionUpdate::AddRules {
        rules: vec![coco_types::PermissionRule {
            source: coco_types::PermissionRuleSource::LocalSettings,
            behavior: coco_types::PermissionBehavior::Allow,
            value: coco_types::PermissionRuleValue {
                tool_pattern: tool_name.to_string(),
                rule_content: Some(command.to_string()),
            },
        }],
        destination: coco_types::PermissionUpdateDestination::LocalSettings,
    }
}

/// Generate a permission update suggestion for a prefix match.
///
/// TS: `suggestionForPrefix()` in shellRuleMatching.ts
pub fn suggestion_for_prefix(tool_name: &str, prefix: &str) -> coco_types::PermissionUpdate {
    coco_types::PermissionUpdate::AddRules {
        rules: vec![coco_types::PermissionRule {
            source: coco_types::PermissionRuleSource::LocalSettings,
            behavior: coco_types::PermissionBehavior::Allow,
            value: coco_types::PermissionRuleValue {
                tool_pattern: tool_name.to_string(),
                rule_content: Some(format!("{prefix}:*")),
            },
        }],
        destination: coco_types::PermissionUpdateDestination::LocalSettings,
    }
}

#[cfg(test)]
#[path = "shell_rules.test.rs"]
mod tests;
