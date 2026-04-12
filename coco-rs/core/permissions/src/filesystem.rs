//! Filesystem permission checking.
//!
//! TS: utils/permissions/filesystem.ts (1.8K LOC)
//!     utils/permissions/pathValidation.ts (16K LOC)
//!
//! Validates file paths against allowed directories, detects path traversal,
//! dangerous files/directories, and suspicious Windows patterns.

use std::path::Path;
use std::path::PathBuf;

// ── Dangerous paths ──

/// Files that should never be auto-edited (dotfiles, config).
///
/// TS: DANGEROUS_FILES in filesystem.ts
const DANGEROUS_FILES: &[&str] = &[
    ".gitconfig",
    ".gitmodules",
    ".bashrc",
    ".bash_profile",
    ".zshrc",
    ".zprofile",
    ".profile",
    ".ripgreprc",
    ".mcp.json",
    ".claude.json",
];

/// Directories whose contents should not be auto-edited.
///
/// TS: DANGEROUS_DIRECTORIES in filesystem.ts
const DANGEROUS_DIRECTORIES: &[&str] = &[".git", ".vscode", ".idea", ".claude"];

/// System directories blocked for all writes.
const BLOCKED_SYSTEM_DIRS: &[&str] = &[
    "/etc", "/usr", "/bin", "/sbin", "/lib", "/lib64", "/boot", "/dev", "/proc", "/sys", "/var/run",
];

// ── Path traversal detection ──

/// Check if a path contains `..` traversal components.
///
/// TS: `containsPathTraversal()` in path.ts
/// Regex: `(?:^|[\\/])\.\.(?:[\\/]|$)`
pub fn contains_path_traversal(path: &str) -> bool {
    for component in path.split(&['/', '\\']) {
        if component == ".." {
            return true;
        }
    }
    false
}

// ── Dangerous file/directory detection ──

/// Check if a path targets a dangerous file (dotfiles, config files).
///
/// TS: `isDangerousFilePathToAutoEdit()` in filesystem.ts
pub fn is_dangerous_file_path(path: &str) -> bool {
    let lower = path.to_lowercase();

    // Check filename against dangerous files
    if let Some(filename) = Path::new(&lower).file_name().and_then(|n| n.to_str()) {
        for &dangerous in DANGEROUS_FILES {
            if filename == dangerous {
                return true;
            }
        }
    }

    // Check if any path component is a dangerous directory
    for component in lower.split(&['/', '\\']) {
        if component.is_empty() {
            continue;
        }
        for &dangerous_dir in DANGEROUS_DIRECTORIES {
            if component == dangerous_dir {
                // Special case: .claude/worktrees/ is allowed (structural path)
                if dangerous_dir == ".claude" && lower.contains(".claude/worktrees/") {
                    continue;
                }
                return true;
            }
        }
    }

    // Block UNC paths
    if lower.starts_with("\\\\") || lower.starts_with("//") {
        return true;
    }

    false
}

/// Check if a path is a Claude settings or config file.
///
/// TS: `isClaudeConfigFilePath()` in filesystem.ts
pub fn is_claude_config_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_lowercase();
    normalized.contains("/.claude/settings.json")
        || normalized.contains("/.claude/settings.local.json")
        || normalized.contains("/.claude/commands/")
        || normalized.contains("/.claude/agents/")
        || normalized.contains("/.claude/skills/")
}

// ── Suspicious Windows path patterns ──

/// Check for suspicious Windows path patterns that could bypass security.
///
/// TS: `hasSuspiciousWindowsPathPattern()` in filesystem.ts
pub fn has_suspicious_windows_pattern(path: &str) -> bool {
    // NTFS Alternate Data Streams: "file.txt::$DATA", "settings.json:stream"
    // Skip drive letter at position 1 (e.g., C:)
    if path.len() > 2 && path[2..].contains(':') {
        return true;
    }

    // 8.3 short name pattern: "GIT~1", "CLAUDE~1"
    if path.contains("~1")
        || path.contains("~2")
        || path.contains("~3")
        || path.contains("~4")
        || path.contains("~5")
        || path.contains("~6")
        || path.contains("~7")
        || path.contains("~8")
        || path.contains("~9")
    {
        return true;
    }

    // Long path prefixes: \\?\, \\.\, //?/, //./
    if path.starts_with("\\\\?\\")
        || path.starts_with("\\\\.\\")
        || path.starts_with("//?/")
        || path.starts_with("//./")
    {
        return true;
    }

    // Trailing dots or spaces
    if path.ends_with('.') || path.ends_with(' ') {
        return true;
    }

    // DOS device names as extensions: .CON, .PRN, .AUX, .NUL, .COM1-9, .LPT1-9
    let upper = path.to_uppercase();
    for suffix in &[
        ".CON", ".PRN", ".AUX", ".NUL", ".COM1", ".COM2", ".COM3", ".COM4", ".COM5", ".COM6",
        ".COM7", ".COM8", ".COM9", ".LPT1", ".LPT2", ".LPT3", ".LPT4", ".LPT5", ".LPT6", ".LPT7",
        ".LPT8", ".LPT9",
    ] {
        if upper.ends_with(suffix) {
            return true;
        }
    }

    // Triple+ dots as path component: .../file.txt
    for component in path.split(&['/', '\\']) {
        if component.len() >= 3 && component.chars().all(|c| c == '.') {
            return true;
        }
    }

    false
}

// ── Path safety validation ──

/// Result of a path safety check.
#[derive(Debug, Clone)]
pub enum PathSafetyResult {
    Safe,
    Blocked {
        message: String,
        /// Whether an LLM classifier can still approve this.
        classifier_approvable: bool,
    },
}

/// Check path safety for auto-edit mode.
///
/// TS: `checkPathSafetyForAutoEdit()` in filesystem.ts
/// Validation order:
/// 1. Suspicious Windows patterns
/// 2. Claude config files
/// 3. Dangerous files/directories
/// 4. Path traversal
/// TS: `checkPathSafetyForAutoEdit()` in filesystem.ts / pathValidation.ts
/// Validation order:
/// 1. Suspicious Windows patterns (NTFS ADS, 8.3, long-path, etc.)
/// 2. Shell expansion patterns ($VAR, `cmd`, %VAR%)
/// 3. Dangerous tilde variants (~user, ~+, ~-)
/// 4. Path traversal (..)
/// 5. Claude config files
/// 6. Dangerous files/directories
pub fn check_path_safety_for_auto_edit(path: &str) -> PathSafetyResult {
    // 1. Suspicious Windows patterns
    if has_suspicious_windows_pattern(path) {
        return PathSafetyResult::Blocked {
            message: format!("suspicious path pattern detected: {path}"),
            classifier_approvable: false,
        };
    }

    // 2. Shell expansion — TOCTOU: path is literal to us but shell may expand
    if has_shell_expansion(path) {
        return PathSafetyResult::Blocked {
            message: format!("path contains shell expansion patterns: {path}"),
            classifier_approvable: false,
        };
    }

    // 3. Dangerous tilde variants
    if has_dangerous_tilde(path) {
        return PathSafetyResult::Blocked {
            message: format!("unsafe tilde expansion in path: {path}"),
            classifier_approvable: false,
        };
    }

    // 4. Path traversal
    if contains_path_traversal(path) {
        return PathSafetyResult::Blocked {
            message: format!("path contains traversal (..): {path}"),
            classifier_approvable: false,
        };
    }

    // 5. Claude config files
    if is_claude_config_path(path) {
        return PathSafetyResult::Blocked {
            message: format!("editing Claude config files requires approval: {path}"),
            classifier_approvable: true,
        };
    }

    // 6. Dangerous files/directories
    if is_dangerous_file_path(path) {
        return PathSafetyResult::Blocked {
            message: format!("dangerous file path requires approval: {path}"),
            classifier_approvable: true,
        };
    }

    PathSafetyResult::Safe
}

// ── Working directory containment ──

/// Check if a path is within the working directory (or allowed directories).
///
/// TS: `pathInWorkingPath()` in filesystem.ts
/// Handles macOS /private/ symlinks and case-insensitive comparison.
pub fn path_in_working_path(path: &str, working_path: &str) -> bool {
    let abs_path = normalize_for_comparison(&resolve_path(path, working_path));
    let abs_working = normalize_for_comparison(&resolve_path(working_path, working_path));

    // Exact match
    if abs_path == abs_working {
        return true;
    }

    // Path must start with working_path + separator
    let with_sep = if abs_working.ends_with('/') {
        abs_working.clone()
    } else {
        format!("{abs_working}/")
    };

    abs_path.starts_with(&with_sep)
}

/// Check if a path is within the allowed working directories.
///
/// TS: isPathWithinAllowedDirs()
pub fn is_path_within_allowed_dirs(path: &str, cwd: &str, additional_dirs: &[String]) -> bool {
    let resolved = resolve_path(path, cwd);
    let normalized = normalize_for_comparison(&resolved);

    // Always allow /tmp
    if normalized.starts_with("/tmp") {
        return true;
    }

    // Check cwd
    if path_in_working_path(path, cwd) {
        return true;
    }

    // Check additional directories
    for dir in additional_dirs {
        if path_in_working_path(path, dir) {
            return true;
        }
    }

    false
}

/// Check if a path is a scratchpad directory (temporary workspace).
pub fn is_scratchpad_dir(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("/tmp/")
        || lower.contains("/scratch/")
        || lower.contains("/scratchpad/")
        || lower.contains("/.cache/")
}

/// Normalize and resolve a path for permission checking.
fn resolve_path(path: &str, cwd: &str) -> String {
    let expanded = expand_tilde(path);
    if expanded.starts_with('/') {
        expanded
    } else {
        format!("{cwd}/{expanded}")
    }
}

/// Normalize a path for comparison: lowercase, handle macOS /private/ symlinks.
fn normalize_for_comparison(path: &str) -> String {
    let mut p = path.to_lowercase();
    // macOS symlink normalization
    if p.starts_with("/private/var/") {
        p = p.replacen("/private/var/", "/var/", 1);
    }
    if p.starts_with("/private/tmp/") {
        p = p.replacen("/private/tmp/", "/tmp/", 1);
    } else if p.starts_with("/private/tmp") {
        p = p.replacen("/private/tmp", "/tmp", 1);
    }
    p
}

/// Expand `~` to `$HOME`. Rejects dangerous tilde variants.
///
/// TS: `pathValidation.ts` blocks `~user`, `~+`, `~-`, `=cmd` to prevent
/// TOCTOU and shell expansion attacks.
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        path.replacen('~', &home, 1)
    } else {
        path.to_string()
    }
}

/// Check for dangerous tilde variants that could expand unexpectedly.
///
/// TS: `pathValidation.ts` — blocks `~user`, `~+`, `~-` to prevent
/// TOCTOU attacks where shell expands these at runtime.
pub fn has_dangerous_tilde(path: &str) -> bool {
    if !path.starts_with('~') {
        return false;
    }
    // Safe: "~" alone or "~/..."
    if path == "~" || path.starts_with("~/") {
        return false;
    }
    // Everything else is suspicious: ~user, ~+, ~-, ~1, etc.
    true
}

/// Check for shell variable expansion patterns that bypass path validation.
///
/// TS: `pathValidation.ts` — blocks `$VAR`, `${VAR}`, `$(cmd)`, backticks,
/// and Zsh `=cmd` expansion.
/// These are literals to the filesystem but shells expand them, creating a
/// TOCTOU gap between validation and execution.
pub fn has_shell_expansion(path: &str) -> bool {
    // $VAR or ${VAR}
    if path.contains('$') {
        return true;
    }
    // Backtick command substitution
    if path.contains('`') {
        return true;
    }
    // Zsh equals expansion: =cmd expands to path of `cmd` binary
    // TS: pathValidation.ts:426
    if path.starts_with('=') && path.len() > 1 && path.as_bytes()[1].is_ascii_alphabetic() {
        return true;
    }
    // Windows %VAR%
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(end) = bytes[i + 1..].iter().position(|&b| b == b'%') {
                if end > 0 {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Get the default allowed directories for a project.
pub fn get_default_allowed_dirs(cwd: &Path) -> Vec<PathBuf> {
    vec![cwd.to_path_buf(), PathBuf::from("/tmp")]
}

/// Validate that a file path is safe for write operations.
///
/// Returns None if safe, Some(reason) if blocked.
pub fn validate_write_path(path: &str, cwd: &str, additional_dirs: &[String]) -> Option<String> {
    let resolved = resolve_path(path, cwd);

    // Path traversal check
    if contains_path_traversal(path) {
        return Some(format!("path traversal detected: {path}"));
    }

    // Block writes to system directories
    for &blocked in BLOCKED_SYSTEM_DIRS {
        if resolved.starts_with(blocked) {
            return Some(format!("write to system directory blocked: {blocked}"));
        }
    }

    // Check if within allowed directories
    if !is_path_within_allowed_dirs(path, cwd, additional_dirs) {
        return Some(format!("path outside allowed directories: {path}"));
    }

    None
}

// ── Symlink chain resolution ──

/// Maximum symlink chain depth (matches POSIX SYMLOOP_MAX).
const MAX_SYMLINK_DEPTH: usize = 40;

/// Collect all paths that must be checked for a single input path.
///
/// TS: `getPathsForPermissionCheck()` in fsOperations.ts
///
/// Follows symlink chains (up to 40 hops), collecting every intermediate
/// target. This prevents symlink bypass attacks where `./link → /etc/passwd`
/// would pass a CWD-relative check.
///
/// Returns: Vec of absolute paths to check (original + all intermediate + resolved).
pub fn get_paths_for_permission_check(path: &str, cwd: &str) -> Vec<String> {
    let abs_path = resolve_path(path, cwd);
    let mut paths = vec![abs_path.clone()];

    // UNC paths: block before any filesystem access
    if abs_path.starts_with("//") || abs_path.starts_with("\\\\") {
        return paths;
    }

    let mut current = PathBuf::from(&abs_path);
    let mut visited = std::collections::HashSet::new();

    for _ in 0..MAX_SYMLINK_DEPTH {
        // Check if path exists and is a symlink
        let meta = match std::fs::symlink_metadata(&current) {
            Ok(m) => m,
            Err(_) => {
                // Non-existent: resolve deepest existing ancestor
                if let Some(resolved) = resolve_deepest_existing_ancestor(&current) {
                    let resolved_str = resolved.to_string_lossy().to_string();
                    if !paths.contains(&resolved_str) {
                        paths.push(resolved_str);
                    }
                }
                break;
            }
        };

        if !meta.file_type().is_symlink() {
            break;
        }

        // Circular symlink detection
        if !visited.insert(current.clone()) {
            break;
        }

        // Read the symlink target
        let target = match std::fs::read_link(&current) {
            Ok(t) => t,
            Err(_) => break,
        };

        // Resolve relative targets against symlink's parent directory
        let resolved_target = if target.is_absolute() {
            target
        } else if let Some(parent) = current.parent() {
            parent.join(&target)
        } else {
            target
        };

        let target_str = resolved_target.to_string_lossy().to_string();
        if !paths.contains(&target_str) {
            paths.push(target_str);
        }

        current = resolved_target;
    }

    // Final canonical resolution via realpath
    if let Ok(canonical) = std::fs::canonicalize(&abs_path) {
        let canonical_str = canonical.to_string_lossy().to_string();
        if !paths.contains(&canonical_str) {
            paths.push(canonical_str);
        }
    }

    paths
}

/// For a non-existent path, resolve the deepest existing ancestor to
/// find where the file would actually be created after symlink resolution.
///
/// TS: `resolveDeepestExistingAncestorSync()` in fsOperations.ts
fn resolve_deepest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    let mut tail_segments: Vec<std::ffi::OsString> = Vec::new();

    // Walk up until we find an existing path
    loop {
        match std::fs::symlink_metadata(&current) {
            Ok(meta) => {
                // Found existing path — resolve it
                if meta.file_type().is_symlink() {
                    if let Ok(resolved) = std::fs::canonicalize(&current) {
                        // Rejoin tail segments
                        let mut result = resolved;
                        for seg in tail_segments.into_iter().rev() {
                            result.push(seg);
                        }
                        return Some(result);
                    }
                }
                // Existing non-symlink: resolve via canonicalize
                if let Ok(resolved) = std::fs::canonicalize(&current) {
                    if resolved != current {
                        let mut result = resolved;
                        for seg in tail_segments.into_iter().rev() {
                            result.push(seg);
                        }
                        return Some(result);
                    }
                }
                return None; // No symlinks in ancestry
            }
            Err(_) => {
                // Not found — save segment and move up
                if let Some(name) = current.file_name() {
                    tail_segments.push(name.to_owned());
                }
                if !current.pop() {
                    return None; // Reached root
                }
            }
        }
    }
}

// ── Internal path exemptions ──

/// Paths within the project memory directory that are auto-writable.
///
/// TS: `checkEditableInternalPath()` in filesystem.ts
/// Exemptions: plan files, scratchpad, agent memory.
pub fn is_editable_internal_path(path: &str, cwd: &str, session_id: Option<&str>) -> bool {
    let normalized = normalize_for_comparison(&resolve_path(path, cwd));
    let cwd_lower = cwd.to_lowercase();

    // Plan files: {cwd}/.claude/plans/*.md
    if normalized.contains("/.claude/plans/") && normalized.ends_with(".md") {
        return true;
    }

    // Scratchpad: /tmp/claude-*/{cwd}/{session}/scratchpad/
    if let Some(sid) = session_id {
        let scratchpad_pattern = format!("/tmp/claude-");
        if normalized.starts_with(&scratchpad_pattern) && normalized.contains("/scratchpad/") {
            return true;
        }
    }

    // Agent memory: ~/.coco/projects/{cwd}/memory/
    let config_home = coco_config::global_config::config_home()
        .to_string_lossy()
        .to_lowercase();
    if normalized.starts_with(&format!("{config_home}/projects/"))
        && normalized.contains("/memory/")
    {
        return true;
    }

    // CLAUDE.md files (project instructions)
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if filename == "CLAUDE.md" || filename == "CLAUDE.local.md" {
        return is_path_within_allowed_dirs(path, cwd, &[]);
    }

    false
}

/// Paths within internal directories that are auto-readable.
///
/// TS: `checkReadableInternalPath()` in filesystem.ts
/// Exemptions: session memory, project dir, plan files, tool results,
/// scratchpad, project temp, agent memory.
pub fn is_readable_internal_path(path: &str, cwd: &str) -> bool {
    let normalized = normalize_for_comparison(&resolve_path(path, cwd));
    let config_home = coco_config::global_config::config_home()
        .to_string_lossy()
        .to_lowercase();

    // Project dir: ~/.coco/projects/{sanitized-cwd}/
    if normalized.starts_with(&format!("{config_home}/projects/")) {
        return true;
    }

    // Plan files (readable in all modes)
    if normalized.contains("/.claude/plans/") && normalized.ends_with(".md") {
        return true;
    }

    // Scratchpad and project temp
    if normalized.starts_with("/tmp/claude-") {
        return true;
    }

    // Agent memory
    if normalized.starts_with(&format!("{config_home}/projects/"))
        && normalized.contains("/memory/")
    {
        return true;
    }

    false
}

// ── Dangerous removal detection ──

/// Check if a path is dangerously broad for removal operations (rm, rmdir).
///
/// TS: `isDangerousRemovalPath()` in pathValidation.ts
/// Blocks: /, ~, /*, wildcards, drive roots, root children.
pub fn is_dangerous_removal_path(path: &str) -> bool {
    let trimmed = path.trim();

    // Wildcard patterns: *, /*, \*
    if trimmed == "*" || trimmed.ends_with("/*") || trimmed.ends_with("\\*") {
        return true;
    }

    // Root directory
    if trimmed == "/" || trimmed == "\\" {
        return true;
    }

    // Home directory
    if trimmed == "~" || trimmed == "~/" {
        return true;
    }

    // Expand and check
    let expanded = expand_tilde(trimmed);
    let normalized = expanded.replace('\\', "/");

    // Direct children of root: /usr, /etc, /tmp, /var, /home, etc.
    if normalized.starts_with('/') && !normalized[1..].contains('/') && normalized.len() > 1 {
        return true;
    }

    // Windows drive roots: C:\, D:\
    if normalized.len() >= 2
        && normalized.as_bytes()[0].is_ascii_alphabetic()
        && normalized.as_bytes()[1] == b':'
    {
        // C: or C:\ or C:/
        if normalized.len() <= 3 {
            return true;
        }
        // Direct children: C:\Windows, C:\Users
        let after_drive = &normalized[3..];
        if !after_drive.contains('/') {
            return true;
        }
    }

    false
}

#[cfg(test)]
#[path = "filesystem.test.rs"]
mod tests;
