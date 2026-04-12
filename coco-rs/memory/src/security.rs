//! Memory path validation and security.
//!
//! TS: memdir/paths.ts — validateMemoryPath, sanitizePathKey.
//! Prevents path traversal attacks, null byte injection, and Unicode
//! normalization exploits.

use std::path::Path;
use std::path::PathBuf;

/// Validation error for memory paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathValidationError {
    /// Path contains `..` traversal.
    Traversal,
    /// Path contains null bytes.
    NullByte,
    /// Path is absolute (must be relative within memdir).
    AbsolutePath,
    /// Path contains UNC prefix (`\\`).
    UncPath,
    /// Path contains dangerous Unicode characters (fullwidth `.` or `/`).
    UnicodeTraversal,
    /// Path is empty.
    Empty,
    /// Path escapes the memory directory.
    Escape,
}

impl std::fmt::Display for PathValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Traversal => write!(f, "path contains '..' traversal"),
            Self::NullByte => write!(f, "path contains null bytes"),
            Self::AbsolutePath => write!(f, "path must be relative"),
            Self::UncPath => write!(f, "UNC paths not allowed"),
            Self::UnicodeTraversal => write!(f, "path contains dangerous Unicode characters"),
            Self::Empty => write!(f, "path is empty"),
            Self::Escape => write!(f, "path escapes the memory directory"),
        }
    }
}

impl std::error::Error for PathValidationError {}

/// Validate a memory file path (relative to the memory directory).
///
/// Rejects:
/// - `..` traversal components
/// - Null bytes (`\0`)
/// - Absolute paths (`/foo`, `C:\foo`)
/// - UNC paths (`\\server\share`)
/// - Fullwidth Unicode dots and slashes (U+FF0E `．`, U+FF0F `／`)
/// - Empty paths
pub fn validate_memory_path(path: &str) -> Result<(), PathValidationError> {
    if path.is_empty() {
        return Err(PathValidationError::Empty);
    }

    // Null byte injection
    if path.contains('\0') {
        return Err(PathValidationError::NullByte);
    }

    // UNC paths
    if path.starts_with("\\\\") {
        return Err(PathValidationError::UncPath);
    }

    // Absolute paths
    if path.starts_with('/') || (path.len() >= 2 && path.as_bytes()[1] == b':') {
        return Err(PathValidationError::AbsolutePath);
    }

    // Traversal
    for component in path.split(['/', '\\']) {
        if component == ".." {
            return Err(PathValidationError::Traversal);
        }
    }

    // Unicode fullwidth attacks
    // U+FF0E = ．(fullwidth full stop)
    // U+FF0F = ／(fullwidth solidus)
    if path.contains('\u{FF0E}') || path.contains('\u{FF0F}') {
        return Err(PathValidationError::UnicodeTraversal);
    }

    // URL-encoded traversal (%2e%2e, %2f)
    let decoded = path
        .replace("%2e", ".")
        .replace("%2E", ".")
        .replace("%2f", "/")
        .replace("%2F", "/");
    for component in decoded.split(['/', '\\']) {
        if component == ".." {
            return Err(PathValidationError::Traversal);
        }
    }

    Ok(())
}

/// Validate that a resolved path stays within the memory directory.
///
/// After canonicalization, the path must be a descendant of `memory_dir`.
pub fn validate_resolved_path(
    path: &Path,
    memory_dir: &Path,
) -> Result<PathBuf, PathValidationError> {
    // Use lexical check (not canonicalize, which requires file to exist)
    let resolved = memory_dir.join(path);
    let normalized = normalize_path(&resolved);
    let mem_normalized = normalize_path(memory_dir);

    if !normalized.starts_with(&mem_normalized) {
        return Err(PathValidationError::Escape);
    }

    Ok(normalized)
}

/// Check if a path is within the auto-memory directory.
///
/// Used by the permission system to grant write carve-outs for memory files.
pub fn is_within_memory_dir(path: &Path, memory_dir: &Path) -> bool {
    let normalized = normalize_path(path);
    let mem_normalized = normalize_path(memory_dir);
    normalized.starts_with(&mem_normalized)
}

/// Normalize a path by resolving `.` and `..` without filesystem access.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

/// Sanitize a string for use as a memory file path key.
///
/// Strips dangerous characters, normalizes to lowercase.
pub fn sanitize_path_key(key: &str) -> String {
    key.to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
                Some(c)
            } else if c == ' ' {
                Some('_')
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "security.test.rs"]
mod tests;
