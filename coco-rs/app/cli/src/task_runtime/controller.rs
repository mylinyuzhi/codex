//! `TaskController` trait implementation — kill + detach.
//!
//! TS source: `tasks/stopTask.ts:38-100` (`stopTask`) +
//! `tasks/LocalAgentTask/LocalAgentTask.tsx:617-650`
//! (`backgroundAgentTask`).

use async_trait::async_trait;
use coco_tool_runtime::{DetachOutcome, TaskController};
use std::sync::atomic::Ordering;
use tracing::{debug, info, instrument};

use super::{TaskRuntime, boxed_msg};

impl TaskRuntime {
    /// W2: signal that a foreground awaiter should detach and let
    /// the task continue in the background. Idempotent — second and
    /// subsequent calls are no-ops (returns
    /// [`DetachOutcome::AlreadyDetached`]). Mirrors TS
    /// `backgroundAgentTask` / `shellCommand.background()`.
    ///
    /// Mechanics:
    /// 1. CAS the per-task `detached: AtomicBool` to true. If already
    ///    set, return [`DetachOutcome::AlreadyDetached`] immediately.
    /// 2. Mark `LocalAgentExtra.is_backgrounded = true` so the TUI
    ///    panel filter can hide the task from the fg list.
    /// 3. Call `detach.notify_one()` — wakes the fg `tool.execute`
    ///    `select!` arm awaiting `.notified()`.
    ///
    /// Returns [`DetachOutcome::Unknown`] for unknown task ids.
    #[instrument(level = "info", skip(self), fields(task_id = %task_id))]
    pub async fn signal_detach(&self, task_id: &str) -> DetachOutcome {
        let snapshot = {
            let entries = self.entries.read().await;
            entries
                .get(task_id)
                .map(|e| (e.detach.clone(), e.detached.clone()))
        };
        let Some((detach, detached)) = snapshot else {
            debug!(
                target: "coco::task_runtime",
                task_id,
                "signal_detach: unknown task id"
            );
            return DetachOutcome::Unknown;
        };
        // CAS gate. `swap` returns the *previous* value: if it was
        // already true, this is a no-op (TS parity:
        // `tasks/LocalAgentTask/LocalAgentTask.tsx:620-622`).
        if detached.swap(true, Ordering::SeqCst) {
            debug!(target: "coco::task_runtime", task_id, "signal_detach: already detached");
            return DetachOutcome::AlreadyDetached;
        }
        // Flip the sidecar flag. Only `LocalAgent` tasks have an
        // entry in the sparse map; for `LocalBash`, `set_backgrounded`
        // is a no-op on a missing entry (`LocalAgentExtra::default`).
        self.manager.set_backgrounded(task_id, true).await;
        detach.notify_one();
        info!(
            target: "coco::task_runtime",
            task_id,
            "signal_detach fired; fg awaiter will receive detach notification"
        );
        DetachOutcome::Detached
    }
}

#[async_trait]
impl TaskController for TaskRuntime {
    /// Kill a running task by firing its cancel token. **Does not**
    /// directly update status, broadcast on the watch, or push a
    /// `<task-notification>` — those are the driver's job, and
    /// doing them here would double-fire the SDK `TaskCompleted` event
    /// and the queued notification envelope.
    ///
    /// - **Shell tasks**: `cancel.cancel()` propagates into the child
    ///   process (`kill_on_drop=true`). The driver's `tokio::select!`
    ///   on `cancel.cancelled()` returns `WaitOutcome::Cancelled`, then
    ///   `apply_shell_terminal_state` runs the single
    ///   `update_status(Killed)` + `sink.push(ShellTerminal{Killed})`.
    /// - **Agent tasks**: the bg-agent closure in
    ///   `coordinator::spawn_background` races `cancel.cancelled()`
    ///   against `engine.execute_query`; on cancel it constructs an
    ///   `Err("task cancelled by leader")` and routes to `mark_failed`,
    ///   which pushes the single agent notification.
    ///
    /// TS parity: `LocalShellTask::killTask` (`tasks/LocalShellTask/LocalShellTask.tsx`)
    /// also only aborts the shell — the `.result.then(...)` handler is
    /// the single notification source.
    #[instrument(level = "info", skip(self), fields(task_id = %task_id))]
    async fn kill_task(&self, task_id: &str) -> Result<(), coco_error::BoxedError> {
        let cancel = {
            let entries = self.entries.read().await;
            entries.get(task_id).map(|e| e.cancel.clone())
        };
        let Some(cancel) = cancel else {
            return Err(boxed_msg(
                format!("No running task found with ID: {task_id}"),
                coco_error::StatusCode::FileNotFound,
            ));
        };
        cancel.cancel();
        info!(
            target: "coco::task_runtime",
            task_id,
            "kill_task fired cancel token; driver will finalize state + push notification"
        );
        Ok(())
    }

    async fn signal_detach(&self, task_id: &str) -> DetachOutcome {
        TaskRuntime::signal_detach(self, task_id).await
    }
}
