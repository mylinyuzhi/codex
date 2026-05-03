//! Resolve the memory directory for a project.
//!
//! TS: `memdir/paths.ts:getAutoMemPath`. The TS chain is:
//!   1. `CLAUDE_COWORK_MEMORY_PATH_OVERRIDE` env override (operator)
//!   2. `CLAUDE_CODE_REMOTE_MEMORY_DIR` env (CCR / swarm leader)
//!   3. `settings.json` `autoMemoryDirectory`
//!   4. `<config_home>/projects/<sanitized-canonical-git-root>/memory/`
//!
//! Steps (1)-(3) are folded into `coco_config::MemoryConfig::resolve`
//! by the time we see the runtime adapter — its `directory: Option<PathBuf>`
//! already represents the result of those overrides. This module
//! handles step (4): the default layout under the config home, anchored
//! to the **canonical** git root so worktrees of the same repo share
//! one memory dir.

use std::path::Path;
use std::path::PathBuf;

/// Memory directory layout for a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryDir {
    /// Personal directory.
    pub personal: PathBuf,
    /// Team subdirectory (`personal/team/`). Always derivable but
    /// pre-computed for callsite ergonomics.
    pub team: PathBuf,
}

impl MemoryDir {
    /// Resolve from a project root.
    ///
    /// `override_dir` wins outright (settings + env layers already
    /// merged by `coco_config::MemoryConfig::resolve`). Otherwise the
    /// default layout is `<config_home>/projects/<sanitized>/memory/`,
    /// where `sanitized` is derived from the **canonical** git root so
    /// linked worktrees share one memory dir. When `project_root`
    /// isn't inside a git repo, fall back to the path itself.
    pub fn resolve(config_home: &Path, project_root: &Path, override_dir: Option<&Path>) -> Self {
        let personal = match override_dir {
            Some(custom) => custom.to_path_buf(),
            None => {
                let canonical = coco_git::find_canonical_git_root(project_root)
                    .unwrap_or_else(|| project_root.to_path_buf());
                let sanitized = sanitize_project_path(&canonical);
                config_home.join("projects").join(sanitized).join("memory")
            }
        };
        let team = personal.join("team");
        Self { personal, team }
    }

    /// Path to the personal `MEMORY.md` index.
    pub fn personal_index(&self) -> PathBuf {
        self.personal.join(crate::store::ENTRYPOINT_NAME)
    }

    /// Path to the team `MEMORY.md` index.
    pub fn team_index(&self) -> PathBuf {
        self.team.join(crate::store::ENTRYPOINT_NAME)
    }
}

/// Sanitize a path for use as a directory name.
///
/// Replaces path separators with `-` and strips leading separators.
/// Mirrors TS `sanitizePath` in `memdir/paths.ts`.
pub fn sanitize_project_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    s.trim_start_matches(['/', '\\']).replace(['/', '\\'], "-")
}

#[cfg(test)]
#[path = "resolve.test.rs"]
mod tests;
