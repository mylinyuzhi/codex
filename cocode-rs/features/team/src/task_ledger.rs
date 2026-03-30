//! Team-specific task ledger with atomic claiming and dependency tracking.
//!
//! Aligned with Claude Code's task ledger system: agents create tasks with
//! `blockedBy` dependency arrays, claim tasks atomically, and completion
//! cascades automatically unblock dependents.
//!
//! Storage: `{base_dir}/{team_name}/tasks.json`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;
use snafu::ResultExt;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::error::team_error;

// ============================================================================
// Types
// ============================================================================

/// Status of a team task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TeamTaskStatus {
    /// Not yet claimed.
    #[default]
    Pending,
    /// Claimed and actively being worked on.
    InProgress,
    /// Work completed.
    Completed,
}

impl TeamTaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

impl std::fmt::Display for TeamTaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A task in the team's shared task ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTask {
    /// Unique task identifier (sequential per team).
    pub id: String,
    /// Brief title of the task.
    pub subject: String,
    /// Detailed description.
    #[serde(default)]
    pub description: String,
    /// Agent ID that claimed this task (None = unclaimed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Current status.
    #[serde(default)]
    pub status: TeamTaskStatus,
    /// Task IDs that this task blocks (forward edges).
    #[serde(default)]
    pub blocks: Vec<String>,
    /// Task IDs that must complete before this task can be claimed.
    #[serde(default)]
    pub blocked_by: Vec<String>,
    /// Arbitrary metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Creation timestamp (Unix seconds).
    pub created_at: i64,
    /// Last update timestamp (Unix seconds).
    pub updated_at: i64,
}

/// Result of attempting to claim a task.
#[derive(Debug, Clone)]
pub enum ClaimResult {
    /// Task successfully claimed.
    Claimed(TeamTask),
    /// Task already claimed by another agent.
    AlreadyClaimed { by: String },
    /// Task is blocked by unfinished dependencies.
    Blocked { blocked_by: Vec<String> },
    /// Task not found.
    NotFound,
}

// ============================================================================
// Ledger state per team
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LedgerState {
    tasks: Vec<TeamTask>,
    next_id: i64,
}

// ============================================================================
// TaskLedger
// ============================================================================

/// Team-specific task ledger with atomic claiming and dependencies.
///
/// Each team gets its own ledger stored at `{base_dir}/{team_name}/tasks.json`.
/// All operations are atomic via `Mutex` and filesystem writes use temp+rename.
#[derive(Debug, Clone)]
pub struct TaskLedger {
    base_dir: PathBuf,
    /// Per-team ledger state.
    state: Arc<Mutex<HashMap<String, LedgerState>>>,
    persist: bool,
}

impl TaskLedger {
    pub fn new(base_dir: PathBuf, persist: bool) -> Self {
        Self {
            base_dir,
            state: Arc::new(Mutex::new(HashMap::new())),
            persist,
        }
    }

    /// Load existing ledgers from disk.
    pub async fn load_from_disk(&self) -> Result<()> {
        if !self.persist || !self.base_dir.exists() {
            return Ok(());
        }
        let mut entries =
            tokio::fs::read_dir(&self.base_dir)
                .await
                .context(team_error::PersistSnafu {
                    message: format!("reading teams dir: {}", self.base_dir.display()),
                })?;

        let mut state = self.state.lock().await;
        while let Some(entry) = entries
            .next_entry()
            .await
            .context(team_error::PersistSnafu {
                message: "iterating teams dir for task ledger",
            })?
        {
            let tasks_path = entry.path().join("tasks.json");
            if let Ok(content) = tokio::fs::read_to_string(&tasks_path).await {
                match serde_json::from_str::<LedgerState>(&content) {
                    Ok(ledger) => {
                        if let Some(name) = entry.file_name().to_str() {
                            state.insert(name.to_string(), ledger);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %tasks_path.display(),
                            error = %e,
                            "Skipping invalid task ledger"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Create a new task in the team's ledger.
    pub async fn create_task(
        &self,
        team_name: &str,
        subject: impl Into<String>,
        description: impl Into<String>,
        blocked_by: Vec<String>,
    ) -> Result<TeamTask> {
        let now = unix_now();
        let mut all = self.state.lock().await;
        let ledger = all.entry(team_name.to_string()).or_default();

        ledger.next_id += 1;
        let id = ledger.next_id.to_string();

        // Wire bidirectional dependency edges before creating the task.
        for blocker_id in &blocked_by {
            if let Some(blocker) = ledger.tasks.iter_mut().find(|t| t.id == *blocker_id)
                && !blocker.blocks.contains(&id)
            {
                blocker.blocks.push(id.clone());
                blocker.updated_at = now;
            }
        }

        // Validate blocked_by references (keep only existing task IDs).
        let valid_blocked_by: Vec<String> = blocked_by
            .into_iter()
            .filter(|bid| ledger.tasks.iter().any(|t| t.id == *bid))
            .collect();

        let task = TeamTask {
            id: id.clone(),
            subject: subject.into(),
            description: description.into(),
            owner: None,
            status: TeamTaskStatus::Pending,
            blocks: Vec::new(),
            blocked_by: valid_blocked_by,
            metadata: None,
            created_at: now,
            updated_at: now,
        };

        ledger.tasks.push(task.clone());

        drop(all);
        self.persist_ledger(team_name).await?;
        Ok(task)
    }

    /// Atomically claim a task for an agent.
    ///
    /// Returns [`ClaimResult`] indicating success or why claiming failed.
    pub async fn claim_task(
        &self,
        team_name: &str,
        task_id: &str,
        agent_id: &str,
    ) -> Result<ClaimResult> {
        let now = unix_now();
        let mut all = self.state.lock().await;
        let ledger = match all.get_mut(team_name) {
            Some(l) => l,
            None => return Ok(ClaimResult::NotFound),
        };

        // Find task index first, then do all checks with shared refs.
        let task_idx = match ledger.tasks.iter().position(|t| t.id == task_id) {
            Some(i) => i,
            None => return Ok(ClaimResult::NotFound),
        };

        // Only pending tasks can be claimed.
        if ledger.tasks[task_idx].status != TeamTaskStatus::Pending {
            let by = ledger.tasks[task_idx]
                .owner
                .clone()
                .unwrap_or_else(|| ledger.tasks[task_idx].status.to_string());
            return Ok(ClaimResult::AlreadyClaimed { by });
        }

        // Check blockedBy: only non-completed blockers count.
        let active_blockers: Vec<String> = ledger.tasks[task_idx]
            .blocked_by
            .iter()
            .filter(|bid| {
                ledger
                    .tasks
                    .iter()
                    .any(|t| t.id == **bid && t.status != TeamTaskStatus::Completed)
            })
            .cloned()
            .collect();

        if !active_blockers.is_empty() {
            return Ok(ClaimResult::Blocked {
                blocked_by: active_blockers,
            });
        }

        // Claim it (now safe to mutate).
        let task = &mut ledger.tasks[task_idx];
        task.owner = Some(agent_id.to_string());
        task.status = TeamTaskStatus::InProgress;
        task.updated_at = now;

        let claimed = task.clone();
        drop(all);
        self.persist_ledger(team_name).await?;
        Ok(ClaimResult::Claimed(claimed))
    }

    /// Complete a task and cascade-unblock dependents.
    ///
    /// When a task is completed, all tasks in its `blocks` list that have
    /// no other active blockers become claimable.
    pub async fn complete_task(&self, team_name: &str, task_id: &str) -> Result<()> {
        let now = unix_now();
        let mut all = self.state.lock().await;
        let ledger = match all.get_mut(team_name) {
            Some(l) => l,
            None => return Err(team_error::TeamNotFoundSnafu { name: team_name }.build()),
        };

        let task = ledger
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| team_error::TaskNotFoundSnafu { id: task_id }.build())?;

        task.status = TeamTaskStatus::Completed;
        task.updated_at = now;

        // Cascade: remove this task from all blocked_by lists.
        let tid = task_id.to_string();
        for t in &mut ledger.tasks {
            if t.blocked_by.contains(&tid) {
                t.blocked_by.retain(|b| b != &tid);
                t.updated_at = now;
            }
        }

        drop(all);
        self.persist_ledger(team_name).await
    }

    /// Add a dependency: `blocker_id` must complete before `blocked_id` can start.
    ///
    /// Both tasks must exist, otherwise returns [`TeamError::TaskNotFound`].
    pub async fn add_dependency(
        &self,
        team_name: &str,
        blocker_id: &str,
        blocked_id: &str,
    ) -> Result<()> {
        let now = unix_now();
        let mut all = self.state.lock().await;
        let ledger = match all.get_mut(team_name) {
            Some(l) => l,
            None => return Err(team_error::TeamNotFoundSnafu { name: team_name }.build()),
        };

        // Validate both tasks exist.
        if !ledger.tasks.iter().any(|t| t.id == blocker_id) {
            return Err(team_error::TaskNotFoundSnafu { id: blocker_id }.build());
        }
        if !ledger.tasks.iter().any(|t| t.id == blocked_id) {
            return Err(team_error::TaskNotFoundSnafu { id: blocked_id }.build());
        }

        // Forward edge: blocker.blocks → blocked_id
        if let Some(blocker) = ledger.tasks.iter_mut().find(|t| t.id == blocker_id)
            && !blocker.blocks.contains(&blocked_id.to_string())
        {
            blocker.blocks.push(blocked_id.to_string());
            blocker.updated_at = now;
        }
        // Reverse edge: blocked.blocked_by → blocker_id
        if let Some(blocked) = ledger.tasks.iter_mut().find(|t| t.id == blocked_id)
            && !blocked.blocked_by.contains(&blocker_id.to_string())
        {
            blocked.blocked_by.push(blocker_id.to_string());
            blocked.updated_at = now;
        }

        drop(all);
        self.persist_ledger(team_name).await
    }

    /// Find the next claimable task (first unblocked pending task).
    ///
    /// This is the task selection algorithm matching CC's `d6Y()`.
    pub async fn next_claimable(&self, team_name: &str) -> Option<TeamTask> {
        let all = self.state.lock().await;
        let ledger = all.get(team_name)?;

        let pending: Vec<&TeamTask> = ledger
            .tasks
            .iter()
            .filter(|t| t.status == TeamTaskStatus::Pending)
            .collect();

        if pending.is_empty() {
            return None;
        }

        // Non-completed task IDs for blocker check.
        let active_ids: std::collections::HashSet<&str> = ledger
            .tasks
            .iter()
            .filter(|t| t.status != TeamTaskStatus::Completed)
            .map(|t| t.id.as_str())
            .collect();

        // First pending task with no active blockers. Returns None if all
        // pending tasks are blocked — never returns a blocked task.
        pending
            .into_iter()
            .find(|t| !t.blocked_by.iter().any(|b| active_ids.contains(b.as_str())))
            .cloned()
    }

    /// Reassign all tasks owned by an agent back to pending.
    ///
    /// Called when an agent shuts down or is removed from the team.
    /// Returns the IDs of unassigned tasks.
    pub async fn reassign_agent_tasks(
        &self,
        team_name: &str,
        agent_id: &str,
    ) -> Result<Vec<String>> {
        let now = unix_now();
        let mut all = self.state.lock().await;
        let ledger = match all.get_mut(team_name) {
            Some(l) => l,
            None => return Ok(Vec::new()),
        };

        let mut unassigned = Vec::new();
        for task in &mut ledger.tasks {
            if task.owner.as_deref() == Some(agent_id) && task.status == TeamTaskStatus::InProgress
            {
                task.owner = None;
                task.status = TeamTaskStatus::Pending;
                task.updated_at = now;
                unassigned.push(task.id.clone());
            }
        }

        if !unassigned.is_empty() {
            drop(all);
            self.persist_ledger(team_name).await?;
        }
        Ok(unassigned)
    }

    /// List all tasks for a team.
    pub async fn list_tasks(&self, team_name: &str) -> Vec<TeamTask> {
        let all = self.state.lock().await;
        all.get(team_name)
            .map(|l| l.tasks.clone())
            .unwrap_or_default()
    }

    /// Get a specific task.
    pub async fn get_task(&self, team_name: &str, task_id: &str) -> Option<TeamTask> {
        let all = self.state.lock().await;
        all.get(team_name)
            .and_then(|l| l.tasks.iter().find(|t| t.id == task_id).cloned())
    }

    /// Delete a completed task and clean up dependency references.
    pub async fn delete_task(&self, team_name: &str, task_id: &str) -> Result<()> {
        let now = unix_now();
        let mut all = self.state.lock().await;
        let ledger = match all.get_mut(team_name) {
            Some(l) => l,
            None => return Ok(()),
        };

        let tid = task_id.to_string();
        ledger.tasks.retain(|t| t.id != tid);

        // Clean references from other tasks.
        for t in &mut ledger.tasks {
            let changed = t.blocks.contains(&tid) || t.blocked_by.contains(&tid);
            t.blocks.retain(|b| b != &tid);
            t.blocked_by.retain(|b| b != &tid);
            if changed {
                t.updated_at = now;
            }
        }

        drop(all);
        self.persist_ledger(team_name).await
    }

    /// Delete all tasks for a team (used on team deletion).
    pub async fn clear_team(&self, team_name: &str) {
        self.state.lock().await.remove(team_name);
        if self.persist {
            let path = self.base_dir.join(team_name).join("tasks.json");
            let _ = tokio::fs::remove_file(&path).await;
        }
    }

    /// Persist a single team's ledger to disk.
    async fn persist_ledger(&self, team_name: &str) -> Result<()> {
        if !self.persist {
            return Ok(());
        }
        let ledger = {
            let all = self.state.lock().await;
            match all.get(team_name) {
                Some(l) => l.clone(),
                None => return Ok(()),
            }
        };

        let dir = self.base_dir.join(team_name);
        tokio::fs::create_dir_all(&dir)
            .await
            .context(team_error::PersistSnafu {
                message: format!("creating team dir: {}", dir.display()),
            })?;

        let path = dir.join("tasks.json");
        let json = serde_json::to_string_pretty(&ledger).context(team_error::SerdeSnafu {
            message: "serializing task ledger",
        })?;

        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, json.as_bytes())
            .await
            .context(team_error::PersistSnafu {
                message: format!("writing temp: {}", tmp.display()),
            })?;
        tokio::fs::rename(&tmp, &path)
            .await
            .context(team_error::PersistSnafu {
                message: format!("renaming to: {}", path.display()),
            })?;
        Ok(())
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "task_ledger.test.rs"]
mod tests;
