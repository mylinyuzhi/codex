//! Running background tasks (LocalShell, LocalAgent, Workflow, etc.).
//!
//! TS: `Task.ts` — `TaskStateBase`, `TaskHandle`, `TaskType`, `TaskStatus`.
//!
//! Lifecycle emissions: when `TaskManager` is constructed with
//! `with_event_sink(tx)`, every `create` / `update_status` emits the
//! matching `CoreEvent::Protocol(TaskStarted | TaskProgress |
//! TaskCompleted)` so SDK consumers see background-task activity in
//! the same NDJSON stream as session/turn events. TS parity:
//! `sdkEventQueue` drain in `utils/sdkEventQueue.ts`.
//!
//! This module handles **running** tasks only — durable plan items
//! live in [`crate::task_list`], ephemeral TodoWrite lists live in
//! [`crate::todos`].

use coco_types::CoreEvent;
use coco_types::ServerNotification;
use coco_types::TaskCompletedParams;
use coco_types::TaskCompletionStatus;
use coco_types::TaskProgressParams;
use coco_types::TaskStartedParams;
use coco_types::TaskStateBase;
use coco_types::TaskStatus;
use coco_types::TaskType;
use coco_types::TaskUsage;
use coco_types::generate_task_id;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc;

/// Grace period before the panel may evict a terminal LocalAgent
/// task. TS: `utils/task/framework.ts:28` `PANEL_GRACE_MS = 30_000`.
pub const PANEL_GRACE_MS: i64 = 30_000;

/// Backward-compat view of the (now-merged) per-task agent-sidecar
/// fields. After W6 / A5 these live directly on `TaskStateBase`;
/// `LocalAgentExtra` is kept as a read-only projection so existing
/// callers (`MemoryRuntime`, tests) can keep their snapshot-and-read
/// pattern without rewriting every call site.
///
/// TS source: `tasks/LocalAgentTask/LocalAgentTask.tsx:128-148`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalAgentExtra {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_summary: Option<String>,
    #[serde(default)]
    pub retrieved: bool,
    #[serde(default)]
    pub retain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evict_after: Option<i64>,
    #[serde(default)]
    pub is_backgrounded: bool,
}

impl From<&TaskStateBase> for LocalAgentExtra {
    fn from(t: &TaskStateBase) -> Self {
        Self {
            progress_summary: t.progress_summary.clone(),
            retrieved: t.retrieved,
            retain: t.retain,
            evict_after: t.evict_after,
            is_backgrounded: t.is_backgrounded,
        }
    }
}

/// Running task manager — tracks background-task lifecycle state.
///
/// Output buffers do **not** live here; the per-task disk file owned
/// by [`coco_cli::disk_task_output::DiskOutputs`] is the system of
/// record for captured stdout/stderr/result text. The manager holds
/// only the typed status, timestamps, and the (W6-merged) LocalAgent
/// sidecar fields on the same `TaskStateBase`.
///
/// **W6 (A5)**: single `tasks` RwLock instead of two locks
/// (`tasks` + `local_agent_extras`). Eliminates the race where a UI
/// `set_retain(true)` and the `update_status(terminal)` evict-after
/// stamp could interleave, silently evicting a panel-pinned task.
pub struct TaskManager {
    tasks: Arc<RwLock<HashMap<String, TaskStateBase>>>,
    /// Optional sink for lifecycle events. When set, `create` and
    /// `update_status` emit `CoreEvent::Protocol(Task*)` notifications
    /// so SDK consumers see background task activity.
    event_tx: Option<mpsc::Sender<CoreEvent>>,
}

impl std::fmt::Debug for TaskManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskManager")
            .field("event_sink", &self.event_tx.is_some())
            .finish()
    }
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            event_tx: None,
        }
    }

    // ── LocalAgent sidecar accessors (W6 / A5 merged) ────────────
    //
    // Fields live on `TaskStateBase`. Setters take the single `tasks`
    // RwLock once. Reads can take a shared lock.

    /// Snapshot the extras for a LocalAgent task; returns default
    /// when the task doesn't exist.
    pub async fn local_agent_extra(&self, id: &str) -> LocalAgentExtra {
        self.tasks
            .read()
            .await
            .get(id)
            .map(LocalAgentExtra::from)
            .unwrap_or_default()
    }

    /// Update a LocalAgent task's progress summary. Idempotent.
    pub async fn set_progress_summary(&self, id: &str, summary: String) {
        if let Some(t) = self.tasks.write().await.get_mut(id) {
            t.progress_summary = Some(summary);
        }
    }

    /// Flip the `retrieved` flag — TS: `compact.ts:1578`.
    pub async fn mark_retrieved(&self, id: &str) {
        if let Some(t) = self.tasks.write().await.get_mut(id) {
            t.retrieved = true;
        }
    }

    /// UI flips this when the user pins a task panel open.
    pub async fn set_retain(&self, id: &str, retain: bool) {
        if let Some(t) = self.tasks.write().await.get_mut(id) {
            t.retain = retain;
        }
    }

    /// Stamp the panel grace-period deadline.
    pub async fn set_evict_after(&self, id: &str, evict_after_ms: Option<i64>) {
        if let Some(t) = self.tasks.write().await.get_mut(id) {
            t.evict_after = evict_after_ms;
        }
    }

    /// Toggle the Ctrl+B session-backgrounded flag.
    pub async fn set_backgrounded(&self, id: &str, backgrounded: bool) {
        if let Some(t) = self.tasks.write().await.get_mut(id) {
            t.is_backgrounded = backgrounded;
        }
    }

    /// Attach an event sink so every task lifecycle transition flows
    /// into the `CoreEvent` channel. Builder pattern; consumes self.
    pub fn with_event_sink(mut self, event_tx: mpsc::Sender<CoreEvent>) -> Self {
        self.event_tx = Some(event_tx);
        self
    }

    /// Create a new task in the [`TaskStatus::Pending`] state. Most
    /// production callers want [`Self::create_running`] — Pending is
    /// reserved for queued-but-not-yet-spawned tasks (none ship in
    /// coco-rs today). Tests use this to exercise lifecycle
    /// transitions explicitly.
    pub async fn create(
        &self,
        task_type: TaskType,
        description: &str,
        output_file: &str,
    ) -> String {
        let id = generate_task_id(task_type);
        self.insert_and_emit(id, task_type, description, output_file, TaskStatus::Pending)
            .await
    }

    /// Create + insert a task already in [`TaskStatus::Running`].
    /// Used by every production background-task spawn — agent
    /// registration (`TaskRuntime::register_agent_task`) and shell
    /// dispatch (`TaskRuntime::spawn_shell_task`) both call this so
    /// only ONE lifecycle event fires: a single
    /// `TaskStarted(Running)` instead of TS-noisy
    /// `TaskStarted(Pending)` → `TaskProgress(Running)` pair.
    pub async fn create_running(
        &self,
        task_type: TaskType,
        description: &str,
        output_file: &str,
    ) -> String {
        let id = generate_task_id(task_type);
        self.insert_and_emit(id, task_type, description, output_file, TaskStatus::Running)
            .await
    }

    /// Insert a task with a caller-provided id (minted upstream so
    /// the caller can resolve per-id state like the disk-output
    /// path *before* the lifecycle event fires) already in
    /// [`TaskStatus::Running`]. Returns the id unchanged.
    pub async fn create_running_with_id(
        &self,
        id: String,
        task_type: TaskType,
        description: &str,
        output_file: &str,
    ) -> String {
        self.insert_and_emit(id, task_type, description, output_file, TaskStatus::Running)
            .await
    }

    async fn insert_and_emit(
        &self,
        id: String,
        task_type: TaskType,
        description: &str,
        output_file: &str,
        status: TaskStatus,
    ) -> String {
        let state = TaskStateBase {
            id: id.clone(),
            task_type,
            status,
            description: description.to_string(),
            tool_use_id: None,
            start_time: current_time_ms(),
            end_time: None,
            total_paused_ms: None,
            output_file: output_file.to_string(),
            output_offset: 0,
            // W6 (A5): merged sidecar fields default to "not set".
            progress_summary: None,
            retrieved: false,
            retain: false,
            evict_after: None,
            is_backgrounded: false,
        };
        self.tasks.write().await.insert(id.clone(), state);
        self.emit_task_started(&id, task_type, description, output_file)
            .await;
        id
    }

    pub async fn get(&self, id: &str) -> Option<TaskStateBase> {
        self.tasks.read().await.get(id).cloned()
    }

    pub async fn update_status(&self, id: &str, status: TaskStatus) {
        // W6 (A5): status flip + evict_after stamp now happen under
        // a single write lock. Previously the evict_after stamp went
        // through `set_evict_after` (the sidecar map's separate
        // lock), creating a window where a UI `set_retain(true)`
        // could land between us reading `retain=false` and writing
        // `evict_after`, silently evicting the just-pinned task.
        let snapshot = {
            let mut tasks = self.tasks.write().await;
            if let Some(task) = tasks.get_mut(id) {
                task.status = status;
                if status.is_terminal() {
                    task.end_time = Some(current_time_ms());
                    // Stamp `evict_after` atomically with the status
                    // flip. TS parity:
                    // `LocalAgentTask.tsx:294, 424, 448` —
                    // `evictAfter: task.retain ? undefined : Date.now() + PANEL_GRACE_MS`.
                    if task.task_type == TaskType::LocalAgent && !task.retain {
                        task.evict_after = Some(current_time_ms() + PANEL_GRACE_MS);
                    }
                }
                Some((task.clone(), task.output_file.clone()))
            } else {
                None
            }
        };
        // Drop the write lock before `.await`-ing the channel send so
        // we don't hold the RwLock across an await point.
        if let Some((task, output_file)) = snapshot {
            if status.is_terminal() {
                self.emit_task_completed(id, &task, &output_file).await;
            } else {
                self.emit_task_progress(id, &task).await;
            }
        }
    }

    /// Stamp the `tool_use_id` onto the matching task entry. Used by
    /// task-spawning tools (AgentTool, BashTool background) so the
    /// `<task-notification>` envelope can route back to the model's
    /// invocation that started the task.
    pub async fn set_tool_use_id(&self, id: &str, tool_use_id: String) {
        if let Some(task) = self.tasks.write().await.get_mut(id) {
            task.tool_use_id = Some(tool_use_id);
        }
    }

    pub async fn list(&self) -> Vec<TaskStateBase> {
        self.tasks.read().await.values().cloned().collect()
    }

    /// Remove all tasks in a terminal state.
    /// Returns the number of tasks removed.
    ///
    /// **Panel-grace gate**: a LocalAgent task whose `evict_after`
    /// is in the future and whose `retain` is true OR whose
    /// `evict_after` deadline has not passed is NOT removed. TS
    /// parity: `framework.ts:138, 241` — `if ('retain' in task &&
    /// (task.evictAfter ?? Infinity) > Date.now()) return prev`.
    pub async fn remove_completed(&self) -> usize {
        // W6 (A5): single-lock eviction — `retain` / `evict_after`
        // live on the same task entry. No risk of a UI flip racing
        // the eviction decision.
        let mut tasks = self.tasks.write().await;
        let now = current_time_ms();

        let terminal_ids: Vec<String> = tasks
            .iter()
            .filter(|(_id, t)| {
                if !t.status.is_terminal() {
                    return false;
                }
                // Panel-grace gate for LocalAgent tasks (TS parity).
                if t.task_type == TaskType::LocalAgent {
                    if t.retain {
                        return false;
                    }
                    if let Some(deadline) = t.evict_after
                        && deadline > now
                    {
                        return false;
                    }
                }
                true
            })
            .map(|(id, _)| id.clone())
            .collect();

        let count = terminal_ids.len();
        for id in &terminal_ids {
            tasks.remove(id);
        }
        count
    }

    async fn emit_task_started(
        &self,
        task_id: &str,
        task_type: TaskType,
        description: &str,
        _output_file: &str,
    ) {
        let Some(tx) = &self.event_tx else { return };
        let params = TaskStartedParams {
            task_id: task_id.to_string(),
            tool_use_id: None,
            description: description.to_string(),
            task_type: Some(task_type_wire_name(task_type).to_string()),
            workflow_name: None,
            prompt: None,
        };
        let _ = tx
            .send(CoreEvent::Protocol(ServerNotification::TaskStarted(params)))
            .await;
    }

    async fn emit_task_progress(&self, task_id: &str, state: &TaskStateBase) {
        let Some(tx) = &self.event_tx else { return };
        let duration_ms = current_time_ms().saturating_sub(state.start_time);
        let params = TaskProgressParams {
            task_id: task_id.to_string(),
            tool_use_id: state.tool_use_id.clone(),
            description: state.description.clone(),
            usage: TaskUsage {
                total_tokens: 0,
                tool_uses: 0,
                duration_ms,
            },
            last_tool_name: None,
            summary: None,
            workflow_progress: Vec::new(),
        };
        let _ = tx
            .send(CoreEvent::Protocol(ServerNotification::TaskProgress(
                params,
            )))
            .await;
    }

    async fn emit_task_completed(&self, task_id: &str, state: &TaskStateBase, output_file: &str) {
        let Some(tx) = &self.event_tx else { return };
        let status = task_status_to_completion(state.status);
        let duration_ms = state
            .end_time
            .unwrap_or_else(current_time_ms)
            .saturating_sub(state.start_time);
        let params = TaskCompletedParams {
            task_id: task_id.to_string(),
            tool_use_id: state.tool_use_id.clone(),
            status,
            output_file: output_file.to_string(),
            summary: state.description.clone(),
            usage: Some(TaskUsage {
                total_tokens: 0,
                tool_uses: 0,
                duration_ms,
            }),
        };
        let _ = tx
            .send(CoreEvent::Protocol(ServerNotification::TaskCompleted(
                params,
            )))
            .await;
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

fn current_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Wire name for a `TaskType`, matching TS `task_type` strings used in
/// `SDKTaskStartedMessage` and the `task_status` reminder
/// `(type: ...)` metadata.
pub(crate) fn task_type_wire_name(task_type: TaskType) -> &'static str {
    match task_type {
        TaskType::LocalBash => "local_bash",
        TaskType::LocalAgent => "local_agent",
        TaskType::LocalWorkflow => "local_workflow",
        TaskType::RemoteAgent => "remote_agent",
        TaskType::InProcessTeammate => "in_process_teammate",
        TaskType::MonitorMcp => "monitor_mcp",
        TaskType::Dream => "dream",
    }
}

/// Map `TaskStatus` (6 variants) to the 3-variant `TaskCompletionStatus`
/// used in `SDKTaskNotificationMessage`. Pending/Running are not terminal
/// states — callers should only reach here when `status.is_terminal()`.
fn task_status_to_completion(status: TaskStatus) -> TaskCompletionStatus {
    match status {
        TaskStatus::Completed => TaskCompletionStatus::Completed,
        TaskStatus::Failed => TaskCompletionStatus::Failed,
        TaskStatus::Killed => TaskCompletionStatus::Stopped,
        // Non-terminal states default to Completed (unreachable in practice).
        TaskStatus::Pending | TaskStatus::Running => TaskCompletionStatus::Completed,
    }
}

#[cfg(test)]
#[path = "running.test.rs"]
mod tests;
