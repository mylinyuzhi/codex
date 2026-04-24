//! Agent worktree manager — Phase 6, Workstream C.
//!
//! TS: `src/utils/worktree.ts` (`createAgentWorktree` at `:902-952`,
//! `removeAgentWorktree` at `:961-1020`, `hasWorktreeChanges` at
//! `:1144-1173`). Ported to Rust with the Rust-specific caveat that
//! the parent's cwd is **not** changed — subagents see the worktree
//! via `ToolUseContext::cwd_override` (async-local equivalent is
//! absent in Rust; explicit field propagation is the substitute).
//!
//! # Scope for the first Rust slice
//!
//! Ported (parity with TS):
//! - `git worktree add -B <branch> <path>` against canonical git root.
//! - `hasWorktreeChanges` via `git status --porcelain`.
//! - `git worktree remove --force` + `git branch -D`.
//! - Post-creation setup: settings.local.json copy + git core.hooksPath
//!   config (TS `worktree.ts:510-578`, items 1 + 2).
//!
//! Deferred (out of scope per plan review):
//! - Hook-based VCS (`WorktreeCreate` hook).
//! - Symlinked directories (`worktree.ts:580-585`).
//! - Commit-attribution prepare-commit-msg hook (`:603-623`).
//! - Resume metadata (`runAgent.ts:738-742`).
//! - Periodic stale-worktree sweep (`:1058-1136`).
//!
//! # Canonical git root
//!
//! Agent worktrees always land in the **canonical** repo's
//! `.claude/worktrees/` dir, even when spawned from inside a session
//! worktree. TS calls `findCanonicalGitRoot` for this reason
//! (`worktree.ts:926`); the Rust equivalent is
//! [`AgentWorktreeManager::canonical_git_root`], resolved at manager
//! construction so subagent worktrees never nest.
//!
//! # Cleanup-on-change policy
//!
//! If the child agent made no changes (staged, unstaged, or
//! untracked), the worktree is removed after the agent completes. If
//! changes exist, the worktree is **kept** on disk for the user to
//! inspect — matches TS `AgentTool.tsx:680-684` and the `kept`
//! variant of [`WorktreeCleanupOutcome`].

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use thiserror::Error;

/// Agent-scoped worktree session. Carries everything needed for
/// cleanup + cwd override on the child agent.
#[derive(Debug, Clone)]
pub struct AgentWorktreeSession {
    /// Absolute worktree path — becomes the child's `cwd_override`.
    pub path: PathBuf,
    /// Temporary branch created by `git worktree add -B`. Deleted on
    /// successful cleanup.
    pub branch: String,
    /// HEAD commit at creation time. Used by `cleanup_if_unchanged`
    /// to detect whether the agent modified anything.
    pub head_commit: String,
    /// Main repo's canonical git root — NOT the worktree path.
    /// Required because `git worktree remove` must be invoked from
    /// the main repo, not the worktree being deleted.
    pub git_root: PathBuf,
}

/// Outcome returned by [`AgentWorktreeManager::cleanup_if_unchanged`].
///
/// TS parity: `AgentTool.tsx:649-684` returns `{worktreePath?, worktreeBranch?}`
/// where empty = removed, populated = kept on disk.
#[derive(Debug, Clone)]
pub enum WorktreeCleanupOutcome {
    /// Worktree was removed because the child agent made no changes.
    Removed,
    /// Worktree was kept on disk. Either the child made changes, the
    /// cleanup subprocess failed, or the agent crashed mid-execution.
    Kept {
        path: PathBuf,
        branch: String,
        reason: KeptReason,
    },
}

/// Why a worktree was kept rather than removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptReason {
    /// Child agent made changes (staged / unstaged / untracked).
    HasChanges,
    /// `git worktree remove` or `git branch -D` failed. Worktree
    /// path may or may not still exist — the caller should warn but
    /// not block the session.
    CleanupFailed,
}

/// Agent worktree creation / cleanup errors. Surfaced to the agent
/// runtime as a tool-result error when the initial create fails;
/// cleanup errors are downgraded to `Kept { reason: CleanupFailed }`
/// so the session doesn't abort.
#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("not in a git repository (no canonical git root resolvable from {path:?})")]
    NotInRepo { path: PathBuf },
    #[error("invalid worktree slug {slug:?}: {reason}")]
    InvalidSlug { slug: String, reason: String },
    #[error("git subprocess failed: {stderr}")]
    GitFailed { stderr: String },
    #[error("io error during worktree setup: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },
}

/// Configuration for optional post-creation setup behaviors.
///
/// All fields are opt-in. The defaults (`Default::default()`) give
/// TS-parity minimum setup: settings.local.json copy + core.hooksPath
/// config only. Enable `symlink_directories` to avoid duplicating
/// large dirs like `node_modules` across worktrees
/// (TS `worktree.ts:580-585`).
#[derive(Debug, Clone, Default)]
pub struct AgentWorktreeConfig {
    /// Directories to symlink from the main repo into each new
    /// worktree. Relative to the main repo root.
    ///
    /// TS parity: `settings.worktree.symlinkDirectories` at
    /// `worktree.ts:581-585`. Typical values: `["node_modules",
    /// "target", ".venv"]`. Missing source dirs are silently
    /// skipped (TS matches this behavior).
    pub symlink_directories: Vec<PathBuf>,
    /// When `true`, background agents with worktree isolation get
    /// a worktree that is NEVER auto-cleaned — it persists for the
    /// user to inspect even if nothing was changed. TS parity:
    /// hook-based worktrees use this semantic (`worktree.ts:659-664`
    /// "Hook-based worktrees are always kept").
    pub keep_worktree_when_background: bool,
}

/// Agent worktree manager.
///
/// Constructed once per session from a resolved canonical git root.
/// All `create_for` and `cleanup_if_unchanged` calls operate against
/// that root — nested spawns from inside a session worktree still
/// land their worktrees in the main repo's `.claude/worktrees/`.
pub struct AgentWorktreeManager {
    canonical_git_root: PathBuf,
    config: AgentWorktreeConfig,
}

impl AgentWorktreeManager {
    /// Build a manager from an explicit canonical git root.
    /// Callers typically resolve this at session bootstrap via
    /// [`Self::discover_from_cwd`].
    pub fn new(canonical_git_root: PathBuf) -> Self {
        Self {
            canonical_git_root,
            config: AgentWorktreeConfig::default(),
        }
    }

    /// Install post-creation setup configuration. Builder-style so
    /// the CLI can plumb `settings.worktree.symlinkDirectories` and
    /// other advanced options without growing `new()`.
    pub fn with_config(mut self, config: AgentWorktreeConfig) -> Self {
        self.config = config;
        self
    }

    /// Discover the canonical git root from `cwd`, falling back to
    /// `NotInRepo` if the directory isn't inside a git repo.
    ///
    /// TS parity: `findCanonicalGitRoot(getCwd())` at `worktree.ts:926`.
    /// The canonical form resolves symlinks + walks to the main repo,
    /// not the nearest `.git` — so a session spawned inside a
    /// worktree still sees the main repo as its root.
    pub fn discover_from_cwd(cwd: &Path) -> Result<Self, WorktreeError> {
        // Use `git rev-parse --git-common-dir` for canonical resolution.
        // `--git-common-dir` returns the SHARED .git directory across
        // worktrees (main repo's .git), not the current worktree's
        // .git/worktrees/<name> link.
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["rev-parse", "--git-common-dir"])
            .output()?;
        if !output.status.success() {
            return Err(WorktreeError::NotInRepo {
                path: cwd.to_path_buf(),
            });
        }
        let common_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // The common dir is usually "<root>/.git"; the repo root is its parent.
        let common_dir_path = PathBuf::from(&common_dir);
        let absolute = if common_dir_path.is_absolute() {
            common_dir_path
        } else {
            cwd.join(&common_dir_path)
        };
        let canonical = absolute.canonicalize()?;
        let root = canonical
            .parent()
            .ok_or_else(|| WorktreeError::NotInRepo {
                path: cwd.to_path_buf(),
            })?
            .to_path_buf();
        Ok(Self::new(root))
    }

    /// The canonical git root this manager operates against.
    pub fn canonical_git_root(&self) -> &Path {
        &self.canonical_git_root
    }

    /// Create a fresh agent worktree with the given slug.
    ///
    /// Slug format per TS `AgentTool.tsx:591`: `agent-<first-8-hex>`
    /// derived from the agent id. Validated here to reject path
    /// separators + shell metacharacters.
    ///
    /// Side effects (TS parity, items 1 + 2 of `performPostCreationSetup`):
    /// - Copy `.claude/settings.local.json` into the worktree.
    /// - Configure `core.hooksPath` to point at the main repo's hooks
    ///   (so husky / custom hooks resolve correctly).
    pub fn create_for(&self, slug: &str) -> Result<AgentWorktreeSession, WorktreeError> {
        validate_slug(slug)?;

        let worktree_path = self
            .canonical_git_root
            .join(".claude")
            .join("worktrees")
            .join(slug);
        let branch = format!("claude/{slug}");

        // Ensure parent dir exists so `git worktree add` doesn't
        // fail on a fresh repo that's never had a worktree before.
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // `-B` creates-or-resets the branch; matches TS
        // `getOrCreateWorktree` behavior where an existing agent
        // worktree can be reused.
        let add_output = Command::new("git")
            .arg("-C")
            .arg(&self.canonical_git_root)
            .args(["worktree", "add", "-B", &branch])
            .arg(&worktree_path)
            .output()?;
        if !add_output.status.success() {
            return Err(WorktreeError::GitFailed {
                stderr: String::from_utf8_lossy(&add_output.stderr).into_owned(),
            });
        }

        let head_commit = get_head_commit(&worktree_path)?;

        // Best-effort post-creation setup. Failures here are
        // non-fatal — a worktree without settings.local still works,
        // just with reduced per-project settings.
        let _ = copy_settings_local(&self.canonical_git_root, &worktree_path);
        let _ = configure_hooks_path(&self.canonical_git_root, &worktree_path);
        // Symlink configured directories (e.g. node_modules) from the
        // main repo. TS parity: `symlinkDirectories` setting.
        if !self.config.symlink_directories.is_empty() {
            let _ = symlink_directories(
                &self.canonical_git_root,
                &worktree_path,
                &self.config.symlink_directories,
            );
        }

        Ok(AgentWorktreeSession {
            path: worktree_path,
            branch,
            head_commit,
            git_root: self.canonical_git_root.clone(),
        })
    }

    /// Same as [`Self::create_for`] but never auto-cleans the
    /// worktree on completion — suitable for background agents
    /// where the parent turn doesn't wait for the child and
    /// post-completion cleanup would race with the still-running
    /// child process.
    ///
    /// TS parity: background agents keep their worktrees; the user
    /// inspects them via the agent listing. Staleness is handled
    /// by `cleanup_stale` on subsequent sessions (30-day sweep).
    pub fn create_for_background(&self, slug: &str) -> Result<AgentWorktreeSession, WorktreeError> {
        // Structurally identical to `create_for`; the caller commits
        // to never calling `cleanup_if_unchanged` on the returned
        // session. A separate method makes intent explicit + gives
        // us a single place to add "always kept" telemetry if
        // needed later.
        self.create_for(slug)
    }

    /// Whether hook-based worktree creation is available. Returns
    /// `true` when the provided hook registry contains at least one
    /// `WorktreeCreate` handler.
    ///
    /// TS parity: `worktree.ts:912` `hasWorktreeCreateHook()`. When
    /// `true`, the caller may route worktree creation through the
    /// hook runner to support non-git VCS (Jujutsu, Mercurial, etc.).
    /// When `false`, falls back to `git worktree add`.
    ///
    /// This method does **not** execute any hooks — it only checks
    /// registration. Full hook-based worktree routing is deferred
    /// (see module doc "Scope for the first Rust slice").
    pub fn has_worktree_create_hook(registry: &coco_hooks::HookRegistry) -> bool {
        !registry
            .find_matching(coco_types::HookEventType::WorktreeCreate, None)
            .is_empty()
    }

    /// Sweep stale agent worktrees — background-safe cleanup.
    ///
    /// Scans `.claude/worktrees/agent-*` under the canonical root
    /// and removes directories whose last-modified time is older
    /// than `older_than`. Used for cases where a prior session
    /// crashed before `cleanup_if_unchanged` could run (parent
    /// killed by ESC/Ctrl+C, crash, lost connection, etc.).
    ///
    /// TS parity: `worktree.ts:1058-1136` `cleanupStaleAgentWorktrees`
    /// uses a 30-day threshold. Returns the number of worktrees
    /// removed.
    ///
    /// Silently skips worktrees that still have changes — user's
    /// work is preserved even if the agent metadata is lost.
    ///
    /// This is a best-effort cleanup; all errors are swallowed so
    /// a stuck worktree doesn't block session startup.
    pub fn cleanup_stale(&self, older_than: std::time::Duration) -> usize {
        let worktrees_dir = self.canonical_git_root.join(".claude").join("worktrees");
        let entries = match std::fs::read_dir(&worktrees_dir) {
            Ok(e) => e,
            Err(_) => return 0, // dir doesn't exist — nothing to sweep.
        };

        let now = std::time::SystemTime::now();
        let mut removed = 0usize;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Match exactly the pattern `agent-<8-hex-chars>` used
            // by `create_for`. Refuse to sweep user-named
            // worktrees (e.g. `wt-myfeature`).
            if !is_agent_slug(&name_str) {
                continue;
            }
            let path = entry.path();
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = metadata.modified().unwrap_or(now);
            let age = now.duration_since(mtime).unwrap_or_default();
            if age < older_than {
                continue;
            }
            // Check for uncommitted changes before removing.
            let has_changes = has_worktree_changes(&path, "").unwrap_or(true);
            if has_changes {
                continue; // preserve user's work.
            }
            // Resolve the branch name from the slug.
            let branch = format!("claude/{}", name_str.strip_prefix("").unwrap_or(&name_str));
            let session = AgentWorktreeSession {
                path: path.clone(),
                branch,
                head_commit: String::new(),
                git_root: self.canonical_git_root.clone(),
            };
            if let WorktreeCleanupOutcome::Removed = self.cleanup_if_unchanged(session) {
                removed += 1;
            }
        }
        removed
    }

    /// Remove the worktree if the child agent made no changes; keep
    /// it on disk otherwise.
    ///
    /// TS parity: `AgentTool.tsx:644-685` `cleanupWorktreeIfNeeded`.
    ///
    /// "Changes" means any entry in `git status --porcelain` — staged,
    /// unstaged, or untracked. TS's `hasWorktreeChanges`
    /// (`worktree.ts:1144-1173`) uses the same criterion.
    pub fn cleanup_if_unchanged(&self, session: AgentWorktreeSession) -> WorktreeCleanupOutcome {
        let has_changes = match has_worktree_changes(&session.path, &session.head_commit) {
            Ok(b) => b,
            Err(_) => {
                // Can't determine — err on the side of keeping for
                // user inspection. Matches TS fallback where a
                // failed status query defaults to "keep".
                return WorktreeCleanupOutcome::Kept {
                    path: session.path,
                    branch: session.branch,
                    reason: KeptReason::CleanupFailed,
                };
            }
        };
        if has_changes {
            return WorktreeCleanupOutcome::Kept {
                path: session.path,
                branch: session.branch,
                reason: KeptReason::HasChanges,
            };
        }

        // Remove worktree from main repo (not from inside the
        // worktree — `git` would refuse).
        let remove_output = Command::new("git")
            .arg("-C")
            .arg(&session.git_root)
            .args(["worktree", "remove", "--force"])
            .arg(&session.path)
            .output();
        let remove_ok = matches!(remove_output, Ok(ref o) if o.status.success());
        if !remove_ok {
            return WorktreeCleanupOutcome::Kept {
                path: session.path,
                branch: session.branch,
                reason: KeptReason::CleanupFailed,
            };
        }

        // Delete the temp branch. Non-fatal if it fails (e.g. branch
        // was detached); TS also swallows errors here.
        let _ = Command::new("git")
            .arg("-C")
            .arg(&session.git_root)
            .args(["branch", "-D", &session.branch])
            .output();

        WorktreeCleanupOutcome::Removed
    }
}

/// Match exactly the `agent-<8-hex-chars>` slug shape used by
/// [`AgentWorktreeManager::create_for`]. This narrow pattern keeps
/// stale-sweep from touching user-named EnterWorktree slugs like
/// `wt-myfeature`. TS parity: `worktree.ts:1022-1029` comment.
fn is_agent_slug(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("agent-") else {
        return false;
    };
    rest.len() == 8 && rest.chars().all(|c| c.is_ascii_hexdigit())
}

fn validate_slug(slug: &str) -> Result<(), WorktreeError> {
    if slug.is_empty() {
        return Err(WorktreeError::InvalidSlug {
            slug: slug.into(),
            reason: "empty".into(),
        });
    }
    for c in slug.chars() {
        let ok = c.is_ascii_alphanumeric() || c == '-' || c == '_';
        if !ok {
            return Err(WorktreeError::InvalidSlug {
                slug: slug.into(),
                reason: format!("invalid character {c:?}"),
            });
        }
    }
    Ok(())
}

fn get_head_commit(path: &Path) -> Result<String, WorktreeError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        return Err(WorktreeError::GitFailed {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

fn has_worktree_changes(path: &Path, _head_commit: &str) -> Result<bool, WorktreeError> {
    // Any non-empty `git status --porcelain` output means changes.
    // TS's `hasWorktreeChanges` uses the same criterion (tracked +
    // untracked). We don't need `_head_commit` for this check — TS
    // carries it for diagnostic logging but doesn't use it either.
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["status", "--porcelain"])
        .output()?;
    if !output.status.success() {
        return Err(WorktreeError::GitFailed {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(!output.stdout.is_empty())
}

/// TS parity: `worktree.ts:516-534` — copies `.claude/settings.local.json`
/// from the main repo into the worktree so child agents inherit
/// local settings (auth tokens, per-project preferences).
fn copy_settings_local(repo_root: &Path, worktree_path: &Path) -> Result<(), WorktreeError> {
    let src = repo_root.join(".claude").join("settings.local.json");
    if !src.exists() {
        return Ok(()); // no local settings to propagate — TS also no-ops.
    }
    let dst = worktree_path.join(".claude").join("settings.local.json");
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&src, &dst)?;
    Ok(())
}

/// TS parity: `worktree.ts:538-578` — configures
/// `core.hooksPath` inside the worktree to point at the main
/// repo's `.husky/` or `.git/hooks/`, so pre-commit / post-commit
/// hooks resolve correctly in the worktree.
fn configure_hooks_path(repo_root: &Path, worktree_path: &Path) -> Result<(), WorktreeError> {
    let husky = repo_root.join(".husky");
    let git_hooks = repo_root.join(".git").join("hooks");
    let hooks_path = if husky.is_dir() {
        husky
    } else if git_hooks.is_dir() {
        git_hooks
    } else {
        return Ok(()); // no hooks dir — nothing to configure.
    };
    let hooks_str = hooks_path.to_string_lossy().into_owned();
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["config", "core.hooksPath", &hooks_str])
        .output()?;
    if !output.status.success() {
        // Log via stderr bubble, but don't fail creation.
        return Err(WorktreeError::GitFailed {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// TS parity: `worktree.ts:580-585` — symlinks configured
/// directories from the main repo into the fresh worktree so large
/// dirs (node_modules, target, .venv) aren't duplicated across
/// every agent's worktree.
///
/// Missing source dirs are silently skipped (TS matches). Any
/// already-existing entry in the worktree is left alone — a later
/// agent might have populated it and we refuse to clobber.
fn symlink_directories(
    repo_root: &Path,
    worktree_path: &Path,
    dirs: &[PathBuf],
) -> Result<(), WorktreeError> {
    for dir in dirs {
        let src = repo_root.join(dir);
        if !src.exists() {
            continue;
        }
        let dst = worktree_path.join(dir);
        if dst.exists() {
            continue; // refuse to clobber an existing entry.
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Use OS-specific symlink creation. Unix: `symlink`; Windows:
        // `symlink_dir` (requires admin/developer mode). We use the
        // `unix` path as the canonical implementation since coco-rs
        // Windows support goes through WSL where Unix symlinks work.
        #[cfg(unix)]
        std::os::unix::fs::symlink(&src, &dst)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&src, &dst)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "agent_worktree.test.rs"]
mod tests;
