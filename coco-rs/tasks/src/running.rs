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

/// Output captured from a completed running task.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Running task manager — tracks background tasks and their outputs.
pub struct TaskManager {
    tasks: Arc<RwLock<HashMap<String, TaskStateBase>>>,
    outputs: Arc<RwLock<HashMap<String, TaskOutput>>>,
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
            outputs: Arc::new(RwLock::new(HashMap::new())),
            event_tx: None,
        }
    }

    /// Attach an event sink so every task lifecycle transition flows
    /// into the `CoreEvent` channel. Builder pattern; consumes self.
    pub fn with_event_sink(mut self, event_tx: mpsc::Sender<CoreEvent>) -> Self {
        self.event_tx = Some(event_tx);
        self
    }

    /// Create a new running task.
    pub async fn create(
        &self,
        task_type: TaskType,
        description: &str,
        output_file: &str,
    ) -> String {
        let id = generate_task_id(task_type);
        let state = TaskStateBase {
            id: id.clone(),
            task_type,
            status: TaskStatus::Pending,
            description: description.to_string(),
            tool_use_id: None,
            start_time: current_time_ms(),
            end_time: None,
            total_paused_ms: None,
            output_file: output_file.to_string(),
            output_offset: 0,
            notified: false,
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
        // Capture the snapshot we need for emission inside the write lock,
        // then drop the lock before `.await`-ing the channel send so we
        // don't hold the RwLock across an await point.
        let snapshot = {
            let mut tasks = self.tasks.write().await;
            if let Some(task) = tasks.get_mut(id) {
                task.status = status;
                if status.is_terminal() {
                    task.end_time = Some(current_time_ms());
                }
                Some((task.clone(), task.output_file.clone()))
            } else {
                None
            }
        };
        if let Some((task, output_file)) = snapshot {
            if status.is_terminal() {
                self.emit_task_completed(id, &task, &output_file).await;
            } else {
                self.emit_task_progress(id, &task).await;
            }
        }
    }

    pub async fn stop(&self, id: &str) {
        self.update_status(id, TaskStatus::Cancelled).await;
    }

    pub async fn set_output(&self, id: &str, output: TaskOutput) {
        self.outputs.write().await.insert(id.to_string(), output);
    }

    pub async fn get_output(&self, id: &str) -> Option<TaskOutput> {
        self.outputs.read().await.get(id).cloned()
    }

    pub async fn list(&self) -> Vec<TaskStateBase> {
        self.tasks.read().await.values().cloned().collect()
    }

    /// Remove all tasks in a terminal state and their stored outputs.
    /// Returns the number of tasks removed.
    pub async fn remove_completed(&self) -> usize {
        let mut tasks = self.tasks.write().await;
        let mut outputs = self.outputs.write().await;

        let terminal_ids: Vec<String> = tasks
            .iter()
            .filter(|(_, t)| t.status.is_terminal())
            .map(|(id, _)| id.clone())
            .collect();

        let count = terminal_ids.len();
        for id in &terminal_ids {
            tasks.remove(id);
            outputs.remove(id);
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
/// `SDKTaskStartedMessage`.
fn task_type_wire_name(task_type: TaskType) -> &'static str {
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
        TaskStatus::Killed | TaskStatus::Cancelled => TaskCompletionStatus::Stopped,
        // Non-terminal states default to Completed (unreachable in practice).
        TaskStatus::Pending | TaskStatus::Running => TaskCompletionStatus::Completed,
    }
}

#[cfg(test)]
#[path = "running.test.rs"]
mod tests;
