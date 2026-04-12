//! Auto-memory configuration.
//!
//! TS: memdir/paths.ts (isAutoMemoryEnabled, getAutoMemPath) +
//!     utils/settings/types.ts (autoMemoryEnabled, autoMemoryDirectory, autoDreamEnabled)

use std::path::Path;
use std::path::PathBuf;

/// Complete auto-memory configuration.
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Whether auto-memory is enabled.
    pub enabled: bool,
    /// Whether background extraction is enabled.
    pub extraction_enabled: bool,
    /// Whether auto-dream consolidation is enabled.
    pub auto_dream_enabled: bool,
    /// Whether team memory is enabled.
    pub team_memory_enabled: bool,
    /// Custom memory directory (overrides default).
    pub custom_directory: Option<PathBuf>,
    /// Extraction throttle: run every N turns (default 1 = every turn).
    pub extraction_throttle: i32,
    /// Whether to skip MEMORY.md index updates (single-step write).
    pub skip_index: bool,
    /// Whether KAIROS mode is active (daily logs + nightly consolidation).
    pub kairos_enabled: bool,
    /// Auto-dream minimum hours between consolidations (default 24).
    pub auto_dream_min_hours: i32,
    /// Auto-dream minimum sessions before consolidation (default 5).
    pub auto_dream_min_sessions: i32,
    /// Maximum relevant memories to surface per turn.
    pub max_relevant_memories: i32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extraction_enabled: true,
            auto_dream_enabled: false,
            team_memory_enabled: false,
            custom_directory: None,
            extraction_throttle: 1,
            skip_index: false,
            kairos_enabled: false,
            auto_dream_min_hours: 24,
            auto_dream_min_sessions: 5,
            max_relevant_memories: 5,
        }
    }
}

impl MemoryConfig {
    /// Create a disabled configuration (bare mode / --no-memory).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            extraction_enabled: false,
            auto_dream_enabled: false,
            ..Self::default()
        }
    }

    /// Create config from environment variables.
    ///
    /// Checks:
    /// - `CLAUDE_CODE_DISABLE_AUTO_MEMORY` → disables all
    /// - `CLAUDE_CODE_SIMPLE` (bare mode) → disables all
    /// - `CLAUDE_CODE_REMOTE_MEMORY_DIR` → custom directory
    /// - `CLAUDE_COWORK_MEMORY_PATH_OVERRIDE` → full path override
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if std::env::var("CLAUDE_CODE_DISABLE_AUTO_MEMORY").is_ok_and(|v| v == "1" || v == "true") {
            return Self::disabled();
        }

        if std::env::var("CLAUDE_CODE_SIMPLE").is_ok_and(|v| v == "1" || v == "true") {
            return Self::disabled();
        }

        // Custom directory overrides
        if let Ok(dir) = std::env::var("CLAUDE_COWORK_MEMORY_PATH_OVERRIDE") {
            config.custom_directory = Some(PathBuf::from(dir));
        } else if let Ok(dir) = std::env::var("CLAUDE_CODE_REMOTE_MEMORY_DIR") {
            config.custom_directory = Some(PathBuf::from(dir));
        }

        config
    }

    /// Resolve the memory directory path for a project.
    ///
    /// Priority:
    /// 1. `custom_directory` (env override)
    /// 2. `~/.claude/projects/<sanitized-cwd>/memory/`
    pub fn resolve_memory_dir(&self, project_root: &Path) -> PathBuf {
        if let Some(custom) = &self.custom_directory {
            return custom.clone();
        }
        resolve_default_memory_dir(project_root)
    }

    /// Resolve the team memory directory (subdirectory of memory dir).
    pub fn resolve_team_memory_dir(&self, project_root: &Path) -> PathBuf {
        self.resolve_memory_dir(project_root).join("team")
    }
}

/// Resolve the default memory directory for a project root.
///
/// Path: `~/.claude/projects/<sanitized-cwd>/memory/`
fn resolve_default_memory_dir(project_root: &Path) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let sanitized = sanitize_project_path(project_root);
    PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(sanitized)
        .join("memory")
}

/// Sanitize a project root path for use as a directory name.
///
/// Replaces path separators with `-` and strips leading `/`.
fn sanitize_project_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    s.trim_start_matches('/').replace(['/', '\\'], "-")
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
