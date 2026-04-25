//! Persistent task-list + per-agent todo-list handle traits.
//!
//! Follows the same callback-decoupling pattern as
//! [`crate::agent_handle::AgentHandle`], [`crate::mailbox_handle::MailboxHandle`],
//! etc. The concrete implementations wrap `coco_tasks::TaskListStore` +
//! `coco_tasks::TodoStore` in the app layer; `coco-tool` stays dep-free.
//!
//! Data types (`TaskRecord`, `TodoRecord`, `TaskListStatus`, etc.) live
//! in `coco-types` so `ToolAppState` can carry typed snapshots without
//! a reverse dep into `coco-tool`. We re-export here for discoverability.

pub use coco_types::ExpandedView;
pub use coco_types::TaskClaimOutcome;
pub use coco_types::TaskListStatus;
pub use coco_types::TaskRecord;
pub use coco_types::TaskRecordUpdate;
pub use coco_types::TodoRecord;

use std::collections::HashMap;
use std::sync::Arc;

/// Shared verification-nudge gate used by both V1 `TodoWrite` and V2
/// `TaskUpdate`. TS source:
/// - V1 `TodoWriteTool.ts:77-85` — runs against `TodoItem.content`
/// - V2 `TaskUpdateTool.ts:334-349` — runs against `Task.subject`
///
/// Returns `true` when the model should be nudged to spawn a
/// verification agent: ≥3 items, all supposedly finished, none matches
/// `/verif/i`. Callers pre-check "main-thread" and "all-done" so this
/// function stays pure.
pub fn check_verification_nudge(items: &[&str]) -> bool {
    if items.len() < 3 {
        return false;
    }
    !items
        .iter()
        .any(|s| s.to_ascii_lowercase().contains("verif"))
}

/// Access to the durable, disk-backed plan-item store.
///
/// **Single store per session**, shared across tools + subagents +
/// teammates. Implementations serialize cross-thread access via file
/// locks (`.lock` + per-task lock), matching TS `proper-lockfile`
/// semantics in `utils/tasks.ts`.
#[async_trait::async_trait]
pub trait TaskListHandle: Send + Sync {
    async fn create_task(
        &self,
        subject: String,
        description: String,
        active_form: Option<String>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) -> anyhow::Result<TaskRecord>;

    async fn get_task(&self, task_id: &str) -> anyhow::Result<Option<TaskRecord>>;

    async fn list_tasks(&self) -> anyhow::Result<Vec<TaskRecord>>;

    async fn update_task(
        &self,
        task_id: &str,
        updates: TaskRecordUpdate,
    ) -> anyhow::Result<Option<TaskRecord>>;

    async fn delete_task(&self, task_id: &str) -> anyhow::Result<bool>;

    async fn block_task(&self, from_id: &str, to_id: &str) -> anyhow::Result<bool>;

    async fn claim_task(
        &self,
        task_id: &str,
        claimant: &str,
        check_agent_busy: bool,
    ) -> anyhow::Result<TaskClaimOutcome>;

    /// Should we emit a verification-agent nudge after this update?
    /// Implementations check "main-thread, all completed, ≥3 tasks,
    /// none match `/verif/i`".
    async fn should_nudge_verification(&self, just_completed: bool, is_main_thread: bool) -> bool;
}

pub type TaskListHandleRef = Arc<dyn TaskListHandle>;

/// In-memory implementation for tests. Real sessions wire up the
/// disk-backed `coco_tasks::TaskListStore` instead.
///
/// Matches the semantic of the persistent store closely enough for
/// unit testing: sequential integer IDs, cascade on delete, atomic
/// claim with busy-check guard, metadata merge with null deletion.
pub struct InMemoryTaskListHandle {
    inner: std::sync::Mutex<InMemoryState>,
}

struct InMemoryState {
    tasks: HashMap<String, TaskRecord>,
    next_id: i64,
    high_water_mark: i64,
}

impl InMemoryTaskListHandle {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(InMemoryState {
                tasks: HashMap::new(),
                next_id: 1,
                high_water_mark: 0,
            }),
        }
    }
}

impl Default for InMemoryTaskListHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl TaskListHandle for InMemoryTaskListHandle {
    async fn create_task(
        &self,
        subject: String,
        description: String,
        active_form: Option<String>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) -> anyhow::Result<TaskRecord> {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Advance next_id past HWM so deleted ids don't get reassigned.
        if guard.high_water_mark >= guard.next_id {
            guard.next_id = guard.high_water_mark + 1;
        }
        let id = guard.next_id.to_string();
        guard.next_id += 1;
        let task = TaskRecord {
            id: id.clone(),
            subject,
            description,
            active_form,
            owner: None,
            status: TaskListStatus::Pending,
            blocks: Vec::new(),
            blocked_by: Vec::new(),
            metadata,
        };
        guard.tasks.insert(id, task.clone());
        Ok(task)
    }

    async fn get_task(&self, task_id: &str) -> anyhow::Result<Option<TaskRecord>> {
        Ok(self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tasks
            .get(task_id)
            .cloned())
    }

    async fn list_tasks(&self) -> anyhow::Result<Vec<TaskRecord>> {
        Ok(self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tasks
            .values()
            .cloned()
            .collect())
    }

    async fn update_task(
        &self,
        task_id: &str,
        updates: TaskRecordUpdate,
    ) -> anyhow::Result<Option<TaskRecord>> {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(task) = guard.tasks.get_mut(task_id) else {
            return Ok(None);
        };
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
            task.status = v;
        }
        if let Some(merge) = updates.metadata_merge {
            let mut base = task.metadata.clone().unwrap_or_default();
            for (k, v) in merge {
                if v.is_null() {
                    base.remove(&k);
                } else {
                    base.insert(k, v);
                }
            }
            task.metadata = if base.is_empty() { None } else { Some(base) };
        }
        Ok(Some(task.clone()))
    }

    async fn delete_task(&self, task_id: &str) -> anyhow::Result<bool> {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Bump HWM before deletion.
        if let Ok(numeric) = task_id.parse::<i64>()
            && numeric > guard.high_water_mark
        {
            guard.high_water_mark = numeric;
        }
        let removed = guard.tasks.remove(task_id).is_some();
        if removed {
            // Cascade: strip the id from siblings.
            for sibling in guard.tasks.values_mut() {
                sibling.blocks.retain(|b| b != task_id);
                sibling.blocked_by.retain(|b| b != task_id);
            }
        }
        Ok(removed)
    }

    async fn block_task(&self, from_id: &str, to_id: &str) -> anyhow::Result<bool> {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !guard.tasks.contains_key(from_id) || !guard.tasks.contains_key(to_id) {
            return Ok(false);
        }
        let mut changed = false;
        if let Some(t) = guard.tasks.get_mut(from_id)
            && !t.blocks.iter().any(|b| b == to_id)
        {
            t.blocks.push(to_id.to_string());
            changed = true;
        }
        if let Some(t) = guard.tasks.get_mut(to_id)
            && !t.blocked_by.iter().any(|b| b == from_id)
        {
            t.blocked_by.push(from_id.to_string());
            changed = true;
        }
        Ok(changed)
    }

    async fn claim_task(
        &self,
        task_id: &str,
        claimant: &str,
        check_agent_busy: bool,
    ) -> anyhow::Result<TaskClaimOutcome> {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(task) = guard.tasks.get(task_id).cloned() else {
            return Ok(TaskClaimOutcome::TaskNotFound);
        };
        if let Some(owner) = &task.owner
            && owner != claimant
        {
            return Ok(TaskClaimOutcome::AlreadyClaimed(task));
        }
        if task.status == TaskListStatus::Completed {
            return Ok(TaskClaimOutcome::AlreadyResolved(task));
        }
        let unresolved: std::collections::HashSet<String> = guard
            .tasks
            .values()
            .filter(|t| t.status != TaskListStatus::Completed)
            .map(|t| t.id.clone())
            .collect();
        let blocked_by: Vec<String> = task
            .blocked_by
            .iter()
            .filter(|id| unresolved.contains(*id))
            .cloned()
            .collect();
        if !blocked_by.is_empty() {
            return Ok(TaskClaimOutcome::Blocked {
                task,
                blocked_by_tasks: blocked_by,
            });
        }
        if check_agent_busy {
            let busy_with: Vec<String> = guard
                .tasks
                .values()
                .filter(|t| {
                    t.status != TaskListStatus::Completed
                        && t.owner.as_deref() == Some(claimant)
                        && t.id != task_id
                })
                .map(|t| t.id.clone())
                .collect();
            if !busy_with.is_empty() {
                return Ok(TaskClaimOutcome::AgentBusy {
                    task,
                    busy_with_tasks: busy_with,
                });
            }
        }
        if let Some(t) = guard.tasks.get_mut(task_id) {
            t.owner = Some(claimant.to_string());
            return Ok(TaskClaimOutcome::Success(t.clone()));
        }
        Ok(TaskClaimOutcome::TaskNotFound)
    }

    async fn should_nudge_verification(&self, just_completed: bool, is_main_thread: bool) -> bool {
        if !is_main_thread || !just_completed {
            return false;
        }
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tasks: Vec<&TaskRecord> = guard.tasks.values().collect();
        if tasks.len() < 3 {
            return false;
        }
        let all_done = tasks.iter().all(|t| t.status == TaskListStatus::Completed);
        if !all_done {
            return false;
        }
        !tasks
            .iter()
            .any(|t| t.subject.to_ascii_lowercase().contains("verif"))
    }
}

/// No-op implementation for non-swarm sessions lacking a store.
/// Returns `Err` on writes so misconfigured tool use surfaces loudly.
pub struct NoOpTaskListHandle;

#[async_trait::async_trait]
impl TaskListHandle for NoOpTaskListHandle {
    async fn create_task(
        &self,
        _subject: String,
        _description: String,
        _active_form: Option<String>,
        _metadata: Option<HashMap<String, serde_json::Value>>,
    ) -> anyhow::Result<TaskRecord> {
        Err(anyhow::anyhow!(
            "task-list handle not configured (no persistent store)"
        ))
    }
    async fn get_task(&self, _task_id: &str) -> anyhow::Result<Option<TaskRecord>> {
        Ok(None)
    }
    async fn list_tasks(&self) -> anyhow::Result<Vec<TaskRecord>> {
        Ok(Vec::new())
    }
    async fn update_task(
        &self,
        _task_id: &str,
        _updates: TaskRecordUpdate,
    ) -> anyhow::Result<Option<TaskRecord>> {
        Ok(None)
    }
    async fn delete_task(&self, _task_id: &str) -> anyhow::Result<bool> {
        Ok(false)
    }
    async fn block_task(&self, _from_id: &str, _to_id: &str) -> anyhow::Result<bool> {
        Ok(false)
    }
    async fn claim_task(
        &self,
        _task_id: &str,
        _claimant: &str,
        _check_agent_busy: bool,
    ) -> anyhow::Result<TaskClaimOutcome> {
        Ok(TaskClaimOutcome::TaskNotFound)
    }
    async fn should_nudge_verification(
        &self,
        _just_completed: bool,
        _is_main_thread: bool,
    ) -> bool {
        false
    }
}

// ── TodoListHandle (V1) ───────────────────────────────────────────────

/// Access to the per-agent ephemeral todo store (V1).
///
/// Key convention: `agent_id.clone().unwrap_or_else(|| session_id)`.
/// Matches TS `appState.todos[context.agentId ?? getSessionId()]`.
#[async_trait::async_trait]
pub trait TodoListHandle: Send + Sync {
    async fn read(&self, key: &str) -> Vec<TodoRecord>;
    async fn write(&self, key: &str, items: Vec<TodoRecord>);
}

pub type TodoListHandleRef = Arc<dyn TodoListHandle>;

/// Default in-memory implementation. Suitable for the main process —
/// teammate processes get their own instance; the V1 todo store is
/// per-process (TS `AppState.todos`).
pub struct InMemoryTodoListHandle {
    inner: std::sync::Mutex<HashMap<String, Vec<TodoRecord>>>,
}

impl InMemoryTodoListHandle {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryTodoListHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl TodoListHandle for InMemoryTodoListHandle {
    async fn read(&self, key: &str) -> Vec<TodoRecord> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    async fn write(&self, key: &str, items: Vec<TodoRecord>) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if items.is_empty() {
            guard.remove(key);
        } else {
            guard.insert(key.to_string(), items);
        }
    }
}
