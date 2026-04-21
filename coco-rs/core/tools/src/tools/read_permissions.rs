//! Shared file-read permission checking for Grep / Glob / Read tools.
//!
//! R6-T20. TS routes every file-read tool through
//! `checkReadPermissionForTool()` (filesystem.ts:1030) which consults
//! the session `toolPermissionContext` for deny rules and applies the
//! resulting glob patterns as ripgrep `--glob '!...'` arguments plus
//! hard-fails direct Read calls against blocked paths.
//!
//! coco-rs resolves the deny patterns ahead of time in
//! `coco_config::ToolConfig::file_read_ignore_patterns` (JSON-first,
//! env override via `COCO_FILE_READ_IGNORE_PATTERNS`). Tools build a
//! matcher from `ctx.tool_config.file_read_ignore_patterns` and pass
//! it into the helpers below. There is intentionally **no** global
//! env-only matcher — keeping a single source of truth prevents
//! JSON-configured patterns from silently disagreeing with a cached
//! env-only snapshot.
//!
//! Wiring:
//!
//! * `ReadTool::check_permissions` rejects file_path directly if it
//!   matches any pattern.
//! * `GrepTool::check_permissions` rejects the `path` argument if it
//!   matches; the walker also skips individual files that match during
//!   traversal via `is_read_ignored_with_matcher`.
//! * `GlobTool::check_permissions` mirrors Grep.
//!
//! Not a security boundary. TS explicitly notes the same thing in
//! `shouldUseSandbox.ts` — the ignore patterns are a convenience to
//! hide sensitive files from the model, not a guarantee.

use coco_types::PermissionDecision;
use coco_types::PermissionDecisionReason;
use globset::Glob;
use globset::GlobSet;
use globset::GlobSetBuilder;
use std::path::Path;

/// Compile a `GlobSet` matcher from a list of patterns.
///
/// Unqualified literal patterns (e.g. `.env`) are automatically expanded
/// with an any-ancestor variant (`**/.env`) so a pattern matches every
/// `.env` in the tree, not just one at the root.
pub fn file_read_ignore_matcher_from_patterns(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns.iter().map(String::as_str) {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        } else {
            tracing::warn!(
                pattern = %pattern,
                "file_read_ignore_patterns contains an invalid glob; skipping"
            );
        }
        // Also add an any-ancestor version so `.env` matches
        // `foo/.env`, `a/b/.env`, etc.
        if !pattern.contains('/')
            && !pattern.contains('*')
            && let Ok(glob) = Glob::new(&format!("**/{pattern}"))
        {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

/// Test a path against a caller-supplied matcher.
///
/// Returns `true` if the path matches any ignore pattern and should be
/// blocked. Accepts both absolute and relative paths. Matches against
/// the raw path, the file name, and the path with leading `./` stripped
/// so a pattern like `".env"` catches `/abs/path/.env`, `.env`, and
/// `./.env`.
pub fn is_read_ignored_with_matcher(path: &Path, matcher: &GlobSet) -> bool {
    if matcher.is_empty() {
        return false;
    }
    if matcher.is_match(path) {
        return true;
    }
    if let Some(file_name) = path.file_name()
        && matcher.is_match(Path::new(file_name))
    {
        return true;
    }
    // Try path without leading `./` for relative paths.
    if let Some(stripped) = path.to_str().and_then(|s| s.strip_prefix("./"))
        && matcher.is_match(Path::new(stripped))
    {
        return true;
    }
    false
}

/// Helper: build a `PermissionDecision::Deny` when the target path is
/// in the ignore list, else `Allow`.
///
/// R6-T20. Used by `Tool::check_permissions` overrides in Read/Grep/Glob.
pub fn check_read_permission_with_matcher(path: &Path, matcher: &GlobSet) -> PermissionDecision {
    if is_read_ignored_with_matcher(path, matcher) {
        PermissionDecision::Deny {
            message: format!(
                "Path `{}` is blocked by file-read ignore patterns. \
                 This is a session-level filter intended to keep \
                 sensitive files out of the model's view; adjust \
                 `tool.file_read_ignore_patterns` in settings (or \
                 `COCO_FILE_READ_IGNORE_PATTERNS`) if you need access.",
                path.display()
            ),
            reason: PermissionDecisionReason::Classifier {
                classifier: "file_read_ignore".into(),
                reason: "path matches file_read_ignore_patterns".into(),
            },
        }
    } else {
        PermissionDecision::Allow {
            updated_input: None,
            feedback: None,
        }
    }
}

#[cfg(test)]
#[path = "read_permissions.test.rs"]
mod tests;
