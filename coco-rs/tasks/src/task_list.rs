//! Durable plan-item store backing the `TaskCreate`/`TaskGet`/
//! `TaskList`/`TaskUpdate` tools.
//!
//! **TS source**: `utils/tasks.ts`. Disk layout:
//!
//! ```text
//! {config_home}/tasks/{sanitize(list_id)}/
//!   ├── .lock             # file-lock sentinel (created on demand)
//!   ├── .highwatermark    # max task id ever assigned; prevents reuse
//!   ├── 1.json
//!   ├── 2.json
//!   └── ...
//! ```
//!
//! Locking: `fs2`-based exclusive advisory lock on `.lock` for list-
//! level ops (create / reset / agent-busy claim) and on `{id}.json`
//! for per-task updates / claims. 30-retry backoff matches the TS
//! `proper-lockfile` budget (~2.6s on a 10-way race).

use coco_types::HookEventType;
use fs2::FileExt;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use coco_tool::check_verification_nudge;

/// Task status — 3 variants, matching TS `TaskStatusSchema` in
/// `utils/tasks.ts:69-74`. **Not** the 6-variant `coco_types::TaskStatus`
/// which is for running background tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

/// A durable plan-item, matching TS `TaskSchema` (`utils/tasks.ts:76-89`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub subject: String,
    #[serde(default)]
    pub description: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "activeForm"
    )]
    pub active_form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub status: TaskStatus,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default, rename = "blockedBy")]
    pub blocked_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Partial update applied in [`TaskListStore::update_task`]. `Option`
/// fields are `None` to leave unchanged; `Some(None)` variants would
/// need explicit sentinels — we don't expose those yet since TS only
/// sets non-null values (null metadata keys are handled inline).
#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub subject: Option<String>,
    pub description: Option<String>,
    pub active_form: Option<String>,
    pub owner: Option<String>,
    pub status: Option<TaskStatus>,
    /// Merge keys into metadata. `null` JSON values delete a key.
    pub metadata_merge: Option<HashMap<String, serde_json::Value>>,
}

/// Outcome of a [`TaskListStore::claim_task`] call. Matches TS
/// `ClaimTaskResult`.
#[derive(Debug, Clone)]
pub enum ClaimResult {
    Success(Task),
    TaskNotFound,
    AlreadyClaimed(Task),
    AlreadyResolved(Task),
    Blocked {
        task: Task,
        blocked_by_tasks: Vec<String>,
    },
    AgentBusy {
        task: Task,
        busy_with_tasks: Vec<String>,
    },
}

const HIGH_WATER_MARK_FILE: &str = ".highwatermark";
const LOCK_FILE: &str = ".lock";
const LOCK_RETRIES: u32 = 30;
const LOCK_MIN_BACKOFF_MS: u64 = 5;
const LOCK_MAX_BACKOFF_MS: u64 = 100;

/// Resolve the task-list-id for the current process, matching TS
/// `getTaskListId()` precedence (`utils/tasks.ts:199-210`):
///
/// 1. `CLAUDE_CODE_TASK_LIST_ID` env (explicit override)
/// 2. In-process teammate's team name
/// 3. `CLAUDE_CODE_TEAM_NAME` env (process-based teammate)
/// 4. Leader team name (set via `TeamCreateTool`)
/// 5. Session id fallback
pub fn resolve_task_list_id(
    teammate_team: Option<&str>,
    leader_team: Option<&str>,
    session_id: &str,
) -> String {
    if let Ok(v) = std::env::var("CLAUDE_CODE_TASK_LIST_ID")
        && !v.is_empty()
    {
        return v;
    }
    if let Some(v) = teammate_team
        && !v.is_empty()
    {
        return v.to_string();
    }
    if let Ok(v) = std::env::var("CLAUDE_CODE_TEAM_NAME")
        && !v.is_empty()
    {
        return v;
    }
    if let Some(v) = leader_team
        && !v.is_empty()
    {
        return v.to_string();
    }
    session_id.to_string()
}

/// Sanitize a string for safe use as a filesystem path component.
/// Matches TS `sanitizePathComponent` (`utils/tasks.ts:217-219`).
pub fn sanitize_path_component(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Optional hook notification sink. Implemented by `coco-hooks` in the
/// app layer — kept as a trait here so `coco-tasks` stays dependency-
/// free from the hook system.
#[async_trait::async_trait]
pub trait TaskHookSink: Send + Sync {
    async fn fire_task_event(
        &self,
        event: HookEventType,
        task_id: &str,
        subject: &str,
        description: &str,
    );
}

/// Disk-backed task-list store.
///
/// A store is bound to a single `task_list_id` + `tasks_root` directory.
/// Use [`resolve_task_list_id`] at session bootstrap to pick the id and
/// construct one store; callers reuse the same `Arc<TaskListStore>` for
/// the whole session.
pub struct TaskListStore {
    tasks_dir: PathBuf,
    /// In-process change notifier (TS `notifyTasksUpdated`).
    change_tx: tokio::sync::broadcast::Sender<()>,
    hook_sink: RwLock<Option<Arc<dyn TaskHookSink>>>,
}

impl TaskListStore {
    /// Open (or create) the store directory for `task_list_id` under
    /// `tasks_root` (typically `{config_home}/tasks`).
    pub fn open(tasks_root: &Path, task_list_id: &str) -> anyhow::Result<Arc<Self>> {
        let tasks_dir = tasks_root.join(sanitize_path_component(task_list_id));
        fs::create_dir_all(&tasks_dir)?;
        let (change_tx, _rx) = tokio::sync::broadcast::channel(16);
        Ok(Arc::new(Self {
            tasks_dir,
            change_tx,
            hook_sink: RwLock::new(None),
        }))
    }

    /// Attach a hook notification sink. Called at bootstrap by the app
    /// layer after `coco-hooks` is constructed.
    pub async fn set_hook_sink(&self, sink: Arc<dyn TaskHookSink>) {
        *self.hook_sink.write().await = Some(sink);
    }

    /// Subscribe to "tasks changed" notifications (TS `onTasksUpdated`).
    pub fn subscribe_changes(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.change_tx.subscribe()
    }

    fn task_path(&self, task_id: &str) -> PathBuf {
        self.tasks_dir
            .join(format!("{}.json", sanitize_path_component(task_id)))
    }

    fn lock_path(&self) -> PathBuf {
        self.tasks_dir.join(LOCK_FILE)
    }

    fn hwm_path(&self) -> PathBuf {
        self.tasks_dir.join(HIGH_WATER_MARK_FILE)
    }

    fn notify(&self) {
        let _ = self.change_tx.send(());
    }

    async fn fire_hook(&self, event: HookEventType, task: &Task) {
        let sink = self.hook_sink.read().await.clone();
        if let Some(sink) = sink {
            sink.fire_task_event(event, &task.id, &task.subject, &task.description)
                .await;
        }
    }

    // ── lock primitives ───────────────────────────────────────────

    async fn with_list_lock<F, R>(&self, f: F) -> anyhow::Result<R>
    where
        F: FnOnce() -> anyhow::Result<R> + Send,
        R: Send,
    {
        let path = self.lock_path();
        ensure_lock_file(&path)?;
        acquire_with_retry(&path, f).await
    }

    async fn with_task_lock<F, R>(&self, task_id: &str, f: F) -> anyhow::Result<R>
    where
        F: FnOnce() -> anyhow::Result<R> + Send,
        R: Send,
    {
        let path = self.task_path(task_id);
        if !path.exists() {
            anyhow::bail!("task file not found: {}", path.display());
        }
        acquire_with_retry(&path, f).await
    }

    // ── primitive disk ops (no locking) ───────────────────────────

    fn read_task_unlocked(&self, task_id: &str) -> anyhow::Result<Option<Task>> {
        let path = self.task_path(task_id);
        match fs::read_to_string(&path) {
            Ok(s) => match serde_json::from_str::<Task>(&s) {
                Ok(t) => Ok(Some(t)),
                Err(e) => {
                    tracing::warn!(
                        task_id = %task_id,
                        error = %e,
                        "task file failed schema validation; ignoring",
                    );
                    Ok(None)
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn write_task_unlocked(&self, task: &Task) -> anyhow::Result<()> {
        let path = self.task_path(&task.id);
        let json = serde_json::to_string_pretty(task)?;
        fs::write(&path, json)?;
        Ok(())
    }

    fn list_tasks_unlocked(&self) -> anyhow::Result<Vec<Task>> {
        let mut out = Vec::new();
        let entries = match fs::read_dir(&self.tasks_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.ends_with(".json") || name_str.starts_with('.') {
                continue;
            }
            let id = name_str.trim_end_matches(".json").to_string();
            if let Ok(Some(task)) = self.read_task_unlocked(&id) {
                out.push(task);
            }
        }
        Ok(out)
    }

    fn read_hwm(&self) -> i64 {
        fs::read_to_string(self.hwm_path())
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(0)
    }

    fn write_hwm(&self, value: i64) -> anyhow::Result<()> {
        fs::write(self.hwm_path(), value.to_string())?;
        Ok(())
    }

    fn highest_id_unlocked(&self) -> i64 {
        let mut highest = 0_i64;
        if let Ok(entries) = fs::read_dir(&self.tasks_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if let Some(stem) = name_str.strip_suffix(".json")
                    && let Ok(id) = stem.parse::<i64>()
                    && id > highest
                {
                    highest = id;
                }
            }
        }
        std::cmp::max(highest, self.read_hwm())
    }

    // ── public API ────────────────────────────────────────────────

    /// Create a new task; returns the assigned id.
    ///
    /// Holds the list-level lock to serialize the "find highest id →
    /// write new file" transition against concurrent creators.
    pub async fn create_task(
        &self,
        subject: String,
        description: String,
        active_form: Option<String>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) -> anyhow::Result<Task> {
        let task = self
            .with_list_lock(|| {
                let highest = self.highest_id_unlocked();
                let id = (highest + 1).to_string();
                let task = Task {
                    id,
                    subject,
                    description,
                    active_form,
                    owner: None,
                    status: TaskStatus::Pending,
                    blocks: Vec::new(),
                    blocked_by: Vec::new(),
                    metadata,
                };
                self.write_task_unlocked(&task)?;
                Ok(task)
            })
            .await?;
        self.notify();
        self.fire_hook(HookEventType::TaskCreated, &task).await;
        Ok(task)
    }

    /// Delete a task and cascade removal of its id from other tasks'
    /// blocks / blockedBy arrays. Updates the high-water-mark so the
    /// id isn't reassigned later.
    pub async fn delete_task(&self, task_id: &str) -> anyhow::Result<bool> {
        let removed = self
            .with_list_lock(|| {
                // Bump HWM before deletion.
                if let Ok(numeric) = task_id.parse::<i64>() {
                    let current = self.read_hwm();
                    if numeric > current {
                        self.write_hwm(numeric)?;
                    }
                }

                let path = self.task_path(task_id);
                match fs::remove_file(&path) {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(false);
                    }
                    Err(e) => return Err(e.into()),
                }

                // Cascade: remove the id from siblings. We already hold
                // the list lock, so read + rewrite inline.
                let all = self.list_tasks_unlocked()?;
                for mut sibling in all {
                    let blocks_had = sibling.blocks.len();
                    let blocked_had = sibling.blocked_by.len();
                    sibling.blocks.retain(|b| b != task_id);
                    sibling.blocked_by.retain(|b| b != task_id);
                    if sibling.blocks.len() != blocks_had || sibling.blocked_by.len() != blocked_had
                    {
                        self.write_task_unlocked(&sibling)?;
                    }
                }
                Ok(true)
            })
            .await?;
        if removed {
            self.notify();
        }
        Ok(removed)
    }

    pub async fn get_task(&self, task_id: &str) -> anyhow::Result<Option<Task>> {
        // Reads do not need the lock — disk atomicity + serde validates.
        self.read_task_unlocked(task_id)
    }

    pub async fn list_tasks(&self) -> anyhow::Result<Vec<Task>> {
        self.list_tasks_unlocked()
    }

    /// Apply a partial update to a task under the task-level lock.
    /// Returns the updated task, or `None` if the task was missing.
    ///
    /// Fires `HookEventType::TaskCompleted` when `updates.status`
    /// transitions to `Completed`.
    pub async fn update_task(
        &self,
        task_id: &str,
        updates: TaskUpdate,
    ) -> anyhow::Result<Option<Task>> {
        if !self.task_path(task_id).exists() {
            return Ok(None);
        }
        let (updated, completed) = self
            .with_task_lock(task_id, || {
                let Some(mut task) = self.read_task_unlocked(task_id)? else {
                    return Ok::<(Option<Task>, bool), anyhow::Error>((None, false));
                };
                let mut newly_completed = false;
                if let Some(v) = updates.subject {
                    task.subject = v;
                }
                if let Some(v) = updates.description {
                    task.description = v;
                }
                if let Some(v) = updates.active_form {
                    task.active_form = Some(v);
                }
                if let Some(v) = updates.owner {
                    task.owner = Some(v);
                }
                if let Some(v) = updates.status {
                    if task.status != TaskStatus::Completed && v == TaskStatus::Completed {
                        newly_completed = true;
                    }
                    task.status = v;
                }
                if let Some(merge) = updates.metadata_merge {
                    let mut base = task.metadata.unwrap_or_default();
                    for (k, v) in merge {
                        if v.is_null() {
                            base.remove(&k);
                        } else {
                            base.insert(k, v);
                        }
                    }
                    task.metadata = if base.is_empty() { None } else { Some(base) };
                }
                self.write_task_unlocked(&task)?;
                Ok((Some(task), newly_completed))
            })
            .await?;

        if updated.is_some() {
            self.notify();
        }
        if completed && let Some(task) = &updated {
            self.fire_hook(HookEventType::TaskCompleted, task).await;
        }
        Ok(updated)
    }

    /// Add a `blocks` / `blockedBy` edge between two tasks.
    /// Returns `false` if either task is missing.
    pub async fn block_task(&self, from_id: &str, to_id: &str) -> anyhow::Result<bool> {
        let (Some(mut from), Some(mut to)) =
            (self.get_task(from_id).await?, self.get_task(to_id).await?)
        else {
            return Ok(false);
        };

        let mut changed = false;
        if !from.blocks.iter().any(|id| id == to_id) {
            from.blocks.push(to_id.to_string());
            self.with_task_lock(from_id, || self.write_task_unlocked(&from))
                .await?;
            changed = true;
        }
        if !to.blocked_by.iter().any(|id| id == from_id) {
            to.blocked_by.push(from_id.to_string());
            self.with_task_lock(to_id, || self.write_task_unlocked(&to))
                .await?;
            changed = true;
        }
        if changed {
            self.notify();
        }
        Ok(true)
    }

    /// Atomic claim (TS `claimTask`). `check_agent_busy=true` adds the
    /// "agent already owns another open task" guard under the list
    /// lock (TS `claimTaskWithBusyCheck`).
    pub async fn claim_task(
        &self,
        task_id: &str,
        claimant: &str,
        check_agent_busy: bool,
    ) -> anyhow::Result<ClaimResult> {
        // Early existence check outside the lock so missing ids return
        // cleanly (TS lock layer errors on missing files).
        let Some(_) = self.get_task(task_id).await? else {
            return Ok(ClaimResult::TaskNotFound);
        };

        if check_agent_busy {
            return self.claim_with_busy_check(task_id, claimant).await;
        }

        let outcome = self
            .with_task_lock(task_id, || {
                let Some(mut task) = self.read_task_unlocked(task_id)? else {
                    return Ok::<ClaimResult, anyhow::Error>(ClaimResult::TaskNotFound);
                };
                if let Some(owner) = &task.owner
                    && owner != claimant
                {
                    return Ok(ClaimResult::AlreadyClaimed(task));
                }
                if task.status == TaskStatus::Completed {
                    return Ok(ClaimResult::AlreadyResolved(task));
                }
                // Unresolved blockers are tasks with status != completed.
                let all = self.list_tasks_unlocked()?;
                let unresolved: HashSet<String> = all
                    .iter()
                    .filter(|t| t.status != TaskStatus::Completed)
                    .map(|t| t.id.clone())
                    .collect();
                let blocked_by: Vec<String> = task
                    .blocked_by
                    .iter()
                    .filter(|id| unresolved.contains(*id))
                    .cloned()
                    .collect();
                if !blocked_by.is_empty() {
                    return Ok(ClaimResult::Blocked {
                        task,
                        blocked_by_tasks: blocked_by,
                    });
                }
                task.owner = Some(claimant.to_string());
                self.write_task_unlocked(&task)?;
                Ok(ClaimResult::Success(task))
            })
            .await?;

        if matches!(outcome, ClaimResult::Success(_)) {
            self.notify();
        }
        Ok(outcome)
    }

    async fn claim_with_busy_check(
        &self,
        task_id: &str,
        claimant: &str,
    ) -> anyhow::Result<ClaimResult> {
        let outcome = self
            .with_list_lock(|| {
                let all = self.list_tasks_unlocked()?;
                let Some(mut task) = all.iter().find(|t| t.id == task_id).cloned() else {
                    return Ok::<ClaimResult, anyhow::Error>(ClaimResult::TaskNotFound);
                };
                if let Some(owner) = &task.owner
                    && owner != claimant
                {
                    return Ok(ClaimResult::AlreadyClaimed(task));
                }
                if task.status == TaskStatus::Completed {
                    return Ok(ClaimResult::AlreadyResolved(task));
                }
                let unresolved: HashSet<String> = all
                    .iter()
                    .filter(|t| t.status != TaskStatus::Completed)
                    .map(|t| t.id.clone())
                    .collect();
                let blocked_by: Vec<String> = task
                    .blocked_by
                    .iter()
                    .filter(|id| unresolved.contains(*id))
                    .cloned()
                    .collect();
                if !blocked_by.is_empty() {
                    return Ok(ClaimResult::Blocked {
                        task,
                        blocked_by_tasks: blocked_by,
                    });
                }
                let busy_with: Vec<String> = all
                    .iter()
                    .filter(|t| {
                        t.status != TaskStatus::Completed
                            && t.owner.as_deref() == Some(claimant)
                            && t.id != task_id
                    })
                    .map(|t| t.id.clone())
                    .collect();
                if !busy_with.is_empty() {
                    return Ok(ClaimResult::AgentBusy {
                        task,
                        busy_with_tasks: busy_with,
                    });
                }
                task.owner = Some(claimant.to_string());
                self.write_task_unlocked(&task)?;
                Ok(ClaimResult::Success(task))
            })
            .await?;

        if matches!(outcome, ClaimResult::Success(_)) {
            self.notify();
        }
        Ok(outcome)
    }

    /// Should the model receive a verification-agent nudge after this
    /// update? Main-thread only, all tasks completed, ≥3 tasks, none
    /// match `/verif/i`. Subjects are scanned in TS (`TaskUpdateTool.ts:345`).
    pub async fn should_nudge_verification_after_update(
        &self,
        just_completed: bool,
        is_main_thread: bool,
    ) -> bool {
        if !is_main_thread || !just_completed {
            return false;
        }
        let tasks = match self.list_tasks_unlocked() {
            Ok(t) => t,
            Err(_) => return false,
        };
        let subjects: Vec<&str> = tasks.iter().map(|t| t.subject.as_str()).collect();
        let all_done = !tasks.is_empty() && tasks.iter().all(|t| t.status == TaskStatus::Completed);
        all_done && check_verification_nudge(&subjects)
    }

    /// Unassign all unresolved tasks owned by a teammate. Returns the
    /// list of `(id, subject)` pairs that were unassigned.
    ///
    /// TS `unassignTeammateTasks` (`utils/tasks.ts:818-860`). We bypass
    /// `update_task` and write directly under the per-task lock because
    /// `TaskUpdate.owner: Option<String>` only expresses "set to Some",
    /// not "clear to None" — TS `updateTask({owner: undefined})` clears
    /// the field. Writing the file directly avoids needing to add a
    /// sentinel variant just for this one caller.
    pub async fn unassign_teammate_tasks(
        &self,
        teammate_id: &str,
        teammate_name: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        let all = self.list_tasks_unlocked()?;
        let mut unassigned = Vec::new();
        for task in all {
            let owned = task.status != TaskStatus::Completed
                && task
                    .owner
                    .as_deref()
                    .is_some_and(|o| o == teammate_id || o == teammate_name);
            if !owned {
                continue;
            }
            self.with_task_lock(&task.id, || {
                if let Some(mut t) = self.read_task_unlocked(&task.id)? {
                    t.owner = None;
                    t.status = TaskStatus::Pending;
                    self.write_task_unlocked(&t)?;
                }
                Ok(())
            })
            .await?;
            unassigned.push((task.id, task.subject));
        }
        if !unassigned.is_empty() {
            self.notify();
        }
        Ok(unassigned)
    }
}

/// Ensure the list-level lock file exists so `fs2` can lock it.
fn ensure_lock_file(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?
        .flush()?;
    Ok(())
}

/// Acquire an exclusive `fs2` lock with backoff and run the closure.
/// Retries on `WouldBlock` up to [`LOCK_RETRIES`] times.
async fn acquire_with_retry<F, R>(path: &Path, f: F) -> anyhow::Result<R>
where
    F: FnOnce() -> anyhow::Result<R> + Send,
    R: Send,
{
    let mut attempt = 0;
    let mut backoff = LOCK_MIN_BACKOFF_MS;
    loop {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        match file.try_lock_exclusive() {
            Ok(()) => {
                let result = f();
                let _ = FileExt::unlock(&file);
                return result;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if attempt >= LOCK_RETRIES {
                    return Err(anyhow::anyhow!(
                        "failed to acquire lock on {} after {} attempts",
                        path.display(),
                        LOCK_RETRIES
                    ));
                }
                tokio::time::sleep(Duration::from_millis(backoff)).await;
                attempt += 1;
                backoff = std::cmp::min(backoff * 2, LOCK_MAX_BACKOFF_MS);
            }
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
#[path = "task_list.test.rs"]
mod tests;
