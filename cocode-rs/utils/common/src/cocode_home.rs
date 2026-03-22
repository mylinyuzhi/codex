//! Shared cocode home directory resolution.

use std::path::PathBuf;

/// Environment variable for custom cocode home directory.
pub const COCODE_HOME_ENV: &str = "COCODE_HOME";

/// Default cocode directory name.
const DEFAULT_DIR: &str = ".cocode";

/// Resolve the cocode home directory.
///
/// Checks `COCODE_HOME` env var first, falls back to `~/.cocode`.
/// If no home directory can be determined, falls back to `./.cocode`.
pub fn find_cocode_home() -> PathBuf {
    std::env::var(COCODE_HOME_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(DEFAULT_DIR)
        })
}

#[cfg(test)]
#[path = "cocode_home.test.rs"]
mod tests;
