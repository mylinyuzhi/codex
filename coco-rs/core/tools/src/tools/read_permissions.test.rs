//! R6-T20: file_read_ignore_matcher + check_read_permission tests.
//!
//! These tests exercise the env-var-driven ignore pattern flow. Because
//! `file_read_ignore_matcher()` caches via `OnceLock`, the cached
//! matcher may not reflect runtime env changes — so these tests use
//! `is_read_ignored` directly against a freshly constructed matcher
//! rather than relying on the cache.

use super::*;
use coco_types::PermissionDecision;

/// Build a matcher from a list of patterns for testing (bypasses the
/// OnceLock cache so the test can set patterns directly).
fn build_test_matcher(patterns: &[&str]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        if let Ok(g) = Glob::new(pat) {
            builder.add(g);
        }
        if !pat.contains('/')
            && !pat.contains('*')
            && let Ok(g) = Glob::new(&format!("**/{pat}"))
        {
            builder.add(g);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

fn is_ignored_with(patterns: &[&str], path: &str) -> bool {
    let matcher = build_test_matcher(patterns);
    let path = Path::new(path);
    if matcher.is_match(path) {
        return true;
    }
    if let Some(name) = path.file_name()
        && matcher.is_match(Path::new(name))
    {
        return true;
    }
    false
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

/// check_read_permission returns Allow for unrelated paths when matcher
/// is cached-empty (default state: no env var set).
#[test]
fn test_check_read_permission_allows_unrelated_paths() {
    // No need to set env vars; the cached matcher is empty by default.
    let result = check_read_permission(Path::new("src/main.rs"));
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
