//! Sensitive file detection for permission checks.
//!
//! Identifies files that require elevated permission due to containing
//! credentials, secrets, or critical configuration.

use std::path::Path;

/// Sensitive file path patterns (matching Claude Code v2.1.76).
const SENSITIVE_FILE_PATTERNS: &[&str] = &[
    // Credentials and keys
    ".env",
    "*.pem",
    "*.key",
    "credentials.json",
    // Keystore formats
    "*.keystore",
    "*.jks",
    "*.p12",
    "*.pfx",
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
    ".ssh/known_hosts",
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

    // SSH key name suffixes (e.g., mykey_rsa, deploy_ed25519)
    let fname = filename.as_ref();
    if fname.ends_with("_rsa")
        || fname.ends_with("_dsa")
        || fname.ends_with("_ecdsa")
        || fname.ends_with("_ed25519")
    {
        return true;
    }

    // known_hosts outside .ssh/ (the .ssh/known_hosts pattern catches the common case)
    if fname == "known_hosts" {
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

/// Check if a directory path requires elevated permission for read-only access.
///
/// Used by tools that operate on directories (glob, grep, ls) to enforce:
/// - Sensitive directory targets → NeedsApproval
/// - Outside working directory → NeedsApproval
/// - Otherwise → Allowed
pub fn check_directory_permission(
    tool_name: &str,
    path: &Path,
    cwd: &Path,
) -> cocode_protocol::PermissionResult {
    use cocode_protocol::ApprovalRequest;
    use cocode_protocol::PermissionResult;

    if is_sensitive_directory(path) {
        return PermissionResult::NeedsApproval {
            request: ApprovalRequest {
                request_id: format!("{tool_name}-sensitive-{}", path.display()),
                tool_name: tool_name.to_string(),
                description: format!("Accessing sensitive directory: {}", path.display()),
                risks: vec![],
                allow_remember: true,
                proposed_prefix_pattern: None,
            },
        };
    }

    if is_outside_cwd(path, cwd) {
        return PermissionResult::NeedsApproval {
            request: ApprovalRequest {
                request_id: format!("{tool_name}-outside-cwd-{}", path.display()),
                tool_name: tool_name.to_string(),
                description: format!("Accessing outside working directory: {}", path.display()),
                risks: vec![],
                allow_remember: true,
                proposed_prefix_pattern: None,
            },
        };
    }

    PermissionResult::Allowed
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
