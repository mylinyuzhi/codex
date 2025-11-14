//! Search-and-replace strategies for smart edit.
//!
//! Implements three progressive matching strategies:
//! 1. Exact - literal string matching
//! 2. Flexible - line-by-line with trimmed whitespace, preserves indentation
//! 3. Regex - token-based flexible regex matching
//!
//! Ported from gemini-cli's smart-edit implementation.

use regex_lite::Regex;

/// Result of a replacement operation
#[derive(Debug, Clone)]
pub struct ReplacementResult {
    pub new_content: String,
    pub occurrences: i32,
    pub strategy: &'static str,
}

/// Try all three strategies in order: exact → flexible → regex
pub fn try_all_strategies(old_string: &str, new_string: &str, content: &str) -> ReplacementResult {
    // Early return for empty search string (invalid)
    if old_string.is_empty() {
        return ReplacementResult {
            new_content: content.to_string(),
            occurrences: 0,
            strategy: "none",
        };
    }

    // Normalize line endings to \n
    let normalized_content = content.replace("\r\n", "\n");
    let normalized_old = old_string.replace("\r\n", "\n");
    let normalized_new = new_string.replace("\r\n", "\n");

    // Strategy 1: Exact match
    if let Some(result) =
        try_exact_replacement(&normalized_old, &normalized_new, &normalized_content)
    {
        return ReplacementResult {
            new_content: restore_trailing_newline(content, &result.0),
            occurrences: result.1,
            strategy: "exact",
        };
    }

    // Strategy 2: Flexible match
    if let Some(result) =
        try_flexible_replacement(&normalized_old, &normalized_new, &normalized_content)
    {
        return ReplacementResult {
            new_content: restore_trailing_newline(content, &result.0),
            occurrences: result.1,
            strategy: "flexible",
        };
    }

    // Strategy 3: Regex match
    if let Some(result) =
        try_regex_replacement(&normalized_old, &normalized_new, &normalized_content)
    {
        return ReplacementResult {
            new_content: restore_trailing_newline(content, &result.0),
            occurrences: result.1,
            strategy: "regex",
        };
    }

    // All strategies failed
    ReplacementResult {
        new_content: content.to_string(),
        occurrences: 0,
        strategy: "none",
    }
}

/// Strategy 1: Exact literal string replacement
fn try_exact_replacement(
    old_string: &str,
    new_string: &str,
    content: &str,
) -> Option<(String, i32)> {
    let occurrences = content.matches(old_string).count() as i32;
    if occurrences > 0 {
        let new_content = content.replace(old_string, new_string);
        Some((new_content, occurrences))
    } else {
        None
    }
}

/// Strategy 2: Flexible replacement with whitespace trimming and indentation preservation
fn try_flexible_replacement(
    old_string: &str,
    new_string: &str,
    content: &str,
) -> Option<(String, i32)> {
    // Split content into lines (preserving line structure)
    let source_lines: Vec<&str> = content.lines().collect();
    let search_lines_stripped: Vec<String> = old_string
        .lines()
        .map(|line| line.trim().to_string())
        .collect();
    let replace_lines: Vec<&str> = new_string.lines().collect();

    if search_lines_stripped.is_empty() {
        return None;
    }

    let mut result_lines = Vec::new();
    let mut occurrences = 0;
    let mut i = 0;

    while i
        <= source_lines
            .len()
            .saturating_sub(search_lines_stripped.len())
    {
        // Check if we have a match at position i
        let window = &source_lines[i..i + search_lines_stripped.len()];
        let window_stripped: Vec<String> =
            window.iter().map(|line| line.trim().to_string()).collect();

        let is_match = window_stripped
            .iter()
            .zip(&search_lines_stripped)
            .all(|(w, s)| w == s);

        if is_match {
            occurrences += 1;

            // Extract indentation from first line of match
            let first_line = window[0];
            let indentation = extract_indentation(first_line);

            // Apply replacement with preserved indentation
            for line in &replace_lines {
                result_lines.push(format!("{indentation}{line}"));
            }

            i += search_lines_stripped.len();
        } else {
            result_lines.push(source_lines[i].to_string());
            i += 1;
        }
    }

    // Add remaining lines
    while i < source_lines.len() {
        result_lines.push(source_lines[i].to_string());
        i += 1;
    }

    if occurrences > 0 {
        Some((result_lines.join("\n"), occurrences))
    } else {
        None
    }
}

/// Strategy 3: Token-based regex replacement (first occurrence only)
fn try_regex_replacement(
    old_string: &str,
    new_string: &str,
    content: &str,
) -> Option<(String, i32)> {
    // Tokenize the search string by splitting on delimiters
    let delimiters = ['(', ')', ':', '[', ']', '{', '}', '>', '<', '='];
    let mut tokenized = old_string.to_string();
    for delim in delimiters {
        tokenized = tokenized.replace(delim, &format!(" {delim} "));
    }

    // Split by whitespace and filter empty tokens
    let tokens: Vec<&str> = tokenized.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    // Escape regex special characters in tokens
    let escaped_tokens: Vec<String> = tokens.iter().map(|t| escape_regex(t)).collect();

    // Join tokens with flexible whitespace pattern
    let pattern = escaped_tokens.join(r"\s*");

    // Build final pattern that captures leading indentation
    let final_pattern = format!(r"^(\s*){pattern}");

    // Compile regex
    let regex = Regex::new(&final_pattern).ok()?;

    // Find match
    let captures = regex.captures(content)?;
    let indentation = captures.get(1).map(|m| m.as_str()).unwrap_or("");

    // Apply replacement with preserved indentation
    let new_lines: Vec<String> = new_string
        .lines()
        .map(|line| format!("{indentation}{line}"))
        .collect();
    let new_block = new_lines.join("\n");

    // Replace only first occurrence
    let new_content = regex.replace(content, new_block.as_str()).to_string();

    Some((new_content, 1)) // Regex strategy only replaces first occurrence
}

/// Extract leading whitespace from a line
fn extract_indentation(line: &str) -> &str {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    &line[..indent_len]
}

/// Restore original trailing newline if present
fn restore_trailing_newline(original: &str, modified: &str) -> String {
    let had_trailing_newline = original.ends_with('\n');
    if had_trailing_newline && !modified.ends_with('\n') {
        format!("{modified}\n")
    } else if !had_trailing_newline && modified.ends_with('\n') {
        modified.trim_end_matches('\n').to_string()
    } else {
        modified.to_string()
    }
}

/// Escape regex special characters
fn escape_regex(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\' => {
                format!("\\{c}")
            }
            _ => c.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_replacement() {
        let content = "fn hello() {\n    println!(\"hello\");\n}";
        let old = "hello";
        let new = "goodbye";

        let result = try_exact_replacement(old, new, content);
        assert!(result.is_some());
        let (new_content, count) = result.unwrap();
        assert_eq!(count, 2); // Matches in function name and string
        assert!(new_content.contains("goodbye"));
    }

    #[test]
    fn test_flexible_replacement_indentation() {
        let content = "fn test() {\n    old_code();\n}";
        let old = "old_code();"; // No indentation
        let new = "new_code();";

        let result = try_flexible_replacement(old, new, content);
        assert!(result.is_some());
        let (new_content, count) = result.unwrap();
        assert_eq!(count, 1);
        // Should preserve the 4-space indentation
        assert!(new_content.contains("    new_code();"));
    }

    #[test]
    fn test_all_strategies_fallback() {
        let content = "fn test() { code(); }";
        let old = "nonexistent";
        let new = "replacement";

        let result = try_all_strategies(old, new, content);
        assert_eq!(result.occurrences, 0);
        assert_eq!(result.strategy, "none");
        assert_eq!(result.new_content, content);
    }

    #[test]
    fn test_trailing_newline_preservation() {
        let with_newline = "content\n";
        let without_newline = "content";

        let restored_with = restore_trailing_newline(with_newline, "modified");
        assert_eq!(restored_with, "modified\n");

        let restored_without = restore_trailing_newline(without_newline, "modified\n");
        assert_eq!(restored_without, "modified");
    }
}
