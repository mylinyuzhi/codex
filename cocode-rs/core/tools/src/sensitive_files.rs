//! Sensitive file detection for permission checks.
//!
//! Identifies files that require elevated permission due to containing
//! credentials, secrets, or critical configuration.

use std::path::Path;

/// Sensitive file path patterns (matching Claude Code v2.1.7).
const SENSITIVE_FILE_PATTERNS: &[&str] = &[
    // Credentials and keys
    ".env",
    "*.pem",
    "*.key",
    "credentials.json",
    // Shell configuration
    ".bashrc",
    ".zshrc",
    ".bash_profile",
    ".zprofile",
    ".profile",
    // Git configuration
    ".gitconfig",
    ".git-credentials",
    ".gitmodules",
    // SSH
    ".ssh/config",
    ".ssh/authorized_keys",
    // Tool configuration
    ".mcp.json",
    ".claude/settings.json",
    ".npmrc",
    ".pypirc",
    ".ripgreprc",
    // CI/CD
    ".github/workflows/*.yml",
];

/// Locked directories that should not be written to.
const LOCKED_DIRECTORIES: &[&str] = &[
    ".claude/",
    ".claude/commands/",
    ".claude/agents/",
    ".claude/skills/",
];

/// Sensitive directories that require approval for writes.
const SENSITIVE_DIRECTORIES: &[&str] = &[".git/", ".vscode/", ".idea/"];

/// Check if a file path matches any sensitive file pattern.
pub fn is_sensitive_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy())
        .unwrap_or_default();

    for pattern in SENSITIVE_FILE_PATTERNS {
        if matches_pattern(pattern, &path_str, &filename) {
            return true;
        }
    }

    // Also check .env.* variants
    if filename.starts_with(".env.") {
        return true;
    }

    // Check service-account*.json
    if filename.starts_with("service-account") && filename.ends_with(".json") {
        return true;
    }

    // Check .ssh/id_*
    if path_str.contains(".ssh/id_") {
        return true;
    }

    false
}

/// Check if a path is within a locked directory.
pub fn is_locked_directory(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    for dir in LOCKED_DIRECTORIES {
        if path_str.contains(dir) {
            return true;
        }
    }
    false
}

/// Check if a path is within a sensitive directory (requires approval for writes).
pub fn is_sensitive_directory(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    for dir in SENSITIVE_DIRECTORIES {
        if path_str.contains(dir) {
            return true;
        }
    }
    false
}

/// Check if a path is outside the given working directory.
pub fn is_outside_cwd(path: &Path, cwd: &Path) -> bool {
    // Canonicalize if possible; fall back to starts_with
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let abs_cwd = cwd.to_path_buf();

    !abs_path.starts_with(&abs_cwd)
}

/// Simple pattern matching for sensitive file detection.
fn matches_pattern(pattern: &str, full_path: &str, filename: &str) -> bool {
    if pattern.contains('/') {
        // Path-based pattern - check if path contains the pattern segment
        if pattern.contains('*') {
            // e.g. ".github/workflows/*.yml"
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                return full_path.contains(parts[0]) && full_path.ends_with(parts[1]);
            }
        }
        // e.g. ".ssh/config" or ".ssh/authorized_keys"
        return full_path.ends_with(pattern) || full_path.contains(&format!("/{pattern}"));
    }

    if let Some(suffix) = pattern.strip_prefix('*') {
        // Extension pattern: "*.pem", "*.key"
        return filename.ends_with(suffix);
    }

    // Exact filename match
    filename == pattern
}

#[cfg(test)]
#[path = "sensitive_files.test.rs"]
mod tests;
