//! Session memory on-disk path resolution.
//!
//! TS: `utils/permissions/filesystem.ts:270 getSessionMemoryPath()`
//! returns `{projectDir}/{sessionId}/session-memory/summary.md`.
//!
//! coco-rs lays it out under the configured `~/.coco/` home so a
//! session-memory move follows the same root as other session state.

use std::path::Path;
use std::path::PathBuf;

const SESSION_MEMORY_DIR: &str = "session-memory";
const SESSION_MEMORY_FILE: &str = "summary.md";

/// Resolve the absolute path of the session-memory summary file for
/// `session_id` rooted at `config_home` (typically `~/.coco/`).
///
/// Caller is responsible for `create_dir_all(parent)` before write.
#[must_use]
pub fn session_memory_path(config_home: &Path, session_id: &str) -> PathBuf {
    config_home
        .join("sessions")
        .join(session_id)
        .join(SESSION_MEMORY_DIR)
        .join(SESSION_MEMORY_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_session_memory_path_layout() {
        let p = session_memory_path(Path::new("/home/u/.coco"), "abc123");
        assert!(p.ends_with("sessions/abc123/session-memory/summary.md"));
        assert!(p.starts_with("/home/u/.coco"));
    }
}
