//! Frontmatter parser for markdown files.
//!
//! Extracts and parses YAML frontmatter between `---` delimiters.
//! Used by skills, commands, agents, memory files, and output styles.
//!
//! TS: `src/utils/frontmatterParser.ts` (370 LOC)

use std::collections::HashMap;

/// Parsed frontmatter result.
#[derive(Debug, Clone)]
pub struct Frontmatter {
    /// Parsed key-value data from YAML frontmatter.
    pub data: HashMap<String, FrontmatterValue>,
    /// Markdown content after the frontmatter block.
    pub content: String,
}

/// A value in the frontmatter YAML.
#[derive(Debug, Clone, PartialEq)]
pub enum FrontmatterValue {
    /// String value.
    String(String),
    /// Boolean value.
    Bool(bool),
    /// Integer value.
    Int(i64),
    /// List of strings.
    StringList(Vec<String>),
    /// Null/empty value (key with no value, e.g., `key:`).
    Null,
}

impl FrontmatterValue {
    /// Get as string, if it is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            FrontmatterValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Get as bool, if it is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            FrontmatterValue::Bool(b) => Some(*b),
            FrontmatterValue::String(s) => match s.to_lowercase().as_str() {
                "true" | "yes" | "1" => Some(true),
                "false" | "no" | "0" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    /// Get as string list, if it is one. Single strings become a 1-element list.
    pub fn as_string_list(&self) -> Option<Vec<&str>> {
        match self {
            FrontmatterValue::StringList(list) => Some(list.iter().map(String::as_str).collect()),
            FrontmatterValue::String(s) => Some(vec![s.as_str()]),
            _ => None,
        }
    }
}

/// Parse markdown file content, extracting YAML frontmatter.
///
/// Frontmatter is delimited by `---` on its own line at the start of the file.
/// Returns the parsed data and the remaining markdown content.
///
/// ```
/// # use coco_frontmatter::parse;
/// let md = "---\ntitle: Hello\n---\n# Body";
/// let fm = parse(md);
/// assert_eq!(fm.data.get("title").unwrap().as_str(), Some("Hello"));
/// assert_eq!(fm.content.trim(), "# Body");
/// ```
pub fn parse(input: &str) -> Frontmatter {
    let trimmed = input.trim_start();

    // Must start with ---
    if !trimmed.starts_with("---") {
        return Frontmatter {
            data: HashMap::new(),
            content: input.to_string(),
        };
    }

    // Find the closing ---
    let after_first = &trimmed[3..];
    let after_first = after_first.strip_prefix('\n').unwrap_or(after_first);

    if let Some(end_pos) = after_first.find("\n---") {
        let yaml_block = &after_first[..end_pos];
        let content_start = end_pos + 4; // skip \n---
        let content = after_first[content_start..]
            .strip_prefix('\n')
            .unwrap_or(&after_first[content_start..]);

        let data = parse_yaml_simple(yaml_block);

        Frontmatter {
            data,
            content: content.to_string(),
        }
    } else {
        // No closing delimiter — treat entire content as body
        Frontmatter {
            data: HashMap::new(),
            content: input.to_string(),
        }
    }
}

/// Simple YAML key-value parser (no nested objects, no complex types).
///
/// Handles:
/// - `key: value` (string)
/// - `key: true/false` (bool)
/// - `key:` (null)
/// - `key: 123` (integer)
/// - Multi-line list items (`- item`)
fn parse_yaml_simple(yaml: &str) -> HashMap<String, FrontmatterValue> {
    let mut map = HashMap::new();
    let mut current_key: Option<String> = None;
    let mut list_items: Vec<String> = Vec::new();

    for line in yaml.lines() {
        let trimmed = line.trim();

        // List item (continuation of previous key)
        if let Some(rest) = trimmed.strip_prefix("- ") {
            let item = rest.trim().to_string();
            // Strip quotes
            let item = strip_quotes(&item);
            list_items.push(item);
            continue;
        }

        // Flush pending list
        if !list_items.is_empty() {
            if let Some(ref key) = current_key {
                map.insert(
                    key.clone(),
                    FrontmatterValue::StringList(list_items.clone()),
                );
            }
            list_items.clear();
        }

        // Key: value pair
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos].trim().to_string();
            let value = trimmed[colon_pos + 1..].trim();

            current_key = Some(key.clone());

            if value.is_empty() {
                map.insert(key, FrontmatterValue::Null);
            } else {
                map.insert(key, parse_value(value));
            }
        }
    }

    // Flush final pending list
    if !list_items.is_empty()
        && let Some(ref key) = current_key
    {
        map.insert(key.clone(), FrontmatterValue::StringList(list_items));
    }

    map
}

/// Parse a single YAML value string.
fn parse_value(s: &str) -> FrontmatterValue {
    match s.to_lowercase().as_str() {
        "true" | "yes" => FrontmatterValue::Bool(true),
        "false" | "no" => FrontmatterValue::Bool(false),
        "null" | "~" => FrontmatterValue::Null,
        _ => {
            // Try integer
            if let Ok(n) = s.parse::<i64>() {
                return FrontmatterValue::Int(n);
            }
            // String (strip quotes if present)
            FrontmatterValue::String(strip_quotes(s))
        }
    }
}

/// Strip surrounding quotes (single or double) from a string.
fn strip_quotes(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
