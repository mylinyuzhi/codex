//! Path whitelisting for auto memory.
//!
//! Determines whether a file path is within the auto memory directory,
//! used to bypass Write/Edit permission prompts for memory files.
//! Includes symlink resolution for team memory security.

use std::path::Path;
use std::path::PathBuf;

/// Check if a path is within the auto memory directory.
///
/// This enables the permission pipeline to auto-allow writes to memory
/// files without user approval.
///
/// Unlike `plan-mode::is_safe_file()` which checks exact file match,
/// this uses `starts_with()` to allow writes to any file under the
/// memory directory (including subdirectories like `team/`).
///
/// Uses canonical path comparison where possible, with fallback to
/// direct `starts_with` comparison.
pub fn is_auto_memory_path(path: &Path, memory_dir: &Path) -> bool {
    let path_str = path.to_string_lossy();

    // Security: reject paths with null bytes
    if path_str.contains('\0') {
        return false;
    }

    // Security: reject UNC paths (Windows)
    if path_str.starts_with("\\\\") {
        return false;
    }

    // Security: reject relative paths (must be absolute)
    if path.is_relative() {
        return false;
    }

    // Security: reject path traversal components
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return false;
    }

    // Try canonical comparison first
    if let (Ok(canonical_path), Ok(canonical_dir)) =
        (path.canonicalize(), memory_dir.canonicalize())
    {
        return canonical_path.starts_with(&canonical_dir);
    }

    // Fallback: direct comparison
    path.starts_with(memory_dir)
}

/// Check if a path is within the team memory directory.
pub fn is_team_memory_path(path: &Path, memory_dir: &Path) -> bool {
    let team_dir = memory_dir.join(crate::directory::TEAM_MEMORY_SUBDIR);
    is_auto_memory_path(path, &team_dir)
}

/// Path traversal error for team memory security.
#[derive(Debug)]
pub struct PathTraversalError {
    pub message: String,
}

impl std::fmt::Display for PathTraversalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PathTraversalError {}

/// Validate a path for team memory writes with symlink resolution.
///
/// Performs async symlink resolution to prevent escape via symlinks.
/// Checks for null bytes, resolves symlinks, and verifies containment
/// within the team directory using canonical paths.
pub async fn validate_team_memory_write_path(
    path: &Path,
    team_dir: &Path,
) -> std::result::Result<PathBuf, PathTraversalError> {
    let path_str = path.to_string_lossy();

    // Reject null bytes
    if path_str.contains('\0') {
        return Err(PathTraversalError {
            message: "Path contains null bytes".to_string(),
        });
    }

    // Reject relative paths
    if path.is_relative() {
        return Err(PathTraversalError {
            message: "Path must be absolute".to_string(),
        });
    }

    // Reject path traversal components before resolution
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(PathTraversalError {
            message: "Path contains parent directory traversal".to_string(),
        });
    }

    // Resolve symlinks on the parent directory (the file itself may not exist yet).
    // This catches symlink attacks where a component of the path is a symlink
    // pointing outside the team directory.
    let parent = path.parent().ok_or_else(|| PathTraversalError {
        message: "Path has no parent directory".to_string(),
    })?;

    let canonical_parent = match tokio::fs::canonicalize(parent).await {
        Ok(p) => p,
        Err(e) if e.raw_os_error() == Some(40) => {
            // ELOOP: too many levels of symbolic links
            return Err(PathTraversalError {
                message: "Symlink loop detected in path".to_string(),
            });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Dangling symlink or nonexistent parent
            return Err(PathTraversalError {
                message: format!("Parent directory does not exist: {}", parent.display()),
            });
        }
        Err(e) => {
            return Err(PathTraversalError {
                message: format!("Failed to resolve path: {e}"),
            });
        }
    };

    let canonical_team_dir = match tokio::fs::canonicalize(team_dir).await {
        Ok(p) => p,
        Err(e) => {
            return Err(PathTraversalError {
                message: format!("Failed to resolve team directory: {e}"),
            });
        }
    };

    // Reconstruct the full canonical path with the filename
    let file_name = path.file_name().ok_or_else(|| PathTraversalError {
        message: "Path has no file name".to_string(),
    })?;
    let canonical_path = canonical_parent.join(file_name);

    // Verify containment
    if !canonical_path.starts_with(&canonical_team_dir) {
        return Err(PathTraversalError {
            message: format!(
                "Path escapes team memory directory: resolved to {}",
                canonical_path.display()
            ),
        });
    }

    Ok(canonical_path)
}

#[cfg(test)]
#[path = "path_check.test.rs"]
mod tests;
