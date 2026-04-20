//! R6-T20: file_read_ignore_matcher_from_patterns + permission helpers.
//!
//! Tests construct a matcher directly from pattern strings so they
//! don't touch process env.

use super::*;
use coco_types::PermissionDecision;

/// Build a matcher from a list of patterns for testing.
fn build_test_matcher(patterns: &[&str]) -> GlobSet {
    let owned: Vec<String> = patterns.iter().map(|s| (*s).to_string()).collect();
    file_read_ignore_matcher_from_patterns(&owned)
}

fn is_ignored_with(patterns: &[&str], path: &str) -> bool {
    let matcher = build_test_matcher(patterns);
    is_read_ignored_with_matcher(Path::new(path), &matcher)
}

/// `.env` pattern catches `.env`, `foo/.env`, `/abs/path/.env`.
#[test]
fn test_dotenv_pattern_catches_all_locations() {
    let patterns = &[".env"];
    assert!(is_ignored_with(patterns, ".env"));
    assert!(is_ignored_with(patterns, "foo/.env"));
    assert!(is_ignored_with(patterns, "/abs/path/.env"));
    assert!(is_ignored_with(patterns, "a/b/c/.env"));
}

/// Glob pattern with wildcard: `*.key` matches any `.key` file.
#[test]
fn test_wildcard_pattern() {
    let patterns = &["*.key"];
    assert!(is_ignored_with(patterns, "private.key"));
    assert!(is_ignored_with(patterns, "ssh_host_rsa.key"));
}

/// Directory pattern: `secrets/*` matches files inside `secrets/`.
#[test]
fn test_directory_pattern() {
    let patterns = &["secrets/*"];
    assert!(is_ignored_with(patterns, "secrets/token"));
    assert!(is_ignored_with(patterns, "secrets/prod.json"));
    // Does NOT match files that happen to have `secrets` in the middle.
    assert!(!is_ignored_with(patterns, "my_secrets.txt"));
}

/// Empty patterns list → nothing is blocked.
#[test]
fn test_empty_patterns_allow_everything() {
    let patterns: &[&str] = &[];
    assert!(!is_ignored_with(patterns, ".env"));
    assert!(!is_ignored_with(patterns, "secrets/token"));
    assert!(!is_ignored_with(patterns, "private.key"));
}

/// `check_read_permission_with_matcher` returns Allow for unrelated
/// paths when the matcher holds no patterns.
#[test]
fn test_check_read_permission_allows_unrelated_paths() {
    let matcher = build_test_matcher(&[]);
    let result = check_read_permission_with_matcher(Path::new("src/main.rs"), &matcher);
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

/// Unicode + path with special chars: glob matching should still work
/// against the path bytes.
#[test]
fn test_non_ascii_path() {
    let patterns = &["*.secret"];
    assert!(is_ignored_with(patterns, "配置.secret"));
    assert!(is_ignored_with(patterns, "мой.secret"));
}
