//! Shared file-read permission checking for Grep / Glob / Read tools.
//!
//! R6-T20. TS routes every file-read tool through
//! `checkReadPermissionForTool()` (filesystem.ts:1030) which consults
//! the session `toolPermissionContext` for deny rules and applies the
//! resulting glob patterns as ripgrep `--glob '!...'` arguments plus
//! hard-fails direct Read calls against blocked paths.
//!
//! coco-rs doesn't yet have a persistent permission context, so this
//! module reads the patterns from a single environment variable:
//!
//! ```text
//! COCO_FILE_READ_IGNORE_PATTERNS=".env:secrets/*:*.key"
//! ```
//!
//! The value is a colon-separated list of glob patterns (per
//! `globset::Glob::new`). Matches against any path component are
//! blocked. When unset, no patterns are applied — matches current
//! coco-rs behavior.
//!
//! Wiring:
//!
//! * `ReadTool::check_permissions` rejects file_path directly if it
//!   matches any pattern.
//! * `GrepTool::check_permissions` rejects the `path` argument if it
//!   matches; the walker also skips individual files that match during
//!   traversal (via `file_read_ignore_matcher`).
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
use std::sync::OnceLock;

/// Cached compiled matcher. Built once from the env var at first use;
/// unset env var → empty matcher (matches nothing).
static READ_IGNORE_MATCHER: OnceLock<GlobSet> = OnceLock::new();

/// Return the compiled matcher. The first call parses
/// `COCO_FILE_READ_IGNORE_PATTERNS` and caches the result.
///
/// R6-T20.
pub fn file_read_ignore_matcher() -> &'static GlobSet {
    READ_IGNORE_MATCHER.get_or_init(|| {
        let raw = std::env::var("COCO_FILE_READ_IGNORE_PATTERNS").unwrap_or_default();
        let mut builder = GlobSetBuilder::new();
        for pattern in raw.split(':').map(str::trim).filter(|s| !s.is_empty()) {
            if let Ok(glob) = Glob::new(pattern) {
                builder.add(glob);
            } else {
                tracing::warn!(
                    pattern = %pattern,
                    "COCO_FILE_READ_IGNORE_PATTERNS contains an invalid glob; skipping"
                );
            }
            // Also add an any-ancestor version so `.env` matches
            // `foo/.env`, `a/b/.env`, etc. Users expect `.env` to
            // exclude every `.env` in the tree, not just one at the
            // root. Add `**/.env` alongside the literal pattern.
            if !pattern.contains('/')
                && !pattern.contains('*')
                && let Ok(glob) = Glob::new(&format!("**/{pattern}"))
            {
                builder.add(glob);
            }
        }
        builder.build().unwrap_or_else(|_| GlobSet::empty())
    })
}

/// Test a path against the configured ignore matcher.
///
/// Returns `true` if the path matches any ignore pattern and should be
/// blocked. Accepts both absolute and relative paths. Matches against
/// the raw path, the file name, and the path with leading `./` stripped
/// so a pattern like `".env"` catches `/abs/path/.env`, `.env`, and
/// `./.env`.
pub fn is_read_ignored(path: &Path) -> bool {
    let matcher = file_read_ignore_matcher();
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
pub fn check_read_permission(path: &Path) -> PermissionDecision {
    if is_read_ignored(path) {
        PermissionDecision::Deny {
            message: format!(
                "Path `{}` is blocked by file-read ignore patterns \
                 (COCO_FILE_READ_IGNORE_PATTERNS). This is a session-level \
                 filter intended to keep sensitive files out of the \
                 model's view; adjust the env var if you need access.",
                path.display()
            ),
            reason: PermissionDecisionReason::Classifier {
                classifier: "file_read_ignore".into(),
                reason: "path matches COCO_FILE_READ_IGNORE_PATTERNS".into(),
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
