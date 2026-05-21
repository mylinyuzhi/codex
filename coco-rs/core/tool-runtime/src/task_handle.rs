//! Background-task callback traits — the seam between tools and the
//! task subsystem.
//!
//! TS source: `utils/task/framework.ts`, `tasks/LocalShellTask/`,
//! `tasks/LocalAgentTask/`, `tools/BashTool/BashTool.tsx`.
//!
//! ## Three narrow traits, one shared opaque type
//!
//! The previous version of this file lumped every background-task
//! operation onto a single `TaskHandle` trait covering spawn,
//! status, kill, list, and poll. After splitting we have three
//! traits with single responsibilities. The reader surface inspects
//! ([`TaskReader`]), the controller surface kills
//! ([`TaskController`]), the spawner surface launches shell tasks
//! ([`ShellTaskSpawner`]).
//!
//! [`BackgroundTaskHandle`] is a marker super-trait so a single
//! `Arc<dyn BackgroundTaskHandle>` can satisfy `ToolUseContext`
//! consumers that need every surface. Production impl
//! (`coco_cli::task_runtime::TaskRuntime`) blanket-implements it.
//!
//! ## Output / lifecycle / notification: where each concern lives
//!
//! - Status + per-task state — [`coco_types::TaskStateBase`] (the
//!   canonical wire shape). `TaskReader` returns it directly.
//! - On-disk output — `coco_cli::disk_task_output::DiskTaskOutput`
//!   (per-session disk file with 5 GB cap, TS-aligned).
//! - Notifications — [`coco_tasks::notification::NotificationSink`].
//!   `TaskRuntime` constructs `TaskNotification` payloads at
//!   terminal transitions and pushes through the sink; the
//!   app-layer impl (`CommandQueueNotificationSink`) wraps the
//!   `coco_query::CommandQueue`. Stall constants +
//!   `matches_interactive_prompt` live in
//!   `coco_tasks::stall` — this crate doesn't re-export them.
//!
//! ## What was deleted in this pass
//!
//! - `BackgroundTaskStatus` (4-variant projection of
//!   `coco_types::TaskStatus`) — readers now consume
//!   `TaskStateBase` directly. Single source of truth.
//! - `BackgroundTaskInfo` DTO — collapsed into `TaskStateBase`.
//! - `format_task_notification` / `format_stall_notification` —
//!   superseded by `coco_tasks::notification::render`.
//! - `StallInfo` / `TASK_NOTIFICATION_TAG` constant —
//!   `coco_tasks::notification` owns the rendering surface.
//! - `STALL_*` constants + `matches_interactive_prompt` — moved to
//!   `coco_tasks::stall`.
//! - `TaskHandle::poll_notifications` — push is the only path now.
//!   Pull was vestigial (no caller in the engine).

use coco_messages::Message;
use coco_types::TaskStateBase;
use std::sync::Arc;

/// Request to spawn a background shell task. Mirrors TS
/// `LocalShellSpawnInput` (`Task.ts:59-67`) modulo the
/// MONITOR_TOOL-specific `kind` field which is not ported yet.
#[derive(Clone)]
pub struct BackgroundShellRequest {
    pub command: String,
    /// Display label the model passes via tool input. TS:
    /// `BashTool.tsx:879` `description: description || command`.
    pub description: String,
    pub timeout_ms: Option<i64>,
    /// `tool_use_id` of the Bash invocation that started this task —
    /// threaded into the `<tool-use-id>` tag on the completion
    /// notification. TS: `BashTool.tsx:909` `toolUseId`.
    pub tool_use_id: Option<String>,
    /// Agent id of the subagent that issued the spawn. Routes the
    /// completion notification back to that agent's queue filter so
    /// teammates only see their own bg-task completions. TS:
    /// `BashTool.tsx:910` `agentId: toolUseContext.agentId`.
    pub agent_id: Option<String>,
    /// W3: when set, the TaskRuntime stdout-drain coroutine emits
    /// `bash_progress` events through this channel every
    /// [`Self::progress_throttle_ms`]. Mirrors TS `runShellCommand`'s
    /// `~1s` `yield { type: 'progress', ... }` cadence
    /// (`tools/BashTool/BashTool.tsx:1128-1140`). `None` = no
    /// progress emission (background-only spawns; bg drains to disk
    /// but doesn't need a live channel).
    pub progress_tx: Option<crate::traits::ProgressSender>,
    /// Throttle interval for `progress_tx` emission. Ignored when
    /// `progress_tx` is `None`. Default 1000 ms (matches TS's
    /// `POLL_INTERVAL_MS`).
    pub progress_throttle_ms: u64,
    /// W3: when set, the TaskRuntime fires `signal_detach(task_id)`
    /// internally after this many ms of foreground execution. Mirrors
    /// TS `ASSISTANT_BLOCKING_BUDGET_MS` (15 s) — used by `BashTool`
    /// in assistant mode to auto-detach long-running fg commands.
    /// `None` = never auto-detach (the caller's `tool.execute`
    /// `select!` arm is the only path that observes detach).
    pub auto_detach_ms: Option<u64>,
    /// W6: optional sandbox runtime state. When `Some` and the
    /// command isn't excluded by sandbox settings, the TaskRuntime
    /// driver wraps the child process via
    /// `SandboxState::try_wrap_command_with_binds` (bwrap / Seatbelt)
    /// before spawn. Mirrors `coco_shell::ExecOptions.sandbox`. `None`
    /// runs unsandboxed — backwards-compatible with sessions that
    /// don't wire sandbox state.
    pub sandbox_state: Option<Arc<coco_sandbox::SandboxState>>,
    /// W6: sandbox bypass requested via `dangerouslyDisableSandbox`
    /// tool input. Honored only when `sandbox_state.is_some()` and
    /// the session allows unsandboxed commands.
    pub sandbox_bypass: coco_sandbox::SandboxBypass,
}

impl std::fmt::Debug for BackgroundShellRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundShellRequest")
            .field("command", &self.command)
            .field("description", &self.description)
            .field("timeout_ms", &self.timeout_ms)
            .field("tool_use_id", &self.tool_use_id)
            .field("agent_id", &self.agent_id)
            .field("progress_tx", &self.progress_tx.is_some())
            .field("progress_throttle_ms", &self.progress_throttle_ms)
            .field("auto_detach_ms", &self.auto_detach_ms)
            .finish()
    }
}

/// Delta output from a background task (incremental read).
#[derive(Debug, Clone)]
pub struct TaskOutputDelta {
    pub content: String,
    pub new_offset: i64,
    pub is_complete: bool,
}

/// Terminal-state outputs for foreground `tool.execute` callers that
/// raced the task's terminal signal. Used by the unified fg/bg
/// execution path (W3): when `tool.execute`'s `select!` arm catches
/// the terminal signal, it calls
/// [`TaskReader::read_terminal_outputs`] to assemble a fg-shape
/// `ToolResult.data` (`{stdout, stderr, exitCode, interrupted}`).
///
/// TS parity: matches the shape of `runShellCommand`'s `ExecResult`
/// (`tools/BashTool/BashTool.tsx` returns `{stdout, stderr, code,
/// interrupted}` on completion). In coco-rs, stdout and stderr are
/// merged on disk (single fd, atomic appends — matching TS file
/// mode), so the impl returns the combined text on `stdout` and
/// leaves `stderr` empty. Callers should not branch on `stderr.is_empty()`
/// to detect success; use `exit_code` instead.
#[derive(Debug, Clone)]
pub struct TerminalOutputs {
    /// Combined stdout + stderr from the on-disk task file, truncated
    /// to `DEFAULT_MAX_READ_BYTES` (~64 KB).
    pub stdout: String,
    /// Empty in the unified path (stdout+stderr merged on disk). Kept
    /// in the type for future implementations that split streams.
    pub stderr: String,
    /// Exit code from the child process. `None` for non-process
    /// terminations (`Cancelled`, `SpawnFailed`, agent tasks).
    pub exit_code: Option<i32>,
    /// True iff the task ended via `kill_task` / cancel-token cascade.
    pub interrupted: bool,
}

/// Read-only inspection surface. Used by `TaskGet`, `TaskList`,
/// `TaskOutput`.
#[async_trait::async_trait]
pub trait TaskReader: Send + Sync {
    /// Snapshot a task's state. Returns `None` (via error) when the
    /// id is unknown.
    async fn get_task_status(&self, task_id: &str)
    -> Result<TaskStateBase, coco_error::BoxedError>;

    /// Read incremental output from a task's on-disk buffer.
    /// `from_offset` is the byte offset the caller last read; the
    /// returned `new_offset` is `from_offset + content.len()`. TS:
    /// `getTaskOutputDelta` (`utils/task/diskOutput.ts`).
    async fn get_task_output_delta(
        &self,
        task_id: &str,
        from_offset: i64,
    ) -> Result<TaskOutputDelta, coco_error::BoxedError>;

    /// Snapshot of every tracked task.
    async fn list_tasks(&self) -> Vec<TaskStateBase>;

    /// Subscribe to the task's terminal-status notification. The
    /// receiver fires exactly once when the task transitions to a
    /// terminal state (`Completed` / `Failed` / `Killed`). Returns
    /// `None` when the id is unknown.
    ///
    /// Replaces 100 ms polling in `TaskOutput` blocking reads with
    /// event-driven wait. Backed by `tokio::sync::watch::Sender`
    /// on the production impl — `watch` retains the last value, so
    /// late subscribers see a task that *already* finished without
    /// missing the signal.
    async fn subscribe_terminal(&self, task_id: &str) -> Option<TerminalSignal>;

    /// Subscribe to the per-task "detach" signal — used by the
    /// unified fg/bg execution path (W3) to race a terminal arm
    /// against a "move to background" arm in `tool.execute`. Returns
    /// `None` when the id is unknown.
    ///
    /// `Arc<Notify>` semantics: a single shared notification slot.
    /// Multiple awaiters of `.notified()` all wake on
    /// [`TaskController::signal_detach`]. Idempotency is enforced at
    /// the signal site via an internal `AtomicBool` — calling
    /// `signal_detach` twice on the same task is a no-op the second
    /// time, matching TS `backgroundAgentTask` (`tasks/LocalAgentTask/LocalAgentTask.tsx:617-650`)
    /// `if (task.isBackgrounded) return false`.
    ///
    /// TS analog: the per-agent `backgroundSignal` Promise stored in
    /// `backgroundSignalResolvers: Map<agentId, () => void>`
    /// (`tasks/LocalAgentTask/LocalAgentTask.tsx:526-614`); shell
    /// path: `shellCommand.background(taskId)` flag flip
    /// (`utils/ShellCommand.ts:349-366`).
    async fn detach_handle(&self, task_id: &str) -> Option<Arc<tokio::sync::Notify>>;

    /// Compose the terminal-state output frame for `tool.execute`
    /// callers that raced the terminal signal in fg mode. Reads up to
    /// `DEFAULT_MAX_READ_BYTES` of the merged stdout+stderr disk file,
    /// stamps exit code (when known) and `interrupted` flag.
    ///
    /// Errors when the task id is unknown. Returns content even when
    /// the task is still running (caller pre-condition: only call
    /// after `subscribe_terminal().await_terminal()` resolves).
    async fn read_terminal_outputs(
        &self,
        task_id: &str,
    ) -> Result<TerminalOutputs, coco_error::BoxedError>;
}

pub type TaskReaderRef = Arc<dyn TaskReader>;

/// One-shot terminal signal. The producer fires this when the task
/// reaches a terminal state; consumers `await` it. Implemented via
/// `tokio::sync::watch::Receiver` on the prod impl so the value is
/// observable even when subscription happens after the task ended.
pub struct TerminalSignal {
    rx: tokio::sync::watch::Receiver<coco_types::TaskStatus>,
}

impl TerminalSignal {
    pub fn new(rx: tokio::sync::watch::Receiver<coco_types::TaskStatus>) -> Self {
        Self { rx }
    }

    /// Wait until the task is in a terminal state, returning the
    /// final status. If the sender is dropped first, returns
    /// whatever the last observed status was.
    pub async fn await_terminal(mut self) -> coco_types::TaskStatus {
        // `wait_for` returns Ok(borrow) on success. If the sender
        // is dropped, return the current value as a best-effort
        // (the watch retains it).
        if let Ok(borrow) = self.rx.wait_for(|s| s.is_terminal()).await {
            return *borrow;
        }
        *self.rx.borrow()
    }
}

/// Outcome of a [`TaskController::signal_detach`] call. Self-documents at
/// the callsite vs an opaque `bool`. Per CLAUDE.md: "Avoid bool/ambiguous
/// params that produce opaque callsites".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetachOutcome {
    /// First call — the per-task `detached` CAS flipped true and the
    /// fg awaiter `Notify` was woken. The task is now backgrounded.
    Detached,
    /// Second-or-later call — the task was already in the detached
    /// state. No-op, matches TS `if (task.isBackgrounded) return false`
    /// at `tasks/LocalAgentTask/LocalAgentTask.tsx:620-622`.
    AlreadyDetached,
    /// Task id unknown to the runtime — no per-task entry exists.
    Unknown,
}

impl DetachOutcome {
    /// True only on the first successful detach. Convenient when the
    /// caller wants to log "we detached this task" once.
    pub fn is_first(self) -> bool {
        matches!(self, Self::Detached)
    }
}

/// Lifecycle control surface. Used by `TaskStop`.
#[async_trait::async_trait]
pub trait TaskController: Send + Sync {
    /// Kill a running task by firing its cancel token. Errors when
    /// the id is unknown. **Does not** update status or push a
    /// notification directly — the driver does that. See
    /// `app/cli/src/task_runtime.rs::kill_task` for the contract.
    async fn kill_task(&self, task_id: &str) -> Result<(), coco_error::BoxedError>;

    /// Signal that a task should detach from its foreground awaiter
    /// and continue running in the background.
    ///
    /// Mechanics:
    /// 1. Atomically flip the per-task "detached" flag. If already
    ///    detached, return [`DetachOutcome::AlreadyDetached`].
    /// 2. Update `LocalAgentExtra.is_backgrounded` (for the TUI panel
    ///    filter to differentiate fg vs detached tasks).
    /// 3. Notify the per-task [`tokio::sync::Notify`] returned by
    ///    [`TaskReader::detach_handle`]. The fg-mode `tool.execute`
    ///    `select!` arm awaiting `.notified()` wakes and returns a
    ///    `{task_id, status: "background"}` shape — the underlying
    ///    process / engine keeps running and will fire its own
    ///    terminal notification later.
    ///
    /// TS parity:
    /// - Shell: `backgroundExistingForegroundTask` (`tasks/LocalShellTask/LocalShellTask.tsx:420-470`)
    ///   → `shellCommand.background(taskId)` (`utils/ShellCommand.ts:349-366`).
    /// - Agent: `backgroundAgentTask` (`tasks/LocalAgentTask/LocalAgentTask.tsx:617-650`)
    ///   → resolves the per-agent `backgroundSignal` resolver.
    async fn signal_detach(&self, task_id: &str) -> DetachOutcome;
}

pub type TaskControllerRef = Arc<dyn TaskController>;

/// Shell-task spawn surface. Used by `BashTool` /
/// `PowerShellTool` background paths.
#[async_trait::async_trait]
pub trait ShellTaskSpawner: Send + Sync {
    /// Spawn a background shell task. Returns the task id
    /// immediately; the child process runs detached.
    async fn spawn_shell_task(
        &self,
        request: BackgroundShellRequest,
    ) -> Result<String, coco_error::BoxedError>;
}

pub type ShellTaskSpawnerRef = Arc<dyn ShellTaskSpawner>;

/// Marker super-trait — `Arc<dyn BackgroundTaskHandle>` is the
/// single value `ToolUseContext` carries. All three sub-traits are
/// implemented by the same struct (`TaskRuntime`).
pub trait BackgroundTaskHandle: TaskReader + TaskController + ShellTaskSpawner {}

impl<T> BackgroundTaskHandle for T where T: TaskReader + TaskController + ShellTaskSpawner + ?Sized {}

pub type BackgroundTaskHandleRef = Arc<dyn BackgroundTaskHandle>;

/// Registration side of the background-task surface.
///
/// `AgentTool`'s background path needs to register a freshly-
/// spawned engine task with the same store the read/control side
/// consumes. Kept as a separate trait so `coco-coordinator` can
/// depend on a narrow seam without dragging the entire
/// task-spawning surface into its dep set.
///
/// Production impl: `coco_cli::task_runtime::TaskRuntime`.
#[async_trait::async_trait]
pub trait AgentTaskRegistry: Send + Sync {
    /// Register a freshly-spawned AgentTool task that the parent will
    /// **synchronously await** (foreground). The task entry starts with
    /// `is_backgrounded = false`; an optional auto-detach timer can flip
    /// it to `true` later. TS parity: `registerAgentForeground`
    /// (`LocalAgentTask.tsx:526-614`).
    ///
    /// Returns the `task_id` the read/control trait accepts.
    ///
    /// - `tool_use_id`: the `Agent(...)` invocation's tool_use id from
    ///   the parent's `ToolUseContext.tool_use_id`. Threaded into the
    ///   `<tool-use-id>` tag on completion notifications so the model
    ///   can correlate the queued envelope with the original tool call.
    ///   TS parity: `AgentTool.tsx` passes `toolUseContext.toolUseId`
    ///   into `registerAgentForeground` / `registerAsyncAgent`.
    /// - `invoking_agent_id`: the agent that *called* AgentTool
    ///   (`ctx.agent_id`), NOT the newly-spawned subagent's id. Used
    ///   as the routing filter on `CommandQueue` so a teammate only
    ///   sees completion notifications for tasks IT spawned. TS
    ///   parity: `BashTool.tsx:910` passes `agentId: toolUseContext.agentId`
    ///   into the queued notification; agent path mirrors.
    async fn register_agent_task(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        invoking_agent_id: Option<&str>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> String;

    /// Register a fire-and-forget background AgentTool task. The task
    /// entry starts with `is_backgrounded = true` — the UI panel
    /// fg/bg filter and any post-compact reminder source see it as
    /// already-backgrounded. TS parity: `registerAsyncAgent`
    /// (`LocalAgentTask.tsx:466-515`) sets `isBackgrounded: true` at
    /// task creation.
    ///
    /// Default impl delegates to [`Self::register_agent_task`] so
    /// existing in-memory test implementations stay compatible (they
    /// don't differentiate fg/bg state); the production
    /// `coco_cli::task_runtime::TaskRuntime` overrides this to set
    /// `is_backgrounded: true` on the underlying `TaskStateBase`.
    async fn register_background_agent_task(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        invoking_agent_id: Option<&str>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> String {
        self.register_agent_task(description, tool_use_id, invoking_agent_id, cancel)
            .await
    }

    /// TS-parity variant that arms an auto-background timer for a
    /// foreground spawn. When `auto_background_ms = Some(ms)`, the
    /// runtime fires `signal_detach(tid)` after `ms` of execution if
    /// the task is still running. Default impl delegates to
    /// [`Self::register_agent_task`] (ignoring the timer) so existing
    /// implementations stay compatible.
    ///
    /// TS source: `LocalAgentTask.tsx:582-608 registerAgentForeground`
    /// `setTimeout(... autoBackgroundMs)`. Always foreground init
    /// (`is_backgrounded = false`); the timer flips it later.
    async fn register_agent_task_with_auto_background(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        invoking_agent_id: Option<&str>,
        cancel: tokio_util::sync::CancellationToken,
        _auto_background_ms: Option<u64>,
    ) -> String {
        self.register_agent_task(description, tool_use_id, invoking_agent_id, cancel)
            .await
    }

    /// Append text to a task's output buffer (typically a single
    /// content chunk from the streaming engine).
    async fn append_output(&self, task_id: &str, chunk: &str);

    /// Mark an agent task completed. The optional payload populates
    /// the `<result>` / `<usage>` / `<worktree>` sections of the
    /// `<task-notification>` envelope. TS:
    /// `LocalAgentTask.tsx:197-262` `enqueueAgentNotification`.
    async fn mark_completed(&self, task_id: &str, payload: AgentCompletionPayload);

    /// Mark an agent task failed with an error message.
    async fn mark_failed(&self, task_id: &str, error: &str);

    /// W4: transition an agent task to a terminal state WITHOUT
    /// pushing a `<task-notification>` envelope. Used by the sync
    /// AgentTool path where the result is returned to the parent
    /// tool call directly — pushing a notification would cause the
    /// model to see the same outcome twice (once via tool result,
    /// once via queued envelope). TS parity: sync agent path
    /// (`AgentTool.tsx:1054`) does **not** call
    /// `enqueueAgentNotification`; only the backgrounded path
    /// (`enqueueAgentNotification` in `tasks/LocalAgentTask/LocalAgentTask.tsx:197-262`)
    /// emits the envelope.
    ///
    /// `succeeded` selects between `TaskStatus::Completed` (true) and
    /// `TaskStatus::Failed` (false). Internally fires the per-task
    /// cancel token and broadcasts the terminal status on the watch
    /// channel — same as `mark_completed`/`mark_failed` would — but
    /// skips the sink push.
    async fn complete_silent(&self, task_id: &str, succeeded: bool);

    /// Register a memory-consolidation (auto-dream) task. Same
    /// lifecycle as agent tasks but with `TaskType::Dream` so the
    /// TUI panel + `TaskList` tool can differentiate background
    /// memory work from user-spawned subagents. No `tool_use_id` /
    /// `invoking_agent_id` because dream is a runtime-internal
    /// service that doesn't surface back to the model via a tool
    /// result. TS parity: `tasks/DreamTask/DreamTask.ts:72`
    /// `registerTask({type: 'dream', ...})`.
    async fn register_dream_task(
        &self,
        description: &str,
        cancel: tokio_util::sync::CancellationToken,
    ) -> String;

    /// W6.2 (full): expose the per-task detach `Notify` so the
    /// coordinator's sync-path engine driver can race `engine.execute_query`
    /// against an external `signal_detach`. Mirrors
    /// [`TaskReader::detach_handle`] but reachable through the
    /// registration-side `AgentTaskRegistry` trait so the
    /// coordinator doesn't need a separate `TaskReader` handle.
    /// Returns `None` when the id is unknown.
    async fn detach_handle(&self, task_id: &str) -> Option<std::sync::Arc<tokio::sync::Notify>>;

    /// Snapshot a task's accumulated output buffer (tail). Used by
    /// the periodic AgentSummary timer.
    async fn read_output(&self, task_id: &str) -> String;

    /// Path to the task's model-readable output file.
    async fn output_file_path(&self, _task_id: &str) -> Option<std::path::PathBuf> {
        None
    }

    /// Whether a task is in a terminal state.
    async fn is_terminal(&self, task_id: &str) -> bool;
}

pub type AgentTaskRegistryRef = Arc<dyn AgentTaskRegistry>;

/// Optional payload threaded into the agent-task completion
/// notification. `None` fields are omitted from the XML envelope —
/// TS parity at `LocalAgentTask.tsx:249-251` (`resultSection`,
/// `usageSection`, `worktreeSection` are template strings that
/// evaluate to `''` when the corresponding data is missing).
#[derive(Debug, Clone, Default)]
pub struct AgentCompletionPayload {
    /// Final response text from the subagent. TS: `finalMessage`
    /// → `<result>...</result>`.
    pub result: Option<String>,
    /// Token / tool-use / duration stats. TS: `usage`
    /// → `<usage>...</usage>`.
    pub usage: Option<AgentUsage>,
    /// Worktree info for `isolation: "worktree"` spawns. TS:
    /// `worktreePath` / `worktreeBranch` → `<worktree>...</worktree>`.
    pub worktree: Option<AgentWorktree>,
}

/// Usage block. Field shape mirrors TS `usage` argument to
/// `enqueueAgentNotification` (`LocalAgentTask.tsx:215-219`).
#[derive(Debug, Clone)]
pub struct AgentUsage {
    pub total_tokens: i64,
    pub tool_uses: i32,
    pub duration_ms: i64,
}

/// Worktree info for completion notifications.
#[derive(Debug, Clone)]
pub struct AgentWorktree {
    pub path: String,
    pub branch: Option<String>,
}

/// Sidecar metadata for a background AgentTool spawn. Mirrors TS
/// `AgentMetadata` (`utils/sessionStorage.ts:264-272`). Persisted
/// when the spawn registers; read on resume.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct AgentSpawnMetadata {
    /// Agent type used at original spawn. Resume routes against
    /// this when `subagent_type` is omitted.
    pub agent_type: String,
    /// Worktree path if the spawn used `isolation: "worktree"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    /// Original task description from the AgentTool input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Per-agent transcript persistence trait. Decouples
/// `coco-coordinator` (root layer) from `coco-session` (app layer)
/// without a layer-rule violation. Production impl lives in
/// `app/cli` and wraps `coco_session::TranscriptStore`.
#[async_trait::async_trait]
pub trait AgentTranscriptStore: Send + Sync {
    /// Append `messages` to the per-agent JSONL transcript.
    /// Idempotent across multiple calls.
    async fn append_agent_messages(
        &self,
        session_id: &str,
        agent_id: &str,
        messages: &[Arc<Message>],
    ) -> Result<(), coco_error::BoxedError>;

    /// Read every persisted message for an agent in conversation
    /// order. Returns `Ok(None)` when no transcript exists.
    async fn load_agent_messages(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> Result<Option<Vec<Arc<Message>>>, coco_error::BoxedError>;

    /// Write the metadata sidecar for an agent.
    async fn write_agent_metadata(
        &self,
        session_id: &str,
        agent_id: &str,
        metadata: &AgentSpawnMetadata,
    ) -> Result<(), coco_error::BoxedError>;

    /// Read the metadata sidecar; `Ok(None)` when no sidecar exists.
    async fn read_agent_metadata(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> Result<Option<AgentSpawnMetadata>, coco_error::BoxedError>;
}

pub type AgentTranscriptStoreRef = Arc<dyn AgentTranscriptStore>;

/// No-op implementation for contexts without background tasks.
/// Used by `ToolUseContext` in tests and headless sessions.
#[derive(Debug, Clone, Default)]
pub struct NoOpBackgroundTaskHandle;

#[async_trait::async_trait]
impl TaskReader for NoOpBackgroundTaskHandle {
    async fn get_task_status(&self, _: &str) -> Result<TaskStateBase, coco_error::BoxedError> {
        Err(unavail())
    }
    async fn get_task_output_delta(
        &self,
        _: &str,
        _: i64,
    ) -> Result<TaskOutputDelta, coco_error::BoxedError> {
        Err(unavail())
    }
    async fn list_tasks(&self) -> Vec<TaskStateBase> {
        vec![]
    }
    async fn subscribe_terminal(&self, _: &str) -> Option<TerminalSignal> {
        None
    }
    async fn detach_handle(&self, _: &str) -> Option<Arc<tokio::sync::Notify>> {
        None
    }
    async fn read_terminal_outputs(
        &self,
        _: &str,
    ) -> Result<TerminalOutputs, coco_error::BoxedError> {
        Err(unavail())
    }
}

#[async_trait::async_trait]
impl TaskController for NoOpBackgroundTaskHandle {
    async fn kill_task(&self, _: &str) -> Result<(), coco_error::BoxedError> {
        Err(unavail())
    }
    async fn signal_detach(&self, _: &str) -> DetachOutcome {
        DetachOutcome::Unknown
    }
}

#[async_trait::async_trait]
impl ShellTaskSpawner for NoOpBackgroundTaskHandle {
    async fn spawn_shell_task(
        &self,
        _: BackgroundShellRequest,
    ) -> Result<String, coco_error::BoxedError> {
        Err(unavail())
    }
}

fn unavail() -> coco_error::BoxedError {
    Box::new(coco_error::PlainError::new(
        "Background tasks not available in this context",
        coco_error::StatusCode::Internal,
    ))
}
