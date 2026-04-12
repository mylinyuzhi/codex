//! Shared coco home directory resolution.

use std::path::PathBuf;

/// Environment variable for custom coco home directory.
pub const COCODE_HOME_ENV: &str = "COCODE_HOME";

/// Default coco directory name.
const DEFAULT_DIR: &str = ".coco";

/// Resolve the coco home directory.
///
/// Checks `COCODE_HOME` env var first, falls back to `~/.coco`.
/// If no home directory can be determined, falls back to `./.coco`.
pub fn find_coco_home() -> PathBuf {
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
#[path = "coco_home.test.rs"]
mod tests;
