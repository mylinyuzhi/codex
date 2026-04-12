//! Team memory directory paths and write validation.
//!
//! TS: memdir/teamMemPaths.ts — team memory is stored in a `team/`
//! subdirectory of the auto-memory directory.
//! Includes symlink-aware write validation to prevent path traversal.

use std::path::Path;
use std::path::PathBuf;

use crate::security::PathValidationError;

/// Get the team memory directory path.
///
/// Team memories are stored at `{memory_dir}/team/`.
pub fn team_memory_dir(memory_dir: &Path) -> PathBuf {
    memory_dir.join("team")
}

/// Get the team MEMORY.md index path.
pub fn team_index_path(memory_dir: &Path) -> PathBuf {
    team_memory_dir(memory_dir).join("MEMORY.md")
}

/// Ensure the team memory directory exists.
pub fn ensure_team_dir(memory_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(team_memory_dir(memory_dir))?;
    Ok(())
}

/// Check if team memory exists (has at least one file).
pub fn has_team_memory(memory_dir: &Path) -> bool {
    let dir = team_memory_dir(memory_dir);
    if !dir.is_dir() {
        return false;
    }
    std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        })
        .unwrap_or(false)
}

/// Read the team MEMORY.md content.
pub fn read_team_index(memory_dir: &Path) -> Option<String> {
    let path = team_index_path(memory_dir);
    std::fs::read_to_string(&path).ok()
}

/// Validate an absolute path is safe for writing to the team memory directory.
///
/// TS: teamMemPaths.ts validateTeamMemWritePath.
///
/// Two-pass validation:
/// 1. Lexical: resolve + string-level check against team dir
/// 2. Symlink: canonicalize deepest existing ancestor and re-check
///
/// Returns the resolved absolute path on success.
pub fn validate_team_mem_write_path(
    file_path: &Path,
    memory_dir: &Path,
) -> Result<PathBuf, PathValidationError> {
    let team_dir = team_memory_dir(memory_dir);

    // Pass 1: lexical check
    let resolved = if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        team_dir.join(file_path)
    };

    let normalized = normalize_path(&resolved);
    let team_normalized = normalize_path(&team_dir);

    if !normalized.starts_with(&team_normalized) {
        return Err(PathValidationError::Escape);
    }

    // Pass 2: symlink resolution (if path or ancestor exists)
    if let Some(real) = realpath_deepest_existing(&normalized) {
        let real_team = team_dir.canonicalize().unwrap_or(team_normalized);
        if !real.starts_with(&real_team) {
            return Err(PathValidationError::Escape);
        }
    }

    Ok(normalized)
}

/// Validate a relative path key from a server/external source.
///
/// TS: teamMemPaths.ts validateTeamMemKey.
pub fn validate_team_mem_key(
    relative_key: &str,
    memory_dir: &Path,
) -> Result<PathBuf, PathValidationError> {
    // Sanitize the key first
    crate::security::validate_memory_path(relative_key)?;

    let team_dir = team_memory_dir(memory_dir);
    let resolved = team_dir.join(relative_key);

    validate_team_mem_write_path(&resolved, memory_dir)
}

/// Check if a file path is within the team memory directory.
pub fn is_team_mem_path(file_path: &Path, memory_dir: &Path) -> bool {
    let team_dir = team_memory_dir(memory_dir);
    let normalized = normalize_path(file_path);
    let team_normalized = normalize_path(&team_dir);
    normalized.starts_with(&team_normalized)
}

/// Resolve symlinks for the deepest existing ancestor of a path.
///
/// TS: teamMemPaths.ts realpathDeepestExisting.
/// Walks up the directory tree until canonicalize succeeds, then re-appends
/// the remaining components.
fn realpath_deepest_existing(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = path.canonicalize() {
        return Some(real);
    }

    // Walk up until we find an existing ancestor
    let mut ancestor = path.parent()?;
    let mut remaining = vec![path.file_name()?];

    loop {
        if let Ok(real) = ancestor.canonicalize() {
            let mut result = real;
            for component in remaining.into_iter().rev() {
                result = result.join(component);
            }
            return Some(result);
        }
        remaining.push(ancestor.file_name()?);
        ancestor = ancestor.parent()?;
    }
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
