//! Path helpers shared by binary subcommand handlers and library
//! bootstrap code.
//!
//! Centralizes path construction that was previously duplicated across
//! `main.rs`, `tui_runner.rs`, and `run_sdk_mode`: the sessions
//! directory, the agent search paths, and the output-style directories.

use std::path::Path;
use std::path::PathBuf;

use coco_config::global_config;

/// `~/.coco/sessions` — disk root for `SessionManager`.
pub fn sessions_dir() -> PathBuf {
    global_config::config_home().join("sessions")
}

/// `~/.coco/output-styles` — user-scope output style markdown dir.
///
/// TS-parity: TS reads `~/.claude/output-styles`; coco-rs uses
/// `~/.coco/output-styles` to match the rest of the namespace
/// (`~/.coco/skills`, `~/.coco/agents`). [`OutputStyleManagerBuilder`]
/// also honors managed and project sources — see [`output_style_dirs`].
///
/// [`OutputStyleManagerBuilder`]: coco_output_styles::manager::OutputStyleManagerBuilder
/// [`output_style_dirs`]: self::output_style_dirs
pub fn user_output_style_dir() -> PathBuf {
    global_config::config_home().join("output-styles")
}

/// `<cwd>/.claude/output-styles` — single project output style dir.
///
/// TS additionally walks every ancestor up to the git root (see
/// `getProjectDirsUpToHome` in TS `markdownConfigLoader.ts`); coco-rs
/// follows the simpler cwd-direct convention used by skills and agents
/// (`get_skill_paths` / `standard_agent_search_paths`). Sessions are
/// explicitly bound to a single cwd, so deeply-nested traversal isn't
/// load-bearing in coco-rs and would surprise users who expect
/// `<cwd>/.claude/...` to be the only project-style root.
pub fn project_output_style_dir(cwd: &Path) -> PathBuf {
    cwd.join(".claude").join("output-styles")
}

/// Cross-platform managed/policy directory for output styles. Mirrors
/// [`coco_skills::get_managed_skills_path`] but for `output-styles`.
///
/// TS reads from `getManagedFilePath()/.claude/output-styles`;
/// coco-rs hardcodes the canonical platform paths so admins can drop
/// markdown in there without setting an env var.
pub fn managed_output_style_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/ClaudeCode/.claude/output-styles")
    }
    #[cfg(not(target_os = "macos"))]
    {
        PathBuf::from("/etc/claude-code/.claude/output-styles")
    }
}

/// Directory list for output styles in TS-parity priority order
/// (lowest to highest): user → project → managed.
///
/// Returned for the SDK `available_output_styles` `discover_*` legacy
/// path; new code prefers
/// [`coco_output_styles::OutputStyleManager::builder`] which accepts
/// each source separately so priority is enforced explicitly.
pub fn output_style_dirs(cwd: &Path) -> Vec<PathBuf> {
    vec![
        user_output_style_dir(),
        project_output_style_dir(cwd),
        managed_output_style_dir(),
    ]
}

/// Standard CLI agent search paths: `~/.coco/agents` (user) plus
/// `<cwd>/.claude/agents` (project). Mirrors TS `agentDirs` from
/// `tools/AgentTool/loadAgentsDir.ts` discovery roots and the legacy
/// `agent_spawn::get_agent_dirs` shape we replaced.
///
/// **Worktree fallback** (TS parity:
/// `utils/markdownConfigLoader.ts:307-330`): when `cwd` resolves into
/// a linked git worktree whose `.claude/agents/` is empty (or not
/// checked out), we additionally search the canonical (main) repo's
/// `.claude/agents/`. The fallback only fires when the canonical root
/// differs from the worktree's git root **and** the worktree dir is
/// missing — a `git worktree add` checks out the full tree, so the
/// shared case (worktree already has the same agent files) skips the
/// fallback to keep precedence stable.
pub fn standard_agent_search_paths(
    config_home: &Path,
    cwd: &Path,
) -> coco_subagent::definition_store::AgentSearchPaths {
    let mut project_dirs = vec![cwd.join(".claude").join("agents")];

    // Push the canonical-repo fallback when applicable. Errors / no-git
    // states are treated as "no fallback needed" — the loader degrades
    // gracefully on missing dirs.
    if let Some(canonical_root) = coco_git::find_canonical_git_root(cwd) {
        let worktree_agents_dir = cwd.join(".claude").join("agents");
        let worktree_root = git_root_for(cwd);
        let worktree_has_agents = std::fs::metadata(&worktree_agents_dir)
            .map(|m| m.is_dir())
            .unwrap_or(false);
        let canonical_agents_dir = canonical_root.join(".claude").join("agents");
        // Only add the canonical-root copy when:
        // 1. cwd is inside a worktree distinct from the canonical root, AND
        // 2. the worktree's own .claude/agents/ is missing or empty.
        // Same-root cases (cwd == canonical_root, or worktree already
        // has agent files) keep the original single-entry shape.
        if worktree_root.as_deref() != Some(canonical_root.as_path())
            && !worktree_has_agents
            && canonical_agents_dir.is_dir()
            && !project_dirs.iter().any(|p| p == &canonical_agents_dir)
        {
            project_dirs.push(canonical_agents_dir);
        }
    }

    coco_subagent::definition_store::AgentSearchPaths {
        user_dir: Some(config_home.join("agents")),
        project_dirs,
        ..coco_subagent::definition_store::AgentSearchPaths::empty()
    }
}

/// Resolve the worktree's own git root (the directory containing the
/// `.git` file or directory) starting at `cwd`. Returns `None` if `cwd`
/// is not inside any git tree. This is distinct from
/// [`coco_git::find_canonical_git_root`], which collapses worktrees
/// onto the main repo via `--git-common-dir`.
fn git_root_for(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}
