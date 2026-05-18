//! Production [`coco_tool_runtime::TaskHandle`] backed by
//! [`coco_tasks::TaskManager`] (P2'+ TaskManager wiring).
//!
//! ## Why
//!
//! Before this module, every production session installed
//! `task_handle: None` on `ToolUseContext`. `TaskGet` /
//! `TaskOutput` / `TaskStop` / `TaskList` returned "no task runtime
//! configured" errors regardless of whether the model had spawned a
//! background task. AgentTool's background path (P2') drove the
//! engine in a detached tokio task but had no way to register the
//! result so the model could address it later.
//!
//! ## Architecture
//!
//! `TaskRuntime` is the single shared owner. The same `Arc` is
//! handed to:
//!
//! - the engine (via `wire_engine` → `with_task_handle` →
//!   `ToolUseContext.task_handle`) for the read/control side
//!   consumed by `Task*` tools;
//! - `SwarmAgentHandle` (via `set_task_runtime`) for the
//!   registration side: AgentTool's background dispatch calls
//!   `runtime.register_agent_task(...)`, which creates the
//!   `TaskManager` entry, allocates per-task output + cancellation,
//!   and returns the `task_id` for the response payload.
//!
//! ## Read / control surface (TaskHandle trait)
//!
//! - `get_task_status` — maps `coco_types::TaskStatus` to
//!   `BackgroundTaskStatus`.
//! - `get_task_output_delta` — incremental read against the per-task
//!   buffer with offset bookkeeping. Used by `TaskOutput`.
//! - `kill_task` — flips the per-task cancellation token AND marks
//!   the manager entry as `Killed`. The bg AgentTool spawn observes
//!   the token and exits early.
//! - `list_tasks` — snapshot of every running task.
//! - `poll_notifications` — terminal-state tasks that the framework
//!   should announce. Stall detection is **not** wired for agent
//!   tasks (TS does this only for shell tasks); calls return [].
//! - `spawn_shell_task` — out of scope for this module; bash-tool
//!   shell tasks need a separate handle. Returns an explicit error.
//!
//! ## Output buffer semantics — disk-backed (TS-aligned)
//!
//! Each task's output is funneled through
//! [`crate::disk_task_output::DiskTaskOutput`] — a TS-aligned port
//! of `utils/task/diskOutput.ts` that writes to
//! `<config_home>/cache/tasks/<session_id>/<task_id>.output` via a
//! single drain task. The 5 GB disk cap matches TS
//! `MAX_TASK_OUTPUT_BYTES`; past it, a truncation marker is
//! appended once and further writes are dropped.
//!
//! `read_output(task_id)` is a tail read for the periodic-summary
//! timer (caps at 8 MiB, matches TS `DEFAULT_MAX_READ_BYTES`).
//! `get_task_output_delta(task_id, from_offset)` is the incremental
//! reader the `TaskOutput` tool drives.
//!
//! The previous in-memory `Arc<Mutex<String>>` cap and the
//! UTF-8-aware head-truncation logic are removed in favor of the
//! disk file as the system of record — the file system already
//! provides bounded reads via `pread`, and the disk cap is 600× the
//! old in-memory cap so long coordinator workloads stop losing
//! early context.
//!
//! Per-task output offset tracking on the consumer side is
//! preserved by the on-disk file's stable byte ordering — readers
//! that hold a `from_offset` from a prior call will see content
//! after that offset on the next read, never duplicates.
//!
//! ## Timer-leak protection
//!
//! Periodic AgentSummary timers in `agent_handle_spawn.rs` race the
//! per-task `CancellationToken` against a 30 s ticker. To bound the
//! window between natural engine completion and timer exit,
//! [`TaskRuntime::mark_completed`] / [`TaskRuntime::mark_failed`]
//! BOTH cancel the token in addition to flipping the lifecycle
//! status — so a clean engine exit terminates the timer immediately
//! instead of waiting up to 30 s for the next `is_terminal` poll.
//!
//! ## Construction
//!
//! Production callers use [`TaskRuntime::with_session_dir`] to wire
//! a per-session disk root (`<config_home>/cache/tasks/<session_id>`).
//! Tests can use [`TaskRuntime::with_temp_dir`] for isolation, or
//! the legacy `new` constructor which spins up an
//! ephemeral temp directory — keeping existing tests unchanged.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use coco_tasks::TaskManager;
use coco_tool_runtime::{
    AgentTaskRegistry, BackgroundShellRequest, BackgroundTaskInfo, BackgroundTaskStatus,
    TaskHandle, TaskOutputDelta,
};
use coco_types::{TaskStatus, TaskType};
use tokio_util::sync::CancellationToken;

use crate::disk_task_output::{DEFAULT_MAX_READ_BYTES, DiskOutputs};

/// Per-task control state held alongside the `TaskManager` entry.
/// The `TaskManager` owns lifecycle status; the on-disk file owns
/// content (managed by `DiskOutputs`); this struct owns the
/// "stop me" cancellation token.
struct TaskEntry {
    cancel: CancellationToken,
}

/// Production task runtime.
///
/// Cheap to clone (every field is `Arc`). Construction is intended
/// to happen once per session in CLI bootstrap; the same `Arc<Self>`
/// flows into the engine and into `SwarmAgentHandle`.
pub struct TaskRuntime {
    manager: Arc<TaskManager>,
    entries: Arc<tokio::sync::RwLock<HashMap<String, TaskEntry>>>,
    disk: Arc<DiskOutputs>,
}

impl TaskRuntime {
    /// Test-friendly constructor — creates an ephemeral temp
    /// directory under `std::env::temp_dir()` keyed by a fresh UUID
    /// so concurrent tests don't collide. Production callers should
    /// use [`Self::with_session_dir`].
    pub fn new(manager: Arc<TaskManager>) -> Self {
        let temp =
            std::env::temp_dir().join(format!("coco-task-rt-{}", uuid::Uuid::new_v4().simple()));
        Self::with_session_dir(manager, temp)
    }

    /// Production constructor. `session_dir` is the per-session
    /// root for on-disk task output files (typically
    /// `<config_home>/cache/tasks/<session_id>`).
    pub fn with_session_dir(manager: Arc<TaskManager>, session_dir: std::path::PathBuf) -> Self {
        Self {
            manager,
            entries: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            disk: Arc::new(DiskOutputs::new(session_dir)),
        }
    }

    /// Read access to the inner `TaskManager` — useful for callers
    /// that already speak the typed-state API directly (e.g. the
    /// engine's own task lifecycle emissions).
    pub fn manager(&self) -> &Arc<TaskManager> {
        &self.manager
    }

    /// Register a background AgentTool spawn. Creates the
    /// `TaskManager` entry, returns the `task_id`, and stores the
    /// cancel token + output buffer keyed by id. The caller (in
    /// `agent_handle_spawn::spawn_subagent`) drives the engine in a
    /// detached task and uses [`Self::append_output`] +
    /// [`Self::mark_completed`] / [`Self::mark_failed`] to update
    /// status as the spawn progresses.
    ///
    /// `description` becomes the task's display label (panel +
    /// `TaskList`). `tool_use_id` ties the registration back to the
    /// model's invocation.
    pub async fn register_agent_task(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        cancel: CancellationToken,
    ) -> String {
        let task_id = self
            .manager
            .create(TaskType::LocalAgent, description, /*output_file=*/ "")
            .await;
        if let Some(tu_id) = tool_use_id {
            self.manager
                .set_tool_use_id(&task_id, tu_id.to_string())
                .await;
        }
        // Move to Running so the panel + lifecycle event reflect that
        // the spawn is in progress. `create` defaults to `Pending`.
        self.manager
            .update_status(&task_id, TaskStatus::Running)
            .await;
        let entry = TaskEntry { cancel };
        self.entries.write().await.insert(task_id.clone(), entry);
        // Eagerly create the disk-output handle so the file path is
        // resolved + the drain task is ready before the first
        // append. Subsequent `disk.get_or_create(task_id)` returns
        // the same `Arc` cheaply.
        let _ = self.disk.get_or_create(&task_id).await;
        task_id
    }

    /// Append text to a task's on-disk output file. Returns
    /// immediately — the actual write runs on the per-task drain
    /// task. Past the 5 GB disk cap (matches TS), drops chunks and
    /// appends a single truncation marker.
    pub async fn append_output(&self, task_id: &str, chunk: &str) {
        let dto = self.disk.get_or_create(task_id).await;
        dto.append(chunk);
    }

    /// Mark the task completed and store the final response text.
    /// Cancels the per-task token so periodic timers exit promptly
    /// instead of waiting up to 30 s for the next `is_terminal` poll.
    pub async fn mark_completed(&self, task_id: &str, response_text: Option<&str>) {
        if let Some(text) = response_text
            && !text.is_empty()
        {
            self.append_output(task_id, text).await;
        }
        self.manager
            .update_status(task_id, TaskStatus::Completed)
            .await;
        self.cancel_task_token(task_id).await;
    }

    /// Mark the task failed and append the error message. Cancels
    /// the per-task token (same reason as `mark_completed`).
    pub async fn mark_failed(&self, task_id: &str, error: &str) {
        self.append_output(task_id, error).await;
        self.manager
            .update_status(task_id, TaskStatus::Failed)
            .await;
        self.cancel_task_token(task_id).await;
    }

    /// Fire the per-task `CancellationToken` so any tokio task
    /// observing it (the bg spawn driver, the periodic AgentSummary
    /// timer) can exit promptly. Idempotent — `cancel.cancel()` is
    /// safe to call after a token already cancelled.
    async fn cancel_task_token(&self, task_id: &str) {
        if let Some(entry) = self.entries.read().await.get(task_id) {
            entry.cancel.cancel();
        }
    }
}

#[async_trait]
impl AgentTaskRegistry for TaskRuntime {
    async fn register_agent_task(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        cancel: CancellationToken,
    ) -> String {
        TaskRuntime::register_agent_task(self, description, tool_use_id, cancel).await
    }
    async fn append_output(&self, task_id: &str, chunk: &str) {
        TaskRuntime::append_output(self, task_id, chunk).await
    }
    async fn mark_completed(&self, task_id: &str, response_text: Option<&str>) {
        TaskRuntime::mark_completed(self, task_id, response_text).await
    }
    async fn mark_failed(&self, task_id: &str, error: &str) {
        TaskRuntime::mark_failed(self, task_id, error).await
    }
    async fn read_output(&self, task_id: &str) -> String {
        // Tail-read with omitted-bytes header. Mirrors TS
        // `getTaskOutput` (`diskOutput.ts:336-357`):
        // returns the last `DEFAULT_MAX_READ_BYTES` (8 MiB) of the
        // file, prepending `[N KB of earlier output omitted]\n`
        // when the file exceeded the cap. The model sees recent
        // activity rather than the cold start — important for long-
        // running coordinator workloads where the head is stale.
        let Some(dto) = self.disk.get(task_id).await else {
            return String::new();
        };
        let _ = dto.flush().await;
        dto.read_tail(DEFAULT_MAX_READ_BYTES)
            .await
            .unwrap_or_default()
    }
    async fn output_file_path(&self, task_id: &str) -> Option<std::path::PathBuf> {
        Some(self.disk.output_path(task_id))
    }
    async fn is_terminal(&self, task_id: &str) -> bool {
        self.manager
            .get(task_id)
            .await
            .map(|s| s.status.is_terminal())
            .unwrap_or(false)
    }
}

fn boxed_msg(msg: impl Into<String>, code: coco_error::StatusCode) -> coco_error::BoxedError {
    Box::new(coco_error::PlainError::new(msg, code))
}

#[async_trait]
impl TaskHandle for TaskRuntime {
    async fn spawn_shell_task(
        &self,
        _: BackgroundShellRequest,
    ) -> Result<String, coco_error::BoxedError> {
        Err(boxed_msg(
            "Shell-task background spawning is not wired through TaskRuntime yet. \
             AgentTool background spawns work; Bash run_in_background does not.",
            coco_error::StatusCode::Internal,
        ))
    }

    async fn get_task_status(
        &self,
        task_id: &str,
    ) -> Result<BackgroundTaskInfo, coco_error::BoxedError> {
        let Some(state) = self.manager.get(task_id).await else {
            return Err(boxed_msg(
                format!("No running task found with ID: {task_id}"),
                coco_error::StatusCode::FileNotFound,
            ));
        };
        Ok(state_to_info(&state))
    }

    async fn get_task_output_delta(
        &self,
        task_id: &str,
        from_offset: i64,
    ) -> Result<TaskOutputDelta, coco_error::BoxedError> {
        let Some(state) = self.manager.get(task_id).await else {
            return Err(boxed_msg(
                format!("No running task found with ID: {task_id}"),
                coco_error::StatusCode::FileNotFound,
            ));
        };
        // Disk-backed delta read. Flush the drain queue first so a
        // freshly-appended chunk is visible — TS `getTaskOutputDelta`
        // is implicitly synchronous via single-threaded JS.
        let Some(dto) = self.disk.get(task_id).await else {
            return Ok(TaskOutputDelta {
                content: String::new(),
                new_offset: from_offset,
                is_complete: state.status.is_terminal(),
            });
        };
        let _ = dto.flush().await;
        let (content, new_offset) = match dto.read_delta(from_offset, DEFAULT_MAX_READ_BYTES).await
        {
            Ok(pair) => pair,
            Err(_) => (String::new(), from_offset),
        };
        let is_complete = state.status.is_terminal();
        Ok(TaskOutputDelta {
            content,
            new_offset,
            is_complete,
        })
    }

    async fn kill_task(&self, task_id: &str) -> Result<(), coco_error::BoxedError> {
        let cancel = self
            .entries
            .read()
            .await
            .get(task_id)
            .map(|e| e.cancel.clone());
        let Some(cancel) = cancel else {
            return Err(boxed_msg(
                format!("No running task found with ID: {task_id}"),
                coco_error::StatusCode::FileNotFound,
            ));
        };
        cancel.cancel();
        self.manager
            .update_status(task_id, TaskStatus::Killed)
            .await;
        Ok(())
    }

    async fn list_tasks(&self) -> Vec<BackgroundTaskInfo> {
        self.manager
            .list()
            .await
            .iter()
            .map(state_to_info)
            .collect()
    }

    async fn poll_notifications(&self) -> Vec<BackgroundTaskInfo> {
        // Return terminal-state tasks that haven't been notified yet,
        // and flip their `notified` flag so we don't repeat. Stall
        // detection isn't wired for agent tasks — TS only stalls
        // shell tasks.
        let mut out = Vec::new();
        let states = self.manager.list().await;
        for state in &states {
            if state.status.is_terminal() && !state.notified {
                out.push(state_to_info(state));
                self.manager.mark_notified(&state.id).await;
            }
        }
        out
    }
}

fn state_to_info(state: &coco_types::TaskStateBase) -> BackgroundTaskInfo {
    let elapsed_seconds = match state.end_time {
        Some(end) => (end - state.start_time).max(0) as f64 / 1000.0,
        None => {
            // Use system-wall-clock since start. We don't have a
            // monotonic clock plumbed; this is best-effort.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(state.start_time);
            (now - state.start_time).max(0) as f64 / 1000.0
        }
    };
    BackgroundTaskInfo {
        task_id: state.id.clone(),
        status: status_to_background(state.status),
        summary: Some(state.description.clone()),
        output_file: if state.output_file.is_empty() {
            None
        } else {
            Some(state.output_file.clone())
        },
        tool_use_id: state.tool_use_id.clone(),
        elapsed_seconds,
        notified: state.notified,
    }
}

fn status_to_background(s: TaskStatus) -> BackgroundTaskStatus {
    match s {
        TaskStatus::Pending | TaskStatus::Running => BackgroundTaskStatus::Running,
        TaskStatus::Completed => BackgroundTaskStatus::Completed,
        TaskStatus::Failed => BackgroundTaskStatus::Failed,
        TaskStatus::Killed | TaskStatus::Cancelled => BackgroundTaskStatus::Killed,
    }
}

#[cfg(test)]
#[path = "task_runtime.test.rs"]
mod tests;
