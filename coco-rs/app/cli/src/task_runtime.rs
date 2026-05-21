//! Production background-task runtime.
//!
//! `TaskRuntime` implements four traits that the tool layer + the
//! coordinator both consume:
//!
//! - [`coco_tool_runtime::TaskReader`] — `TaskGet` / `TaskList` /
//!   `TaskOutput` read paths. Implemented in [`reader`].
//! - [`coco_tool_runtime::TaskController`] — `TaskStop`. Implemented
//!   in [`controller`].
//! - [`coco_tool_runtime::ShellTaskSpawner`] — Bash / PowerShell
//!   `run_in_background`. Implemented in [`shell`].
//! - [`coco_tool_runtime::AgentTaskRegistry`] — `SwarmAgentHandle`'s
//!   background AgentTool dispatch. Implemented in [`agent`].
//!
//! ## Where each concern lives
//!
//! - Lifecycle state — [`coco_tasks::TaskManager`].
//! - Disk-backed output — [`crate::disk_task_output::DiskOutputs`].
//! - Per-task cancel + terminal-status broadcast — [`TaskEntry`]
//!   below, indexed by task id in `entries`.
//! - Notification XML construction + push — done in [`agent`] / [`shell`]
//!   using [`coco_tasks::notification`] primitives. The sink
//!   (`Arc<dyn NotificationSink>`) is always wired; tests use
//!   `NoOpNotificationSink` (default), production wires
//!   [`crate::command_queue_sink::CommandQueueNotificationSink`].
//! - Stall watchdog — [`stall::watchdog`] / [`stall::agent_watchdog`]
//!   spawned per task.
//! - Auto-background / auto-detach / progress timers — [`timers`].
//!
//! ## Module map (TS counterpart)
//!
//! | Submodule        | TS source                                                       |
//! |------------------|-----------------------------------------------------------------|
//! | [`agent`]        | `tasks/LocalAgentTask/LocalAgentTask.tsx`                       |
//! | [`shell`]        | `tasks/LocalShellTask/LocalShellTask.tsx` + `killShellTasks.ts` |
//! | [`reader`]       | `utils/task/framework.ts` (read side) + `diskOutput.ts`         |
//! | [`controller`]   | `tasks/stopTask.ts` (kill) + `LocalAgentTask.tsx:617-650 backgroundAgentTask` + `utils/ShellCommand.ts:349-366` (detach) |
//! | [`timers`]       | `LocalAgentTask.tsx:582-608` (autoBackgroundMs setTimeout)      |
//! | [`stall`]        | `LocalShellTask.tsx:46-104` (startStallWatchdog)                |
//!
//! ## D8-narrow (deferred) — collapse the dual-store split
//!
//! The current architecture keys per-task state across two locks
//! that this struct holds independently:
//!
//! - `TaskRuntime.entries: HashMap<id, TaskEntry>` — per-task
//!   control state (cancel token, watch sender, detach Notify,
//!   detached AtomicBool, exit_code OnceLock).
//! - `coco_tasks::TaskManager.tasks: HashMap<id, TaskStateBase>` —
//!   lifecycle state (status, output_file, start/end time, extras).
//!
//! Any inconsistency between the two is a latent bug source. The
//! adversarial review (D8) recommends collapsing into one store
//! owned by `TaskManager`:
//!
//! 1. Add `tokio-util = { features = ["rt"] }` to `coco-tasks` deps
//!    (needed for `CancellationToken`).
//! 2. Move [`TaskEntry`] from this file into `coco-tasks::running`
//!    (rename to `TaskControl` for clarity).
//! 3. Replace `TaskManager.tasks: HashMap<id, TaskStateBase>` with
//!    `HashMap<id, TaskRow { base: TaskStateBase, control: TaskControl }>`
//!    so both halves move atomically under one lock.
//! 4. Drop `TaskRuntime.entries` and route every read/write through
//!    `self.manager.with_control(id, |c| ...)`-style accessors on
//!    `TaskManager`.
//! 5. Adjust the ~15 callsites in `agent.rs` / `shell.rs` /
//!    `reader.rs` / `controller.rs` / `timers.rs`.
//!
//! Estimated effort: L (multi-PR). Tracked as TS-parity-neutral
//! architectural cleanup — current behavior is correct, this just
//! eliminates the cross-lock surface.

mod agent;
mod controller;
mod reader;
mod shell;
mod stall;
mod timers;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use coco_tasks::{NoOpNotificationSink, NotificationSinkRef, TaskManager};
use coco_types::TaskStatus;
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::disk_task_output::DiskOutputs;

/// Per-task control state.
///
/// `cancel` fires the kill path. `status_tx` broadcasts terminal
/// transitions so `TaskOutput` blocking reads (and any future
/// observer) can `await` instead of polling. `watch` retains the
/// last value, so a subscriber that arrives after the task ended
/// still sees the terminal status.
///
/// `invoking_agent_id` is the routing filter on `CommandQueue` for
/// terminal notifications — it's the agent that *called* the tool
/// that created this task (`ctx.agent_id`), NOT a generated subagent
/// id. Stored here (rather than re-read from `TaskManager`) because
/// `TaskStateBase` carries `tool_use_id` but not `agent_id`. TS
/// parity: `BashTool.tsx:910` / `AgentTool.tsx` thread
/// `toolUseContext.agentId` through to the notification.
///
/// `detach` is the per-task one-shot "move to background" signal
/// (W2). `tool.execute` in fg mode `select!`s on `.notified()`. The
/// adjacent `detached` flag is the CAS gate that makes
/// [`TaskRuntime::signal_detach`](controller) idempotent — mirrors TS
/// `backgroundAgentTask`'s `if (task.isBackgrounded) return false`
/// (`tasks/LocalAgentTask/LocalAgentTask.tsx:620-622`).
pub(in crate::task_runtime) struct TaskEntry {
    pub(in crate::task_runtime) cancel: CancellationToken,
    pub(in crate::task_runtime) status_tx: watch::Sender<TaskStatus>,
    pub(in crate::task_runtime) invoking_agent_id: Option<String>,
    pub(in crate::task_runtime) detach: Arc<Notify>,
    pub(in crate::task_runtime) detached: Arc<AtomicBool>,
    /// Set once by the shell driver in `apply_shell_terminal_state`
    /// for `Exited` outcomes. `None` for agent tasks and shell
    /// outcomes lacking a process exit (`Cancelled` / `SpawnFailed` /
    /// `TimedOut`). Read by [`reader::TaskReader::read_terminal_outputs`]
    /// to compose the fg `ToolResult.data` `exitCode` field.
    pub(in crate::task_runtime) exit_code: Arc<std::sync::OnceLock<i32>>,
}

/// Production task runtime.
///
/// Cheap to clone (every field is `Arc`). Construction happens once
/// per session in CLI bootstrap; the same `Arc<Self>` flows into the
/// engine (read/control) and into `SwarmAgentHandle` (registration).
pub struct TaskRuntime {
    pub(in crate::task_runtime) manager: Arc<TaskManager>,
    pub(in crate::task_runtime) entries: Arc<tokio::sync::RwLock<HashMap<String, TaskEntry>>>,
    pub(in crate::task_runtime) disk: Arc<DiskOutputs>,
    /// Always wired. `NoOpNotificationSink` is the default when no
    /// producer attaches — terminal events are silently dropped,
    /// matching TS sessions that run without a turn loop (headless
    /// jobs / `--bare` SDK). Production attaches the
    /// `CommandQueueNotificationSink`.
    pub(in crate::task_runtime) notification_sink: NotificationSinkRef,
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
}

/// Build a [`coco_error::BoxedError`] from a message + status code.
/// Shared across the four trait impls — kept here so each submodule
/// uses the same wrapping. Visible only inside `task_runtime::*`.
pub(in crate::task_runtime) fn boxed_msg(
    msg: impl Into<String>,
    code: coco_error::StatusCode,
) -> coco_error::BoxedError {
    Box::new(coco_error::PlainError::new(msg, code))
}

#[cfg(test)]
#[path = "task_runtime.test.rs"]
mod tests;
