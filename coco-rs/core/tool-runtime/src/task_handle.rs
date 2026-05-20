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
#[derive(Debug, Clone)]
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
}

/// Delta output from a background task (incremental read).
#[derive(Debug, Clone)]
pub struct TaskOutputDelta {
    pub content: String,
    pub new_offset: i64,
    pub is_complete: bool,
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

/// Lifecycle control surface. Used by `TaskStop`.
#[async_trait::async_trait]
pub trait TaskController: Send + Sync {
    /// Kill a running task. Fires the per-task cancel token and
    /// flips status to `Killed`. Errors when the id is unknown.
    async fn kill_task(&self, task_id: &str) -> Result<(), coco_error::BoxedError>;
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
    /// Register a freshly-spawned background AgentTool task.
    /// Returns the `task_id` the read/control trait accepts.
    async fn register_agent_task(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> String;

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
}

#[async_trait::async_trait]
impl TaskController for NoOpBackgroundTaskHandle {
    async fn kill_task(&self, _: &str) -> Result<(), coco_error::BoxedError> {
        Err(unavail())
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
