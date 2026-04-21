//! Auto-memory configuration.
//!
//! TS: memdir/paths.ts (isAutoMemoryEnabled, getAutoMemPath) +
//!     utils/settings/types.ts (autoMemoryEnabled, autoMemoryDirectory, autoDreamEnabled)

use std::path::Path;
use std::path::PathBuf;

/// Complete auto-memory configuration.
///
/// Mirrors `coco_config::MemoryConfig` with a runtime-side custom
/// `custom_directory` field (settings resolution uses `directory`,
/// the runtime consumer uses `custom_directory` for clarity). Auto-
/// dream / KAIROS / max-relevant-memories fields were removed; re-add
/// alongside their consumers in `prefetch.rs` / `dream.rs` when those
/// pipelines ship.
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Whether auto-memory is enabled.
    pub enabled: bool,
    /// Whether background extraction is enabled.
    pub extraction_enabled: bool,
    /// Whether team memory is enabled.
    pub team_memory_enabled: bool,
    /// Custom memory directory (overrides default).
    pub custom_directory: Option<PathBuf>,
    /// Extraction throttle: run every N turns (default 1 = every turn).
    pub extraction_throttle: i32,
    /// Whether to skip MEMORY.md index updates (single-step write).
    pub skip_index: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extraction_enabled: true,
            team_memory_enabled: false,
            custom_directory: None,
            extraction_throttle: 1,
            skip_index: false,
        }
    }
}

impl MemoryConfig {
    /// Create a disabled configuration (bare mode / --no-memory).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            extraction_enabled: false,
            ..Self::default()
        }
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

impl From<coco_config::MemoryConfig> for MemoryConfig {
    fn from(config: coco_config::MemoryConfig) -> Self {
        Self {
            enabled: config.enabled,
            extraction_enabled: config.extraction_enabled,
            team_memory_enabled: config.team_memory_enabled,
            custom_directory: config.directory,
            extraction_throttle: config.extraction_throttle,
            skip_index: config.skip_index,
        }
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
