//! Git worktree management for agent isolation.
//!
//! TS: utils/worktree.ts (worktree creation, resume, cleanup)
//!
//! Provides worktree creation and management for multi-agent workflows where
//! each agent needs an isolated working directory. Worktrees share the same
//! git object store to avoid disk bloat.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// Information about a git worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree directory.
    pub path: PathBuf,
    /// Branch name checked out in this worktree.
    pub branch: String,
    /// HEAD commit SHA.
    pub head_commit: String,
    /// Whether the worktree existed before (resumed) or was newly created.
    pub existed: bool,
}

/// Result of a worktree creation attempt.
#[derive(Debug)]
pub struct WorktreeCreateResult {
    /// The worktree info if creation succeeded.
    pub info: WorktreeInfo,
    /// The base branch used for creation (only set for newly created worktrees).
    pub base_branch: Option<String>,
}

// ── Slug Validation ──

/// Maximum length for a worktree slug.
const MAX_SLUG_LENGTH: usize = 64;

/// Validate a worktree slug to prevent path traversal and directory escape.
///
/// The slug is joined into `.claude/worktrees/<slug>` so we must prevent:
/// - `..` segments that escape the worktrees directory
/// - Absolute paths that discard the prefix
/// - Invalid characters
///
/// Forward slashes are allowed for nesting (e.g., `user/feature`); each
/// segment is validated independently.
pub fn validate_slug(slug: &str) -> anyhow::Result<()> {
    if slug.is_empty() {
        anyhow::bail!("Worktree slug cannot be empty");
    }

    if slug.len() > MAX_SLUG_LENGTH {
        anyhow::bail!(
            "Worktree slug must be {MAX_SLUG_LENGTH} characters or fewer (got {})",
            slug.len()
        );
    }

    for segment in slug.split('/') {
        if segment == "." || segment == ".." {
            anyhow::bail!(
                "Invalid worktree name \"{slug}\": must not contain \".\" or \"..\" path segments"
            );
        }
        if segment.is_empty() {
            anyhow::bail!(
                "Invalid worktree name \"{slug}\": empty segment (leading/trailing slash)"
            );
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            anyhow::bail!(
                "Invalid worktree name \"{slug}\": each segment must contain only \
                 letters, digits, dots, underscores, and dashes"
            );
        }
    }

    Ok(())
}

/// Flatten nested slugs: `user/feature` becomes `user+feature`.
///
/// This avoids D/F conflicts in git refs (`worktree-user` file vs
/// `worktree-user/feature` directory) and prevents nested worktree
/// directories that `git worktree remove` would cascade-delete.
fn flatten_slug(slug: &str) -> String {
    slug.replace('/', "+")
}

/// Compute the branch name for a worktree from its slug.
pub fn worktree_branch_name(slug: &str) -> String {
    format!("worktree-{}", flatten_slug(slug))
}

/// Compute the directory path for a worktree.
fn worktree_dir(repo_root: &Path, slug: &str) -> PathBuf {
    repo_root
        .join(".claude")
        .join("worktrees")
        .join(flatten_slug(slug))
}

// ── Core Operations ──

/// Create a git worktree for agent isolation.
///
/// If the worktree already exists (by checking for a HEAD ref), returns
/// it as resumed. Otherwise creates a new worktree branching from
/// `origin/<default_branch>` (falling back to `HEAD`).
///
/// TS: getOrCreateWorktree() in worktree.ts
pub fn create_worktree(
    repo_root: &Path,
    slug: &str,
    base_branch: Option<&str>,
) -> anyhow::Result<WorktreeCreateResult> {
    validate_slug(slug)?;

    let worktree_path = worktree_dir(repo_root, slug);
    let branch = worktree_branch_name(slug);

    // Fast resume: check if worktree already exists by looking for HEAD
    if let Some(head) = read_worktree_head(&worktree_path) {
        return Ok(WorktreeCreateResult {
            info: WorktreeInfo {
                path: worktree_path,
                branch,
                head_commit: head,
                existed: true,
            },
            base_branch: None,
        });
    }

    // Ensure parent directory exists
    let worktrees_dir = repo_root.join(".claude").join("worktrees");
    std::fs::create_dir_all(&worktrees_dir)?;

    // Determine base branch
    let base = base_branch.unwrap_or("HEAD");

    // Resolve base SHA
    let base_sha = run_git(repo_root, &["rev-parse", base])
        .map_err(|e| anyhow::anyhow!("Failed to resolve base branch \"{base}\": {e}"))?;

    // Create the worktree with -B to reset any orphan branch
    let worktree_path_str = worktree_path.display().to_string();
    let result = run_git(
        repo_root,
        &["worktree", "add", "-B", &branch, &worktree_path_str, base],
    );

    match result {
        Ok(_) => Ok(WorktreeCreateResult {
            info: WorktreeInfo {
                path: worktree_path,
                branch,
                head_commit: base_sha.trim().to_string(),
                existed: false,
            },
            base_branch: Some(base.to_string()),
        }),
        Err(e) => Err(anyhow::anyhow!("Failed to create worktree: {e}")),
    }
}

/// Remove a git worktree.
///
/// Uses `git worktree remove --force` to clean up the worktree directory
/// and unregister it from the git worktree list.
pub fn remove_worktree(repo_root: &Path, slug: &str) -> anyhow::Result<()> {
    validate_slug(slug)?;

    let worktree_path = worktree_dir(repo_root, slug);
    let worktree_path_str = worktree_path.display().to_string();

    run_git(
        repo_root,
        &["worktree", "remove", "--force", &worktree_path_str],
    )
    .map_err(|e| anyhow::anyhow!("Failed to remove worktree: {e}"))?;

    // Also try to delete the branch (best-effort)
    let branch = worktree_branch_name(slug);
    let _ = run_git(repo_root, &["branch", "-D", &branch]);

    Ok(())
}

/// List all git worktrees for this repository.
///
/// Parses the output of `git worktree list --porcelain`.
pub fn list_worktrees(repo_root: &Path) -> anyhow::Result<Vec<WorktreeInfo>> {
    let output = run_git(repo_root, &["worktree", "list", "--porcelain"])?;
    let mut worktrees = Vec::new();
    let mut path = None;
    let mut head = None;
    let mut branch = None;

    for line in output.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            // Flush previous entry
            if let (Some(p), Some(h)) = (path.take(), head.take()) {
                worktrees.push(WorktreeInfo {
                    path: PathBuf::from(p),
                    branch: branch.take().unwrap_or_default(),
                    head_commit: h,
                    existed: true,
                });
            }
            path = Some(p.to_string());
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            head = Some(h.to_string());
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            branch = Some(b.to_string());
        }
    }

    // Flush last entry
    if let (Some(p), Some(h)) = (path, head) {
        worktrees.push(WorktreeInfo {
            path: PathBuf::from(p),
            branch: branch.unwrap_or_default(),
            head_commit: h,
            existed: true,
        });
    }

    Ok(worktrees)
}

/// Check if a worktree has uncommitted changes.
///
/// Runs `git status --porcelain` in the worktree directory and returns
/// `true` if there is any output (indicating changes).
pub fn has_changes(worktree_path: &Path) -> anyhow::Result<bool> {
    let output = run_git(worktree_path, &["status", "--porcelain"])?;
    Ok(!output.trim().is_empty())
}

// ── Helpers ──

/// Read the HEAD SHA for a worktree by reading its `.git` pointer file.
///
/// Returns `None` if the worktree doesn't exist or HEAD can't be resolved.
/// This avoids spawning a subprocess for a simple file read.
fn read_worktree_head(worktree_path: &Path) -> Option<String> {
    let git_path = worktree_path.join(".git");
    if !git_path.exists() {
        return None;
    }

    // If .git is a file (worktree pointer), read the gitdir path
    if git_path.is_file() {
        let content = std::fs::read_to_string(&git_path).ok()?;
        let gitdir = content.strip_prefix("gitdir:")?.trim();
        let gitdir_path = if Path::new(gitdir).is_relative() {
            worktree_path.join(gitdir)
        } else {
            PathBuf::from(gitdir)
        };
        let head_content = std::fs::read_to_string(gitdir_path.join("HEAD")).ok()?;
        return resolve_head_ref(&head_content, &gitdir_path);
    }

    // If .git is a directory (main worktree), read HEAD directly
    if git_path.is_dir() {
        let head_content = std::fs::read_to_string(git_path.join("HEAD")).ok()?;
        return resolve_head_ref(&head_content, &git_path);
    }

    None
}

/// Resolve a HEAD file content to a commit SHA.
fn resolve_head_ref(head_content: &str, git_dir: &Path) -> Option<String> {
    let trimmed = head_content.trim();
    if let Some(ref_path) = trimmed.strip_prefix("ref: ") {
        // Symbolic ref — resolve via the common dir or git dir
        let common_dir = git_dir.join("commondir");
        let base = if common_dir.exists() {
            let common = std::fs::read_to_string(&common_dir).ok()?;
            let common_path = common.trim();
            if Path::new(common_path).is_relative() {
                git_dir.join(common_path)
            } else {
                PathBuf::from(common_path)
            }
        } else {
            git_dir.to_path_buf()
        };
        let ref_file = base.join(ref_path);
        std::fs::read_to_string(ref_file)
            .ok()
            .map(|s| s.trim().to_string())
    } else if trimmed.len() >= 40 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        // Detached HEAD — SHA directly
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Run a git command and return its stdout.
fn run_git(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git: {e}"))?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|e| anyhow::anyhow!("git output is not valid UTF-8: {e}"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{}", stderr.trim())
    }
}

#[cfg(test)]
#[path = "worktree.test.rs"]
mod tests;
