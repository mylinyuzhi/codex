//! Parsing utilities for @mention support.
//!
//! Provides parsing for user prompt @mentions:
//! - File mentions: @file.txt, @"path with spaces", @file.txt:10-20
//! - Agent mentions: @agent-search, @agent-edit

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use regex_lite::Regex;

// ============================================
// Regex Patterns (compiled lazily)
// ============================================

fn quoted_file_regex() -> Regex {
    Regex::new(r#"(?:^|\s)@"([^"]+)""#).expect("valid quoted file regex")
}

fn unquoted_file_regex() -> Regex {
    Regex::new(r#"(?:^|\s)@([^\s"@]+)"#).expect("valid unquoted file regex")
}

fn agent_mention_regex() -> Regex {
    Regex::new(r"(?:^|\s)@(agent-[\w:.@-]+)").expect("valid agent mention regex")
}

fn line_range_regex() -> Regex {
    Regex::new(r"^([^:]+)(?::(\d+)(?:-(\d+))?)?$").expect("valid line range regex")
}

// ============================================
// Types
// ============================================

/// A file mention parsed from user prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMention {
    /// Raw path string from the mention.
    pub raw_path: String,
    /// Line start (1-indexed, if specified).
    pub line_start: Option<i32>,
    /// Line end (1-indexed, if specified).
    pub line_end: Option<i32>,
    /// Whether the path was quoted.
    pub is_quoted: bool,
}

impl FileMention {
    /// Resolve the file mention to an absolute path.
    pub fn resolve(&self, cwd: &Path) -> PathBuf {
        let path = Path::new(&self.raw_path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        }
    }

    /// Check if this mention has a line range.
    pub fn has_line_range(&self) -> bool {
        self.line_start.is_some()
    }
}

/// An agent mention parsed from user prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMention {
    /// Agent type (e.g., "search", "edit").
    pub agent_type: String,
}

/// Result of parsing all @mentions from user prompt.
#[derive(Debug, Default)]
pub struct ParsedMentions {
    /// File mentions (@file, @"path", @file:10-20).
    pub files: Vec<FileMention>,
    /// Agent mentions (@agent-type).
    pub agents: Vec<AgentMention>,
}

// ============================================
// Parsing Functions
// ============================================

/// Parse all @mentions from user prompt.
///
/// Extracts file mentions and agent mentions from the user's message.
/// Deduplicates mentions and filters out agent mentions from file mentions.
pub fn parse_mentions(user_prompt: &str) -> ParsedMentions {
    let mut result = ParsedMentions::default();

    // First, extract agent mentions (they start with @agent-)
    let agent_mentions = parse_agent_mentions(user_prompt);
    let agent_strings: HashSet<String> = agent_mentions
        .iter()
        .map(|a| format!("agent-{}", a.agent_type))
        .collect();
    result.agents = agent_mentions;

    // Then extract file mentions, filtering out agent mentions
    result.files = parse_file_mentions(user_prompt)
        .into_iter()
        .filter(|f| !agent_strings.contains(&f.raw_path))
        .collect();

    result
}

/// Parse file mentions from user prompt.
///
/// Supports:
/// - @file.txt
/// - @"path with spaces"
/// - @path/to/file
/// - @file.txt:10
/// - @file.txt:10-20
pub fn parse_file_mentions(user_prompt: &str) -> Vec<FileMention> {
    let mut mentions = Vec::new();
    let mut seen = HashSet::new();

    let quoted_re = quoted_file_regex();
    let unquoted_re = unquoted_file_regex();
    let line_re = line_range_regex();

    // Parse quoted mentions first (higher priority)
    for cap in quoted_re.captures_iter(user_prompt) {
        if let Some(path_match) = cap.get(1) {
            let raw = path_match.as_str().to_string();
            if seen.insert(raw.clone()) {
                let (path, line_start, line_end) = parse_line_range_with_regex(&raw, &line_re);
                mentions.push(FileMention {
                    raw_path: path,
                    line_start,
                    line_end,
                    is_quoted: true,
                });
            }
        }
    }

    // Parse unquoted mentions
    for cap in unquoted_re.captures_iter(user_prompt) {
        if let Some(path_match) = cap.get(1) {
            let raw = path_match.as_str().to_string();
            // Skip if already seen (quoted version) or if it's an agent mention
            if raw.starts_with("agent-") {
                continue;
            }
            if seen.insert(raw.clone()) {
                let (path, line_start, line_end) = parse_line_range_with_regex(&raw, &line_re);
                mentions.push(FileMention {
                    raw_path: path,
                    line_start,
                    line_end,
                    is_quoted: false,
                });
            }
        }
    }

    mentions
}

/// Parse agent mentions from user prompt.
///
/// Supports: @agent-search, @agent-edit, @agent-custom
pub fn parse_agent_mentions(user_prompt: &str) -> Vec<AgentMention> {
    let mut mentions = Vec::new();
    let mut seen = HashSet::new();

    let agent_re = agent_mention_regex();

    for cap in agent_re.captures_iter(user_prompt) {
        if let Some(agent_match) = cap.get(1) {
            let full_type = agent_match.as_str();
            // Extract agent type (strip "agent-" prefix)
            let agent_type = full_type
                .strip_prefix("agent-")
                .unwrap_or(full_type)
                .to_string();

            if seen.insert(agent_type.clone()) {
                mentions.push(AgentMention { agent_type });
            }
        }
    }

    mentions
}

/// Parse line range from file path.
///
/// Input: "file.txt:10-20" -> ("file.txt", Some(10), Some(20))
/// Input: "file.txt:10" -> ("file.txt", Some(10), None)  // means "to EOF"
/// Input: "file.txt" -> ("file.txt", None, None)
fn parse_line_range_with_regex(input: &str, regex: &Regex) -> (String, Option<i32>, Option<i32>) {
    if let Some(caps) = regex.captures(input) {
        let path = caps
            .get(1)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        let line_start = caps.get(2).and_then(|m| m.as_str().parse::<i32>().ok());
        let line_end = caps.get(3).and_then(|m| m.as_str().parse::<i32>().ok());

        (path, line_start, line_end)
    } else {
        (input.to_string(), None, None)
    }
}

// ============================================
// Tests
// ============================================

#[cfg(test)]
#[path = "parsing.test.rs"]
mod tests;
