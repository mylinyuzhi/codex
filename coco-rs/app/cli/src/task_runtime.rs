//! Production background-task runtime.
//!
//! `TaskRuntime` implements four traits that the tool layer + the
//! coordinator both consume:
//!
//! - [`coco_tool_runtime::TaskReader`] — `TaskGet` / `TaskList` /
//!   `TaskOutput` read paths.
//! - [`coco_tool_runtime::TaskController`] — `TaskStop`.
//! - [`coco_tool_runtime::ShellTaskSpawner`] — Bash / PowerShell
//!   `run_in_background`.
//! - [`coco_tool_runtime::AgentTaskRegistry`] — `SwarmAgentHandle`'s
//!   background AgentTool dispatch.
//!
//! ## Where each concern lives
//!
//! - Lifecycle state — [`coco_tasks::TaskManager`].
//! - Disk-backed output — [`crate::disk_task_output::DiskOutputs`].
//! - Per-task cancel + terminal-status broadcast — [`TaskEntry`]
//!   below, indexed by task id.
//! - Notification XML construction + push — done in this module
//!   using [`coco_tasks::notification`] primitives. The sink
//!   (`Arc<dyn NotificationSink>`) is always wired; tests use
//!   `NoOpNotificationSink` (default), production wires
//!   [`crate::command_queue_sink::CommandQueueNotificationSink`].
//! - Stall watchdog — [`stall::watchdog`] spawned per bg shell.
//!
//! ## TS source
//!
//! - `tasks/LocalShellTask/LocalShellTask.tsx` — shell lifecycle.
//! - `tasks/LocalAgentTask/LocalAgentTask.tsx` — agent lifecycle.
//! - `utils/task/diskOutput.ts` — disk-output semantics.
//! - `utils/task/framework.ts:138, 241` — panel-grace + eviction.

mod stall;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use coco_tasks::{
    NoOpNotificationSink, NotificationKind, NotificationSink, NotificationSinkRef, TaskManager,
    TaskNotification, TaskUsage as NotifTaskUsage, TerminalStatus, Worktree as NotifWorktree,
};
use coco_tool_runtime::{
    AgentCompletionPayload, AgentTaskRegistry, BackgroundShellRequest, ShellTaskSpawner,
    TaskController, TaskOutputDelta, TaskReader, TerminalSignal,
};
use coco_types::{TaskStateBase, TaskStatus, TaskType};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, trace, warn};

use crate::disk_task_output::{DEFAULT_MAX_READ_BYTES, DiskOutputs, DiskTaskOutput};

/// Per-task control state.
///
/// `cancel` fires the kill path. `status_tx` broadcasts terminal
/// transitions so `TaskOutput` blocking reads (and any future
/// observer) can `await` instead of polling. `watch` retains the
/// last value, so a subscriber that arrives after the task ended
/// still sees the terminal status.
struct TaskEntry {
    cancel: CancellationToken,
    status_tx: watch::Sender<TaskStatus>,
}

/// Production task runtime.
///
/// Cheap to clone (every field is `Arc`). Construction happens once
/// per session in CLI bootstrap; the same `Arc<Self>` flows into the
/// engine (read/control) and into `SwarmAgentHandle` (registration).
pub struct TaskRuntime {
    manager: Arc<TaskManager>,
    entries: Arc<tokio::sync::RwLock<HashMap<String, TaskEntry>>>,
    disk: Arc<DiskOutputs>,
    /// Always wired. `NoOpNotificationSink` is the default when no
    /// producer attaches — terminal events are silently dropped,
    /// matching TS sessions that run without a turn loop (headless
    /// jobs / `--bare` SDK). Production attaches the
    /// `CommandQueueNotificationSink`.
    notification_sink: NotificationSinkRef,
}

impl TaskRuntime {
    /// Test-friendly constructor — temp dir, no-op notification
    /// sink. Production callers use [`Self::with_session_dir`] +
    /// [`Self::with_notification_sink`].
    pub fn new(manager: Arc<TaskManager>) -> Self {
        let temp =
            std::env::temp_dir().join(format!("coco-task-rt-{}", uuid::Uuid::new_v4().simple()));
        Self::with_session_dir(manager, temp)
    }

    /// Production constructor. `session_dir` is the per-session
    /// root for on-disk task output files (typically
    /// `<config_home>/cache/tasks/<session_id>`). Notification sink
    /// defaults to no-op until [`Self::with_notification_sink`]
    /// attaches one.
    pub fn with_session_dir(manager: Arc<TaskManager>, session_dir: std::path::PathBuf) -> Self {
        debug!(
            target: "coco::task_runtime",
            session_dir = %session_dir.display(),
            "constructing TaskRuntime"
        );
        Self {
            manager,
            entries: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            disk: Arc::new(DiskOutputs::new(session_dir)),
            notification_sink: Arc::new(NoOpNotificationSink),
        }
    }

    /// Attach the notification sink. After this call, every terminal
    /// transition pushes a `<task-notification>` envelope through the
    /// sink. TS parity: `enqueuePendingNotification({mode:
    /// 'task-notification'})` (`utils/messageQueueManager.ts:142`).
    pub fn with_notification_sink(mut self, sink: NotificationSinkRef) -> Self {
        info!(
            target: "coco::task_runtime",
            "task-notification sink attached"
        );
        self.notification_sink = sink;
        self
    }

    /// Read access to the inner `TaskManager`.
    pub fn manager(&self) -> &Arc<TaskManager> {
        &self.manager
    }

    /// Register a background AgentTool spawn. Mints the id, resolves
    /// the disk path, inserts as `Running` with one lifecycle event,
    /// and stores per-task control state (cancel token + watch).
    #[instrument(
        level = "info",
        skip(self, cancel),
        fields(description = %description, tool_use_id = ?tool_use_id)
    )]
    pub async fn register_agent_task(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        cancel: CancellationToken,
    ) -> String {
        let task_id = coco_types::generate_task_id(TaskType::LocalAgent);
        let dto = self.disk.get_or_create(&task_id).await;
        let output_path = dto.path().display().to_string();
        let assigned = self
            .manager
            .create_running_with_id(
                task_id.clone(),
                TaskType::LocalAgent,
                description,
                &output_path,
            )
            .await;
        debug_assert_eq!(assigned, task_id);
        if let Some(tu_id) = tool_use_id {
            self.manager
                .set_tool_use_id(&task_id, tu_id.to_string())
                .await;
        }
        // `watch::channel` returns (Sender, Receiver). We drop the
        // initial receiver — `subscribe_terminal` creates fresh ones
        // on demand, and `send_replace` doesn't require receivers
        // (`tokio::sync::watch::Sender::send_replace`).
        let (status_tx, _) = watch::channel(TaskStatus::Running);
        self.entries
            .write()
            .await
            .insert(task_id.clone(), TaskEntry { cancel, status_tx });
        info!(
            target: "coco::task_runtime",
            task_id = %task_id,
            task_type = "local_agent",
            output_file = %output_path,
            "agent task registered (Running)"
        );
        task_id
    }

    /// Append text to a task's on-disk output file. Returns
    /// immediately — the actual fs write runs on the per-task drain
    /// task. Past the 5 GB cap, drops chunks and appends a single
    /// truncation marker (TS-aligned).
    pub async fn append_output(&self, task_id: &str, chunk: &str) {
        let dto = self.disk.get_or_create(task_id).await;
        dto.append(chunk);
        trace!(
            target: "coco::task_runtime",
            task_id,
            chunk_bytes = chunk.len(),
            "appended chunk"
        );
    }

    /// Mark an agent task completed. Cancels the per-task token so
    /// periodic timers exit immediately, broadcasts the terminal
    /// status, and pushes a rich `<task-notification>` with optional
    /// `<result>` / `<usage>` / `<worktree>` sections.
    ///
    /// TS parity: `LocalAgentTask.tsx:197-262` `enqueueAgentNotification`.
    #[instrument(
        level = "info",
        skip(self, payload),
        fields(task_id = %task_id)
    )]
    pub async fn mark_completed(&self, task_id: &str, payload: AgentCompletionPayload) {
        if let Some(text) = payload.result.as_deref()
            && !text.is_empty()
        {
            self.append_output(task_id, text).await;
        }
        self.transition_terminal(task_id, TaskStatus::Completed)
            .await;
        self.push_agent_notification(task_id, TerminalStatus::Completed, payload, None)
            .await;
        info!(target: "coco::task_runtime", task_id, "task marked Completed");
    }

    /// Mark an agent task failed. Appends the error to the output
    /// buffer, flips status to `Failed`, fires the watch, and pushes
    /// a notification carrying the error in the summary.
    ///
    /// TS parity: `LocalAgentTask.tsx:197-262` failure branch.
    #[instrument(
        level = "info",
        skip(self, error),
        fields(task_id = %task_id, error_bytes = error.len())
    )]
    pub async fn mark_failed(&self, task_id: &str, error: &str) {
        self.append_output(task_id, error).await;
        self.transition_terminal(task_id, TaskStatus::Failed).await;
        self.push_agent_notification(
            task_id,
            TerminalStatus::Failed,
            AgentCompletionPayload::default(),
            Some(error.to_string()),
        )
        .await;
        warn!(target: "coco::task_runtime", task_id, "task marked Failed");
    }

    async fn transition_terminal(&self, task_id: &str, status: TaskStatus) {
        debug_assert!(status.is_terminal());
        self.manager.update_status(task_id, status).await;
        if let Some(entry) = self.entries.read().await.get(task_id) {
            entry.cancel.cancel();
            // `send_replace` works even when no receivers exist —
            // `send` returns Err in that case and the terminal
            // signal is lost. Watch always retains the last value
            // so a subsequent `subscribe()` sees it.
            entry.status_tx.send_replace(status);
        }
    }

    /// Pull the description + tool_use_id + output_file from
    /// canonical state (TaskManager) and push the agent-shaped
    /// notification.
    async fn push_agent_notification(
        &self,
        task_id: &str,
        status: TerminalStatus,
        payload: AgentCompletionPayload,
        error: Option<String>,
    ) {
        let Some(state) = self.manager.get(task_id).await else {
            return;
        };
        let n = TaskNotification {
            task_id: state.id,
            tool_use_id: state.tool_use_id,
            agent_id: None,
            output_file: state.output_file,
            description: state.description,
            kind: NotificationKind::AgentTerminal {
                status,
                result: payload.result,
                usage: payload.usage.map(|u| NotifTaskUsage {
                    total_tokens: u.total_tokens,
                    tool_uses: u.tool_uses,
                    duration_ms: u.duration_ms,
                }),
                worktree: payload.worktree.map(|w| NotifWorktree {
                    path: w.path,
                    branch: w.branch,
                }),
                error,
            },
        };
        self.notification_sink.push(n).await;
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
    async fn mark_completed(&self, task_id: &str, payload: AgentCompletionPayload) {
        TaskRuntime::mark_completed(self, task_id, payload).await
    }
    async fn mark_failed(&self, task_id: &str, error: &str) {
        TaskRuntime::mark_failed(self, task_id, error).await
    }
    async fn read_output(&self, task_id: &str) -> String {
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
impl TaskReader for TaskRuntime {
    async fn get_task_status(
        &self,
        task_id: &str,
    ) -> Result<TaskStateBase, coco_error::BoxedError> {
        self.manager.get(task_id).await.ok_or_else(|| {
            boxed_msg(
                format!("No running task found with ID: {task_id}"),
                coco_error::StatusCode::FileNotFound,
            )
        })
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
        if is_complete && state.task_type == TaskType::LocalAgent {
            self.manager.mark_retrieved(task_id).await;
            trace!(
                target: "coco::task_runtime",
                task_id,
                "marked LocalAgent task as retrieved"
            );
        }
        trace!(
            target: "coco::task_runtime",
            task_id,
            from_offset,
            new_offset,
            delta_bytes = content.len(),
            is_complete,
            "served task output delta"
        );
        Ok(TaskOutputDelta {
            content,
            new_offset,
            is_complete,
        })
    }

    async fn list_tasks(&self) -> Vec<TaskStateBase> {
        self.manager.list().await
    }

    async fn subscribe_terminal(&self, task_id: &str) -> Option<TerminalSignal> {
        let entries = self.entries.read().await;
        entries
            .get(task_id)
            .map(|e| TerminalSignal::new(e.status_tx.subscribe()))
    }
}

#[async_trait]
impl TaskController for TaskRuntime {
    #[instrument(level = "info", skip(self), fields(task_id = %task_id))]
    async fn kill_task(&self, task_id: &str) -> Result<(), coco_error::BoxedError> {
        let entry_clone = {
            let entries = self.entries.read().await;
            entries
                .get(task_id)
                .map(|e| (e.cancel.clone(), e.status_tx.clone()))
        };
        let Some((cancel, status_tx)) = entry_clone else {
            return Err(boxed_msg(
                format!("No running task found with ID: {task_id}"),
                coco_error::StatusCode::FileNotFound,
            ));
        };
        cancel.cancel();
        self.manager
            .update_status(task_id, TaskStatus::Killed)
            .await;
        status_tx.send_replace(TaskStatus::Killed);
        // Push agent-shaped notification (LocalAgent path); shell
        // tasks reach Killed through their own driver and push from
        // `apply_shell_terminal_state`. We don't know the type
        // here without an extra manager read — branch on it.
        let Some(state) = self.manager.get(task_id).await else {
            return Ok(());
        };
        let n = TaskNotification {
            task_id: state.id,
            tool_use_id: state.tool_use_id,
            agent_id: None,
            output_file: state.output_file,
            description: state.description,
            kind: match state.task_type {
                TaskType::LocalBash => NotificationKind::ShellTerminal {
                    status: TerminalStatus::Killed,
                    exit_code: None,
                },
                _ => NotificationKind::AgentTerminal {
                    status: TerminalStatus::Killed,
                    result: None,
                    usage: None,
                    worktree: None,
                    error: None,
                },
            },
        };
        self.notification_sink.push(n).await;
        info!(
            target: "coco::task_runtime",
            task_id,
            "task killed via kill_task; cancel + watch fired"
        );
        Ok(())
    }
}

#[async_trait]
impl ShellTaskSpawner for TaskRuntime {
    #[instrument(
        level = "info",
        skip(self, request),
        fields(
            command_preview = %command_preview(&request.command),
            timeout_ms = ?request.timeout_ms,
            agent_id = ?request.agent_id,
        )
    )]
    async fn spawn_shell_task(
        &self,
        request: BackgroundShellRequest,
    ) -> Result<String, coco_error::BoxedError> {
        let task_id = coco_types::generate_task_id(TaskType::LocalBash);
        let dto = self.disk.get_or_create(&task_id).await;
        let output_path = dto.path().display().to_string();
        let assigned = self
            .manager
            .create_running_with_id(
                task_id.clone(),
                TaskType::LocalBash,
                &request.description,
                &output_path,
            )
            .await;
        debug_assert_eq!(assigned, task_id);
        if let Some(tu_id) = request.tool_use_id.as_deref() {
            self.manager
                .set_tool_use_id(&task_id, tu_id.to_string())
                .await;
        }
        let cancel = CancellationToken::new();
        let (status_tx, _) = watch::channel(TaskStatus::Running);
        self.entries.write().await.insert(
            task_id.clone(),
            TaskEntry {
                cancel: cancel.clone(),
                status_tx: status_tx.clone(),
            },
        );
        info!(
            target: "coco::task_runtime::shell",
            task_id = %task_id,
            description = %request.description,
            output_file = %output_path,
            "background shell task spawned"
        );

        let manager = self.manager.clone();
        let sink = self.notification_sink.clone();
        let driver_task_id = task_id.clone();
        let driver_description = request.description.clone();
        let driver_tool_use_id = request.tool_use_id.clone();
        let driver_agent_id = request.agent_id.clone();
        let driver_output_path = output_path.clone();
        let command_str = request.command.clone();
        let timeout_ms = request.timeout_ms.unwrap_or(120_000);

        let dto_for_driver = dto.clone();
        let cancel_for_driver = cancel.clone();
        let stall_cancel = CancellationToken::new();
        tokio::spawn(stall::watchdog(
            task_id.clone(),
            request.description.clone(),
            request.tool_use_id.clone(),
            request.agent_id.clone(),
            output_path.clone(),
            dto.clone(),
            sink.clone(),
            stall_cancel.clone(),
        ));

        let driver_status_tx = status_tx;
        let stall_cancel_for_driver = stall_cancel;
        tokio::spawn(async move {
            let outcome =
                run_shell_task(&command_str, timeout_ms, cancel_for_driver, dto_for_driver).await;
            stall_cancel_for_driver.cancel();
            apply_shell_terminal_state(
                &manager,
                &driver_status_tx,
                &driver_task_id,
                &driver_description,
                driver_tool_use_id.as_deref(),
                driver_agent_id.as_deref(),
                &driver_output_path,
                sink.as_ref(),
                outcome,
            )
            .await;
        });

        Ok(task_id)
    }
}

/// Result of one shell-task execution. Carries enough information
/// for `apply_shell_terminal_state` to compose the TS-aligned
/// summary string + status.
enum WaitOutcome {
    Exited { code: i32 },
    TimedOut { budget_ms: i64 },
    Cancelled,
    SpawnFailed,
}

/// Subset of [`WaitOutcome`] the terminal-apply step needs.
/// `stderr_tail` lives separately so it can be threaded through
/// without polluting the lifecycle enum.
struct ShellOutcome {
    wait: WaitOutcome,
}

/// Spawn the child process directly (bypassing `coco_shell::ShellExecutor`
/// — the BashTool security pipeline already cleared the command at
/// the foreground entry point in `bash.rs::execute`, and TS streams
/// stdout straight to disk which `ShellExecutor::execute_with_progress`
/// doesn't expose). Streams stdout + stderr to the per-task disk file
/// in real time so the stall watchdog observes growth.
#[instrument(
    level = "debug",
    skip(cancel, dto),
    fields(command_preview = %command_preview(command), timeout_ms)
)]
async fn run_shell_task(
    command: &str,
    timeout_ms: i64,
    cancel: CancellationToken,
    dto: DiskTaskOutput,
) -> ShellOutcome {
    use tokio::io::AsyncReadExt;
    use tokio::process::Command;

    #[cfg(windows)]
    let (program, args) = ("cmd.exe", vec!["/C", command]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/bash", vec!["-c", command]);

    let mut cmd = Command::new(program);
    cmd.args(&args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let mut child = match cmd.kill_on_drop(true).spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!(
                target: "coco::task_runtime::shell",
                error = %e,
                "failed to spawn child process"
            );
            let msg = format!("\n[failed to spawn shell child: {e}]\n");
            dto.append(&msg);
            let _ = dto.flush().await;
            return ShellOutcome {
                wait: WaitOutcome::SpawnFailed,
            };
        }
    };

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let dto_stdout = dto.clone();
    let stdout_handle = tokio::spawn(async move {
        if let Some(mut pipe) = stdout_pipe {
            let mut buf = vec![0u8; 8192];
            loop {
                match pipe.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]);
                        dto_stdout.append(&chunk);
                    }
                    Err(_) => break,
                }
            }
        }
    });

    let dto_stderr = dto.clone();
    let stderr_handle = tokio::spawn(async move {
        let mut tail = String::new();
        if let Some(mut pipe) = stderr_pipe {
            let mut buf = vec![0u8; 8192];
            loop {
                match pipe.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                        dto_stderr.append(&chunk);
                        tail.push_str(&chunk);
                        if tail.len() > 4096 {
                            let cut = tail.len() - 4096;
                            tail.drain(..cut);
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        tail
    });

    let timeout_duration = Duration::from_millis(timeout_ms.max(0) as u64);
    let outcome = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            let _ = child.kill().await;
            WaitOutcome::Cancelled
        }
        result = child.wait() => match result {
            Ok(status) => WaitOutcome::Exited { code: status.code().unwrap_or(-1) },
            Err(e) => {
                warn!(target: "coco::task_runtime::shell", error = %e, "child wait failed");
                WaitOutcome::SpawnFailed
            }
        },
        () = tokio::time::sleep(timeout_duration) => {
            let _ = child.kill().await;
            WaitOutcome::TimedOut { budget_ms: timeout_ms }
        }
    };

    let _ = stdout_handle.await;
    // stderr tail is already on disk; the join result is intentionally
    // dropped — the terminal-apply step reads task state from
    // TaskManager, not from in-memory tails.
    let _ = stderr_handle.await;
    let _ = dto.flush().await;

    ShellOutcome { wait: outcome }
}

/// Final lifecycle update for a shell task: flip status,
/// broadcast on the watch, and push the TS-aligned terminal
/// notification.
#[allow(clippy::too_many_arguments)]
async fn apply_shell_terminal_state(
    manager: &TaskManager,
    status_tx: &watch::Sender<TaskStatus>,
    task_id: &str,
    description: &str,
    tool_use_id: Option<&str>,
    agent_id: Option<&str>,
    output_path: &str,
    sink: &dyn NotificationSink,
    outcome: ShellOutcome,
) {
    let (status, terminal, exit_code) = match outcome.wait {
        WaitOutcome::Exited { code: 0 } => {
            (TaskStatus::Completed, TerminalStatus::Completed, Some(0))
        }
        WaitOutcome::Exited { code } => {
            // Non-zero exit. TS treats any non-zero as failure
            // (`LocalShellTask.tsx:148-156`).
            (TaskStatus::Failed, TerminalStatus::Failed, Some(code))
        }
        WaitOutcome::TimedOut { budget_ms } => {
            // TS doesn't distinguish timeout from failed in the
            // status enum; coco-rs surfaces a clearer log line via
            // the budget but the status remains Failed for the model.
            warn!(
                target: "coco::task_runtime::shell",
                task_id,
                budget_ms,
                "shell task exceeded budget"
            );
            (TaskStatus::Failed, TerminalStatus::Failed, None)
        }
        WaitOutcome::Cancelled => (TaskStatus::Killed, TerminalStatus::Killed, None),
        WaitOutcome::SpawnFailed => (TaskStatus::Failed, TerminalStatus::Failed, None),
    };
    manager.update_status(task_id, status).await;
    status_tx.send_replace(status);

    let n = TaskNotification {
        task_id: task_id.to_string(),
        tool_use_id: tool_use_id.map(String::from),
        agent_id: agent_id.map(String::from),
        output_file: output_path.to_string(),
        description: description.to_string(),
        kind: NotificationKind::ShellTerminal {
            status: terminal,
            exit_code,
        },
    };
    sink.push(n).await;
    info!(
        target: "coco::task_runtime::shell",
        task_id,
        status = ?status,
        "background shell task terminal"
    );
}

/// Short identifier shown in `tracing` spans for shell commands.
/// Avoids logging the entire command which can be hundreds of
/// characters with heredocs / pipes.
fn command_preview(cmd: &str) -> String {
    truncate_for_label(cmd, 60)
}

fn truncate_for_label(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
#[path = "task_runtime.test.rs"]
mod tests;
