//! Common utilities for smart edit operations
//!
//! This module provides text manipulation, file operations, and hashing
//! utilities used by both matching strategies and LLM correction.

use regex_lite::Regex;
use sha2::Digest;
use sha2::Sha256;
use std::sync::LazyLock;

/// Safe literal string replacement
///
/// Rust's str::replace() is already literal (not regex), so this is mainly
/// for API consistency with gemini-cli and explicit documentation of behavior.
pub fn safe_literal_replace(content: &str, old: &str, new: &str) -> String {
    content.replace(old, new)
}

/// Count exact occurrences of pattern in content
///
/// Uses Rust's matches() which returns non-overlapping matches.
/// Example: "aaa".matches("aa") returns 1, not 2.
pub fn exact_match_count(content: &str, pattern: &str) -> i32 {
    content.matches(pattern).count() as i32
}

/// Unescape LLM over-escaped strings
///
/// LLMs frequently over-escape strings (e.g., \\n instead of \n).
/// This function attempts to fix these issues automatically.
///
/// Supported escape sequences:
/// - \\n → \n (newline)
/// - \\t → \t (tab)
/// - \\r → \r (carriage return)
/// - \\' → ' (single quote)
/// - \\" → " (double quote)
/// - \\` → ` (backtick)
/// - \\\\ → \ (backslash)
///
/// Pattern: `\\+(n|t|r|'|"|`|\\|\n)` matches one or more backslashes followed by
/// a special character.
pub fn unescape_string(s: &str) -> String {
    static UNESCAPE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\\+(n|t|r|'|"|`|\\|\n)"#).expect("Invalid unescape regex"));

    UNESCAPE_RE
        .replace_all(s, |caps: &regex_lite::Captures| {
            let captured_char = &caps[1];
            match captured_char {
                "n" => "\n",
                "t" => "\t",
                "r" => "\r",
                "'" => "'",
                "\"" => "\"",
                "`" => "`",
                "\\" => "\\",
                "\n" => "\n",  // Actual newline preceded by backslash
                _ => &caps[0], // Fallback (should not happen)
            }
            .to_string()
        })
        .to_string()
}

/// Compute SHA256 hash of content for concurrent modification detection
///
/// Used to detect if a file was modified externally between read and write operations.
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Detect line ending style (CRLF vs LF)
///
/// Simple heuristic: if file contains any \r\n, treat as CRLF.
/// Works for 99% of cases.
pub fn detect_line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Restore trailing newline to match original file
///
/// Preserves original file's trailing newline state (important for git diffs,
/// linters, etc.)
pub fn restore_trailing_newline(original: &str, modified: &str) -> String {
    let had_trailing = original.ends_with('\n');
    let has_trailing = modified.ends_with('\n');

    match (had_trailing, has_trailing) {
        (true, false) => format!("{modified}\n"),
        (false, true) => modified.trim_end_matches('\n').to_string(),
        _ => modified.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_literal_replace() {
        assert_eq!(
            safe_literal_replace("hello world", "world", "rust"),
            "hello rust"
        );
        assert_eq!(
            safe_literal_replace("$50 item", "$50", "$100"),
            "$50 item".replace("$50", "$100")
        );
    }

    #[test]
    fn test_exact_match_count() {
        assert_eq!(exact_match_count("hello hello", "hello"), 2);
        assert_eq!(exact_match_count("aaa", "aa"), 1); // Non-overlapping
        assert_eq!(exact_match_count("test", "notfound"), 0);
    }

    #[test]
    fn test_unescape_string_newlines() {
        assert_eq!(unescape_string("hello\\nworld"), "hello\nworld");
        assert_eq!(unescape_string("\\\\n"), "\n"); // 2 backslashes + n → newline
        assert_eq!(unescape_string("\\\\\\\\n"), "\n"); // 4 backslashes + n → newline
    }

    #[test]
    fn test_unescape_string_tabs() {
        assert_eq!(unescape_string("col1\\tcol2"), "col1\tcol2");
        assert_eq!(unescape_string("\\\\t"), "\t");
    }

    #[test]
    fn test_unescape_string_quotes() {
        assert_eq!(unescape_string("it\\'s"), "it's");
        assert_eq!(unescape_string("say \\\"hello\\\""), "say \"hello\"");
        assert_eq!(unescape_string("code: \\`x\\`"), "code: `x`");
    }

    #[test]
    fn test_unescape_string_backslash() {
        assert_eq!(unescape_string("path\\\\file"), "path\\file");
    }

    #[test]
    fn test_unescape_real_newline_not_affected() {
        let input = "hello\nworld"; // Real newline
        assert_eq!(unescape_string(input), input);
    }

    #[test]
    fn test_unescape_mixed() {
        let mixed = "line1\nline2\\nline3"; // Real + escaped newline
        assert_eq!(unescape_string(mixed), "line1\nline2\nline3");
    }

    #[test]
    fn test_hash_content_consistency() {
        let content = "test content";
        let hash1 = hash_content(content);
        let hash2 = hash_content(content);
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA256 hex is 64 chars
    }

    #[test]
    fn test_hash_content_different() {
        let hash1 = hash_content("content1");
        let hash2 = hash_content("content2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_detect_line_ending_lf() {
        assert_eq!(detect_line_ending("line1\nline2\nline3\n"), "\n");
        assert_eq!(detect_line_ending("single line"), "\n");
    }

    #[test]
    fn test_detect_line_ending_crlf() {
        assert_eq!(detect_line_ending("line1\r\nline2\r\n"), "\r\n");
        assert_eq!(detect_line_ending("mixed\r\nhas\ncrlf"), "\r\n"); // Any CRLF → CRLF
    }

    #[test]
    fn test_restore_trailing_newline_add() {
        let original = "has\ntrailing\n";
        let modified = "has\ntrailing"; // Lost trailing newline
        assert_eq!(
            restore_trailing_newline(original, modified),
            "has\ntrailing\n"
        );
    }

    #[test]
    fn test_restore_trailing_newline_remove() {
        let original = "no trailing";
        let modified = "no trailing\n"; // Added trailing newline
        assert_eq!(restore_trailing_newline(original, modified), "no trailing");
    }

    #[test]
    fn test_restore_trailing_newline_unchanged() {
        let original = "has\n";
        let modified = "modified\n";
        assert_eq!(restore_trailing_newline(original, modified), "modified\n");

        let original2 = "no newline";
        let modified2 = "modified";
        assert_eq!(restore_trailing_newline(original2, modified2), "modified");
    }
}
