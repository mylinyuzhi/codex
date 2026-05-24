//! Auto memory directory resolution and management.
//!
//! Resolves the memory directory path and ensures it exists.

use std::path::Path;
use std::path::PathBuf;

use sha2::Digest;
use sha2::Sha256;
use snafu::ResultExt;
use tracing::debug;

use crate::error::auto_memory_error as err;

// Environment variable names (matching cocode-config env_loader constants).
// Canonical definitions — config.rs re-uses these via `crate::directory::`.
pub(crate) const ENV_COWORK_MEMORY_PATH_OVERRIDE: &str = "COCODE_COWORK_MEMORY_PATH_OVERRIDE";
const ENV_AUTO_MEMORY_DIR: &str = "COCODE_AUTO_MEMORY_DIR";
const ENV_AUTO_MEMORY_DIR_COMPAT: &str = "CLAUDE_CODE_AUTO_MEMORY_DIR";
pub(crate) const ENV_REMOTE_MEMORY_DIR: &str = "COCODE_REMOTE_MEMORY_DIR";
pub(crate) const ENV_REMOTE_MEMORY_DIR_COMPAT: &str = "CLAUDE_CODE_REMOTE_MEMORY_DIR";

/// Get the auto memory directory path.
///
/// Priority chain:
/// 1. `COCODE_COWORK_MEMORY_PATH_OVERRIDE` env var
/// 2. `COCODE_AUTO_MEMORY_DIR` env var
/// 3. `custom_dir` from config settings
/// 4. Default: `{home}/.cocode/projects/{hash}/memory/`
pub fn get_auto_memory_directory(cwd: &Path, custom_dir: Option<&str>) -> PathBuf {
    // Priority 1: Cowork override
    if let Some(val) = get_non_empty_env(ENV_COWORK_MEMORY_PATH_OVERRIDE) {
        debug!(path = %val, "Using cowork memory path override");
        return PathBuf::from(val);
    }

    // Priority 2: Env var override
    if let Some(val) = get_non_empty_env(ENV_AUTO_MEMORY_DIR)
        .or_else(|| get_non_empty_env(ENV_AUTO_MEMORY_DIR_COMPAT))
    {
        debug!(path = %val, "Using auto memory dir env var");
        return PathBuf::from(val);
    }

    // Priority 3: Custom directory from config
    if let Some(dir) = custom_dir.filter(|d| !d.is_empty()) {
        debug!(path = %dir, "Using custom autoMemory.directory");
        return PathBuf::from(dir);
    }

    // Priority 4: Default project-hash-based path
    let home = get_home_directory();
    let hash = project_hash(cwd);
    home.join("projects").join(hash).join("memory")
}

/// Team memory subdirectory name.
pub const TEAM_MEMORY_SUBDIR: &str = "team";

/// Get the team memory directory path.
pub fn get_team_memory_directory(memory_dir: &Path) -> PathBuf {
    memory_dir.join(TEAM_MEMORY_SUBDIR)
}

/// Ensure the memory directory exists.
pub async fn ensure_memory_dir_exists(dir: &Path) -> crate::Result<()> {
    tokio::fs::create_dir_all(dir)
        .await
        .context(err::CreateDirSnafu {
            path: dir.display().to_string(),
        })
}

/// Compute a project hash from the working directory.
///
/// Uses SHA-256 of the path, truncated to the first 12 hex characters.
pub fn project_hash(cwd: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cwd.to_string_lossy().as_bytes());
    let result = hasher.finalize();
    cocode_utils_string::bytes_to_hex(&result[..6])
}

/// Get a non-empty environment variable value.
fn get_non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Get the home directory for memory storage.
///
/// Checks `COCODE_REMOTE_MEMORY_DIR` first, then falls back to
/// the local cocode home directory (`~/.cocode/`).
fn get_home_directory() -> PathBuf {
    if let Some(val) = get_non_empty_env(ENV_REMOTE_MEMORY_DIR)
        .or_else(|| get_non_empty_env(ENV_REMOTE_MEMORY_DIR_COMPAT))
    {
        return PathBuf::from(val);
    }

    // Default: ~/.cocode/
    dirs::home_dir()
        .unwrap_or_else(|| {
            tracing::warn!(
                "Home directory unavailable, falling back to /tmp/.cocode/ for memory storage"
            );
            PathBuf::from("/tmp")
        })
        .join(".cocode")
}

#[cfg(test)]
#[path = "directory.test.rs"]
mod tests;
