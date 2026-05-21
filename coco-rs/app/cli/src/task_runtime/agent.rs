//! Agent task lifecycle — registration, output drain, terminal
//! transitions, completion notifications.
//!
//! TS source:
//! - `tasks/LocalAgentTask/LocalAgentTask.tsx` (lifecycle + state).
//! - `tasks/DreamTask/DreamTask.ts` (dream task variant).
//! - `LocalAgentTask.tsx:197-262 enqueueAgentNotification`
//!   (terminal `<task-notification>` envelope with `<result>` /
//!   `<usage>` / `<worktree>` sections).
//! - `LocalAgentTask.tsx:466-515 registerAsyncAgent`
//!   (fire-and-forget bg spawn).
//! - `LocalAgentTask.tsx:526-614 registerAgentForeground`
//!   (sync spawn with optional autoBackgroundMs timer).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use async_trait::async_trait;
use coco_tasks::{
    NotificationKind, TaskNotification, TaskUsage as NotifTaskUsage, TerminalStatus,
    Worktree as NotifWorktree,
};
use coco_tool_runtime::{AgentCompletionPayload, AgentRegistration, AgentTaskRegistry};
use coco_types::{TaskStatus, TaskType};
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;
use tracing::{info, instrument, trace, warn};

use super::stall;
use super::timers::spawn_agent_auto_background_timer;
use super::{TaskEntry, TaskRuntime};
use crate::disk_task_output::DEFAULT_MAX_READ_BYTES;

impl TaskRuntime {
    /// Register a freshly-spawned AgentTool task. The `registration`
    /// parameter selects the initial fg/bg state and the optional
    /// auto-detach timer (see [`AgentRegistration`]).
    ///
    /// Mints the id, resolves the disk path, inserts as `Running` with
    /// one lifecycle event, and stores per-task control state (cancel
    /// token + watch + invoking agent id). On
    /// `ForegroundWithAutoDetach`, spawns the per-task auto-detach
    /// timer in `timers::spawn_agent_auto_background_timer`.
    ///
    /// TS-parity mapping documented on [`AgentRegistration`].
    #[instrument(
        level = "info",
        skip(self, cancel),
        fields(description = %description, tool_use_id = ?tool_use_id, invoking_agent_id = ?invoking_agent_id, ?registration)
    )]
    pub async fn register_agent_task(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        invoking_agent_id: Option<&str>,
        cancel: CancellationToken,
        registration: AgentRegistration,
    ) -> String {
        let (is_backgrounded, auto_background_ms) = registration.decompose();
        self.register_agent_task_inner(
            description,
            tool_use_id,
            invoking_agent_id,
            cancel,
            auto_background_ms,
            is_backgrounded,
        )
        .await
    }

    /// Internal helper that owns the actual registration steps. The
    /// public entry point [`Self::register_agent_task`] decomposes
    /// the `AgentRegistration` enum into the two underlying axes
    /// before calling this.
    async fn register_agent_task_inner(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        invoking_agent_id: Option<&str>,
        cancel: CancellationToken,
        auto_background_ms: Option<u64>,
        is_backgrounded: bool,
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
                is_backgrounded,
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
        self.entries.write().await.insert(
            task_id.clone(),
            TaskEntry {
                cancel: cancel.clone(),
                status_tx,
                invoking_agent_id: invoking_agent_id.map(String::from),
                detach: Arc::new(Notify::new()),
                detached: Arc::new(AtomicBool::new(false)),
                exit_code: Arc::new(std::sync::OnceLock::new()),
            },
        );
        // W6 (Item 3 / A4): spawn the agent stall watchdog. Fires a
        // notification if the agent's disk output is silent for
        // `AGENT_STALL_THRESHOLD_MS`. The bg agent driver in
        // `coordinator::spawn_background` drains `Stream::TextDelta`
        // into `append_output` so disk-size growth is a faithful
        // proxy for "agent is still working". Cancel propagates from
        // the task entry's cancel token, so terminal transitions /
        // `kill_task` stop the watchdog cleanly.
        tokio::spawn(stall::agent_watchdog(
            task_id.clone(),
            description.to_string(),
            tool_use_id.map(String::from),
            invoking_agent_id.map(String::from),
            output_path.clone(),
            dto.clone(),
            self.notification_sink.clone(),
            cancel.clone(),
        ));
        // TS parity (`LocalAgentTask.tsx:582-608`): when `autoBackgroundMs`
        // is set, the foreground sub-agent auto-detaches after that
        // many ms of execution. The fg awaiter (`tool.execute`'s
        // `select!` arm) gets the detach signal and unblocks with
        // `AsyncLaunched`; the engine keeps running detached.
        if let Some(ms) = auto_background_ms.filter(|v| *v > 0) {
            spawn_agent_auto_background_timer(
                task_id.clone(),
                ms,
                self.entries.clone(),
                self.manager.clone(),
                cancel,
            );
        }
        info!(
            target: "coco::task_runtime",
            task_id = %task_id,
            task_type = "local_agent",
            output_file = %output_path,
            auto_background_ms = ?auto_background_ms,
            "agent task registered (Running, stall watchdog spawned)"
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
        // Record the error text on the sidecar so the post-compact
        // `task_status` reminder can surface it as `delta_summary` for
        // failed terminal tasks. TS parity:
        // `compact.ts:1591-1594` reads `agent.error` for terminal-status
        // tasks; `messages.ts:4005-4006` renders `"Delta: <error>"`.
        self.manager.set_error(task_id, error.to_string()).await;
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

    pub(super) async fn transition_terminal(&self, task_id: &str, status: TaskStatus) {
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
    /// canonical state (TaskManager) and the routing `invoking_agent_id`
    /// from the per-task entry; then push the agent-shaped
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
        let invoking_agent_id = self
            .entries
            .read()
            .await
            .get(task_id)
            .and_then(|e| e.invoking_agent_id.clone());
        let n = TaskNotification {
            task_id: state.id,
            tool_use_id: state.tool_use_id,
            agent_id: invoking_agent_id,
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
        invoking_agent_id: Option<&str>,
        cancel: CancellationToken,
        registration: AgentRegistration,
    ) -> String {
        TaskRuntime::register_agent_task(
            self,
            description,
            tool_use_id,
            invoking_agent_id,
            cancel,
            registration,
        )
        .await
    }
    async fn append_output(&self, task_id: &str, chunk: &str) {
        TaskRuntime::append_output(self, task_id, chunk).await
    }
    async fn set_progress_summary(&self, task_id: &str, summary: String) {
        // Delegate to the inner manager — it owns the change-detect
        // logic (D7) and the per-emit SDK gate (D14).
        self.manager.set_progress_summary(task_id, summary).await;
    }
    async fn set_progress(&self, task_id: &str, progress: coco_types::TaskProgress) {
        // Delegate to TaskManager which preserves any existing summary
        // across overlapping writes (`updateAgentProgress` TS parity).
        self.manager.set_progress(task_id, progress).await;
    }
    async fn mark_completed(&self, task_id: &str, payload: AgentCompletionPayload) {
        TaskRuntime::mark_completed(self, task_id, payload).await
    }
    async fn mark_failed(&self, task_id: &str, error: &str) {
        TaskRuntime::mark_failed(self, task_id, error).await
    }
    async fn complete_silent(&self, task_id: &str, succeeded: bool) {
        let status = if succeeded {
            TaskStatus::Completed
        } else {
            TaskStatus::Failed
        };
        self.transition_terminal(task_id, status).await;
        info!(
            target: "coco::task_runtime",
            task_id,
            ?status,
            "complete_silent: terminal transition without notification (W4 sync path)"
        );
    }

    async fn detach_handle(&self, task_id: &str) -> Option<Arc<Notify>> {
        // W6.2 full: same data as TaskReader's impl; expose via
        // AgentTaskRegistry so the coordinator can race detach
        // without holding a separate TaskReader handle.
        coco_tool_runtime::TaskReader::detach_handle(self, task_id).await
    }

    async fn register_dream_task(&self, description: &str, cancel: CancellationToken) -> String {
        // Mint a Dream-prefixed task_id so `TaskList` / TUI can
        // identify auto-dream entries at a glance.
        let task_id = coco_types::generate_task_id(TaskType::Dream);
        let dto = self.disk.get_or_create(&task_id).await;
        let output_path = dto.path().display().to_string();
        // Dream tasks run as internal services (no user-visible
        // foreground awaiter) — initialize as backgrounded so the TUI
        // panel filter and fg/bg-discriminating consumers treat them
        // consistently with `registerAsyncAgent`.
        let assigned = self
            .manager
            .create_running_with_id(
                task_id.clone(),
                TaskType::Dream,
                description,
                &output_path,
                /* is_backgrounded */ true,
            )
            .await;
        debug_assert_eq!(assigned, task_id);
        let (status_tx, _) = watch::channel(TaskStatus::Running);
        self.entries.write().await.insert(
            task_id.clone(),
            TaskEntry {
                cancel,
                status_tx,
                // Dream is internal — no invoker agent, no detach
                // mechanism (it's not user-cancellable mid-run via
                // Ctrl+B; `kill_task` is the only stop path).
                invoking_agent_id: None,
                detach: Arc::new(Notify::new()),
                detached: Arc::new(AtomicBool::new(false)),
                exit_code: Arc::new(std::sync::OnceLock::new()),
            },
        );
        info!(
            target: "coco::task_runtime",
            task_id = %task_id,
            task_type = "dream",
            output_file = %output_path,
            "auto-dream task registered (Running)"
        );
        task_id
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
