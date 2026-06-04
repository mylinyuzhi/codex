//! Team file I/O — disk persistence for team configuration.
//!
//! TS: utils/swarm/teamHelpers.ts — readTeamFile, writeTeamFileAsync, getTeamDir.
//!
//! File layout:
//! ```text
//! ~/.claude/teams/
//!   {team-name}/
//!     config.json       # TeamFile
//!     inboxes/
//!       {agent-name}.json  # TeammateMessage[]
//!     permissions/
//!       pending/
//!       resolved/
//! ```

use std::path::PathBuf;

use crate::types::TeamFile;

/// Base directory for all teams.
///
/// TS: `~/.claude/teams/`
pub fn teams_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("teams")
}

/// Get the team directory for a given team name.
///
/// TS: `getTeamDir(teamName)` → `~/.claude/teams/{sanitized-name}/`
pub fn get_team_dir(team_name: &str) -> PathBuf {
    teams_base_dir().join(crate::types::sanitize_name(team_name))
}

/// Get the team file path for a given team name.
///
/// TS: `getTeamFilePath(teamName)` → `~/.claude/teams/{name}/config.json`
pub fn get_team_file_path(team_name: &str) -> PathBuf {
    get_team_dir(team_name).join("config.json")
}

/// Read a team file from disk.
///
/// TS: `readTeamFile(teamName)`
pub fn read_team_file(team_name: &str) -> crate::Result<Option<TeamFile>> {
    let path = get_team_file_path(team_name);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let team_file: TeamFile = serde_json::from_str(&content)?;
    Ok(Some(team_file))
}

/// Write a team file to disk.
///
/// TS: `writeTeamFileAsync(teamName, teamFile)`
pub fn write_team_file(team_name: &str, team_file: &TeamFile) -> crate::Result<()> {
    let path = get_team_file_path(team_name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(team_file)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// Remove a teammate from the team file by agent ID.
///
/// TS: `removeMemberByAgentId(teamName, agentId)`
pub(crate) fn remove_member_by_agent_id(team_name: &str, agent_id: &str) -> crate::Result<bool> {
    let Some(mut team_file) = read_team_file(team_name)? else {
        return Ok(false);
    };
    let before = team_file.members.len();
    team_file.members.retain(|m| m.agent_id != agent_id);
    if team_file.members.len() < before {
        write_team_file(team_name, &team_file)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// List all known team names (from the teams directory).
pub fn list_team_names() -> Vec<String> {
    let base = teams_base_dir();
    if !base.is_dir() {
        return Vec::new();
    }
    let entries = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // Verify config.json exists
            if e.path().join("config.json").exists() {
                Some(name)
            } else {
                None
            }
        })
        .collect()
}

/// Clean up everything a team owns: per-member worktrees, the team dir,
/// and the team's task-list directory.
///
/// TS: `cleanupTeamDirectories(teamName)` (`teamHelpers.ts:641-683`) —
/// destroy each `member.worktreePath`, then `rm` the team dir, then `rm`
/// `getTasksDir(sanitizedName)`. The worktree + tasks-dir steps are
/// best-effort (TS `Promise.allSettled`): a failure there is logged and
/// must not abort the team-dir removal.
pub fn cleanup_team_directories(team_name: &str) -> crate::Result<()> {
    // 1. Destroy each member's git worktree. Read the team file BEFORE the
    //    dir is removed below; skip members without an isolated worktree.
    if let Ok(Some(team_file)) = read_team_file(team_name) {
        for member in &team_file.members {
            if let Some(worktree_path) = &member.worktree_path
                && let Err(e) = destroy_worktree(worktree_path)
            {
                tracing::warn!(
                    member = %member.name,
                    worktree = %worktree_path,
                    error = %e,
                    "team cleanup: failed to destroy worktree (continuing)"
                );
            }
        }
    }

    // 2. Remove the team dir and its contents.
    let dir = get_team_dir(team_name);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir)?;
    }

    // 3. Remove the team's task-list directory, which would otherwise be
    //    orphaned. The store's `task_list_id` is `sanitize_name(team_name)`
    //    (roster_store) and lives at `{config_home}/tasks/{sanitized}`
    //    (`TaskList::open`). Best-effort so a tasks-dir failure never blocks
    //    a delete.
    let task_list_id = crate::types::sanitize_name(team_name);
    let tasks_dir = coco_config::global_config::config_home()
        .join("tasks")
        .join(coco_tasks::task_list::sanitize_path_component(
            &task_list_id,
        ));
    if tasks_dir.is_dir()
        && let Err(e) = std::fs::remove_dir_all(&tasks_dir)
    {
        tracing::warn!(
            dir = %tasks_dir.display(),
            error = %e,
            "team cleanup: failed to remove task-list dir (continuing)"
        );
    }

    Ok(())
}

/// Clean up teams owned by the current session.
///
/// TS: `cleanupSessionTeams()`
pub fn cleanup_session_teams(session_id: &str) -> crate::Result<()> {
    for name in list_team_names() {
        if let Ok(Some(tf)) = read_team_file(&name)
            && tf.lead_session_id.as_deref() == Some(session_id)
        {
            cleanup_team_directories(&name)?;
        }
    }
    Ok(())
}

/// Destroy a git worktree.
///
/// TS: `destroyWorktree(worktreePath)` — runs `git worktree remove`, falls back to rm.
pub fn destroy_worktree(worktree_path: &str) -> crate::Result<()> {
    let path = std::path::Path::new(worktree_path);
    if !path.exists() {
        return Ok(());
    }
    // Try git worktree remove first
    let status = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", worktree_path])
        .status();
    if status.is_ok_and(|s| s.success()) {
        return Ok(());
    }
    // Fallback: remove directory
    std::fs::remove_dir_all(path)?;
    Ok(())
}

/// Registry of teams owned by the current session (for cleanup on exit).
static SESSION_TEAMS: std::sync::RwLock<Option<Vec<String>>> = std::sync::RwLock::new(None);

/// Register a team for cleanup when the session ends.
///
/// TS: `registerTeamForSessionCleanup(teamName)`
pub fn register_team_for_session_cleanup(team_name: &str) {
    let mut guard = SESSION_TEAMS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let teams = guard.get_or_insert_with(Vec::new);
    if !teams.contains(&team_name.to_string()) {
        teams.push(team_name.to_string());
    }
}

/// Unregister a team from session cleanup.
///
/// TS: `unregisterTeamForSessionCleanup(teamName)`
pub fn unregister_team_for_session_cleanup(team_name: &str) {
    let mut guard = SESSION_TEAMS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(teams) = guard.as_mut() {
        teams.retain(|t| t != team_name);
    }
}

/// Get all teams registered for session cleanup.
pub fn get_session_cleanup_teams() -> Vec<String> {
    SESSION_TEAMS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .unwrap_or_default()
}

/// Kill all pane-based teammates for a team.
///
/// TS: `killOrphanedTeammatePanes(teamName)`
pub fn kill_orphaned_teammate_panes(team_name: &str) -> crate::Result<()> {
    let Some(team_file) = read_team_file(team_name)? else {
        return Ok(());
    };
    for member in &team_file.members {
        if !member.tmux_pane_id.is_empty() {
            // Best-effort kill
            let _ = std::process::Command::new("tmux")
                .args(["kill-pane", "-t", &member.tmux_pane_id])
                .status();
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "team_file.test.rs"]
mod tests;
