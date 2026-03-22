//! Path whitelisting for auto memory.
//!
//! Determines whether a file path is within the auto memory directory,
//! used to bypass Write/Edit permission prompts for memory files.

use std::path::Path;

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

#[cfg(test)]
#[path = "path_check.test.rs"]
mod tests;
