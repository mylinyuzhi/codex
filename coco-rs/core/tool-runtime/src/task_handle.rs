//! Background task handle trait — abstraction for background task operations from tools.
//!
//! TS: `utils/task/framework.ts`, `tasks/LocalShellTask/LocalShellTask.tsx`
//!
//! Enables tools to spawn background tasks (shell commands, agents) and
//! manage their lifecycle. Task notifications are injected as XML messages
//! when tasks complete.
//!
//! Stall detection: Implementations poll output files every 5s and trigger
//! a notification if output hasn't changed for 45s AND the last line matches
//! an interactive prompt pattern. Stall is a notification event, NOT a status
//! transition — the task remains Running.

use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;

/// Stall detection check interval.
///
/// TS: `STALL_CHECK_INTERVAL_MS = 5_000`
pub const STALL_CHECK_INTERVAL_MS: u64 = 5_000;

/// Stall detection threshold — output frozen for this long triggers notification.
///
/// TS: `STALL_THRESHOLD_MS = 45_000`
pub const STALL_THRESHOLD_MS: u64 = 45_000;

/// Number of tail bytes to read for prompt pattern detection.
///
/// TS: `STALL_TAIL_BYTES = 1024`
pub const STALL_TAIL_BYTES: usize = 1024;

/// Status of a background task.
///
/// Note: TS has no "stalled" status. Stall is a notification event,
/// not a status transition. A stalled task remains Running until it
/// completes, fails, or is killed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskStatus {
    Running,
    Completed,
    Failed,
    Killed,
}

/// Info about a background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskInfo {
    pub task_id: String,
    pub status: BackgroundTaskStatus,
    pub summary: Option<String>,
    pub output_file: Option<String>,
    /// Tool use ID that spawned this task (for notification targeting).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// Elapsed time in seconds since task started.
    #[serde(default)]
    pub elapsed_seconds: f64,
    /// Whether a notification was already sent for this task.
    /// Prevents duplicate notifications on repeated polling.
    #[serde(default)]
    pub notified: bool,
}

/// Request to spawn a background shell task.
#[derive(Debug, Clone)]
pub struct BackgroundShellRequest {
    pub command: String,
    pub timeout_ms: Option<i64>,
    pub description: Option<String>,
}

/// Delta output from a background task (incremental read).
#[derive(Debug, Clone)]
pub struct TaskOutputDelta {
    pub content: String,
    pub new_offset: i64,
    pub is_complete: bool,
}

/// Stall detection result.
///
/// TS: stall watchdog checks if output is frozen and last line matches prompt.
#[derive(Debug, Clone)]
pub struct StallInfo {
    pub task_id: String,
    /// Tail of the output (last STALL_TAIL_BYTES bytes).
    pub output_tail: String,
    /// How long the output has been frozen (seconds).
    pub frozen_seconds: f64,
}

/// XML task notification tag for injecting task results into conversation.
pub const TASK_NOTIFICATION_TAG: &str = "task-notification";

/// Format a task completion notification as XML for model consumption.
///
/// TS: `enqueueShellNotification()` — includes status tag for completion.
pub fn format_task_notification(info: &BackgroundTaskInfo) -> String {
    let status = match info.status {
        BackgroundTaskStatus::Running => "running",
        BackgroundTaskStatus::Completed => "completed",
        BackgroundTaskStatus::Failed => "failed",
        BackgroundTaskStatus::Killed => "killed",
    };

    let mut xml = format!(
        "<{TASK_NOTIFICATION_TAG}>\n<task-id>{}</task-id>\n",
        info.task_id
    );

    if let Some(tool_use_id) = &info.tool_use_id {
        xml.push_str(&format!("<tool-use-id>{tool_use_id}</tool-use-id>\n"));
    }
    if let Some(output_file) = &info.output_file {
        xml.push_str(&format!("<output-file>{output_file}</output-file>\n"));
    }
    xml.push_str(&format!("<status>{status}</status>\n"));
    if let Some(summary) = &info.summary {
        xml.push_str(&format!("<summary>{summary}</summary>\n"));
    }

    xml.push_str(&format!("</{TASK_NOTIFICATION_TAG}>"));
    xml
}

/// Format a stall notification as XML for model consumption.
///
/// TS: Stall notifications intentionally OMIT the `<status>` tag.
/// From TS comment: "No <status> tag — print.ts treats <status> as a terminal
/// signal and an unknown value falls through to 'completed', falsely closing
/// the task for SDK consumers."
///
/// The output_tail is included OUTSIDE the XML tags as raw text so the
/// model can see what the command is waiting for.
pub fn format_stall_notification(stall: &StallInfo, output_file: Option<&str>) -> String {
    let mut xml = format!(
        "<{TASK_NOTIFICATION_TAG}>\n<task-id>{}</task-id>\n",
        stall.task_id
    );

    if let Some(path) = output_file {
        xml.push_str(&format!("<output-file>{path}</output-file>\n"));
    }
    xml.push_str(&format!(
        "<summary>Task appears to be waiting for input (output frozen for {:.0}s)</summary>\n",
        stall.frozen_seconds
    ));

    xml.push_str(&format!("</{TASK_NOTIFICATION_TAG}>\n"));

    // Raw output tail outside XML so model can see prompt
    if !stall.output_tail.is_empty() {
        xml.push_str(&stall.output_tail);
    }

    xml
}

/// Check if the last line of output tail matches an interactive prompt pattern.
///
/// TS: `looksLikePrompt()` — checks only the LAST line of the tail,
/// uses regex patterns for context-aware questions.
///
/// Only the last line is checked to avoid false positives from prompt-like
/// strings appearing in normal output above the current prompt.
pub fn matches_interactive_prompt(tail: &str) -> bool {
    // Extract the last non-empty line
    let last_line = tail.trim_end().rsplit('\n').next().unwrap_or("").trim();

    if last_line.is_empty() {
        return false;
    }

    let lower = last_line.to_lowercase();

    // Simple string patterns (exact TS patterns)
    let string_patterns = [
        "(y/n)",
        "[y/n]",
        "y/n",
        "(yes/no)",
        "[yes/no]",
        "yes/no",
        "password:",
        "passphrase:",
        "[sudo]",
        "enter passphrase",
    ];

    if string_patterns.iter().any(|p| lower.contains(p)) {
        return true;
    }

    // Directed question patterns (TS regex equivalent):
    // /\b(?:Do you|Would you|Shall I|Are you sure|Ready to)\b.*\? *$/i
    let question_prefixes = ["do you", "would you", "shall i", "are you sure", "ready to"];
    if (lower.ends_with('?') || lower.ends_with("? "))
        && question_prefixes.iter().any(|p| lower.contains(p))
    {
        return true;
    }

    // Action prompts ending with ?
    // /Continue\?/i, /Overwrite\?/i, /Proceed\?/i
    let action_prompts = ["continue?", "overwrite?", "proceed?"];
    if action_prompts.iter().any(|p| lower.contains(p)) {
        return true;
    }

    // Press prompts: /Press (any key|Enter)/i
    if lower.contains("press any key") || lower.contains("press enter") {
        return true;
    }

    false
}

/// Trait for background task operations from tools.
///
/// Implementations must:
/// 1. Spawn tasks and track their state (Running/Completed/Failed/Killed)
/// 2. Run stall detection internally:
///    - Poll every `STALL_CHECK_INTERVAL_MS` (5s) for each running task
///    - Track output file mtime and size
///    - If output frozen for `STALL_THRESHOLD_MS` (45s) AND last line
///      matches `matches_interactive_prompt()`, enqueue a stall notification
///    - Stall detection is one-shot per frozen period: reset when output grows
/// 3. Enqueue notifications via `poll_notifications()` — both completions and stalls
/// 4. Persist output to disk for large outputs (max 8MB per delta read)
/// 5. Track `notified` flag per task to prevent duplicate notifications
#[async_trait::async_trait]
pub trait TaskHandle: Send + Sync {
    /// Spawn a background shell task.
    /// Returns the task ID immediately.
    async fn spawn_shell_task(&self, request: BackgroundShellRequest) -> anyhow::Result<String>;

    /// Get the status of a background task.
    async fn get_task_status(&self, task_id: &str) -> anyhow::Result<BackgroundTaskInfo>;

    /// Read incremental output from a background task.
    ///
    /// TS: `getTaskOutputDelta(taskId, fromOffset, maxBytes)` — max 8MB per read.
    async fn get_task_output_delta(
        &self,
        task_id: &str,
        from_offset: i64,
    ) -> anyhow::Result<TaskOutputDelta>;

    /// Kill a running background task.
    async fn kill_task(&self, task_id: &str) -> anyhow::Result<()>;

    /// List all active background tasks.
    async fn list_tasks(&self) -> Vec<BackgroundTaskInfo>;

    /// Poll tasks for completion and stall events.
    /// Returns tasks that have notifications pending (completions + stalls).
    ///
    /// TS: `pollTasks()` — called periodically by framework.
    /// Implementations run stall detection internally during this call.
    async fn poll_notifications(&self) -> Vec<BackgroundTaskInfo>;
}

pub type TaskHandleRef = Arc<dyn TaskHandle>;

/// Registration side of the background-task surface.
///
/// AgentTool's background path needs to register a freshly-spawned
/// engine task with the same store the read/control side
/// ([`TaskHandle`]) consumes. Splitting registration out of
/// `TaskHandle` keeps the trait shape that `coco-tools` already
/// targets unchanged while letting `coco-coordinator` reach the
/// registry through a narrow seam without depending on the CLI
/// crate where the production impl lives.
///
/// Production impl: `coco_cli::task_runtime::TaskRuntime`.
#[async_trait::async_trait]
pub trait AgentTaskRegistry: Send + Sync {
    /// Register a freshly-spawned background AgentTool task.
    ///
    /// Returns the `task_id` that subsequent
    /// [`TaskHandle::get_task_status`] / `get_task_output_delta` /
    /// `kill_task` calls accept. The implementation owns the per-
    /// task cancellation token (handed back to the caller for
    /// observation), output buffer, and lifecycle status.
    async fn register_agent_task(
        &self,
        description: &str,
        tool_use_id: Option<&str>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> String;

    /// Append text to a task's output buffer.
    async fn append_output(&self, task_id: &str, chunk: &str);

    /// Mark a task completed and stash the final response text (if
    /// any) into the output buffer.
    async fn mark_completed(&self, task_id: &str, response_text: Option<&str>);

    /// Mark a task failed and append the error message.
    async fn mark_failed(&self, task_id: &str, error: &str);

    /// Snapshot a task's accumulated output buffer. Used by the
    /// periodic AgentSummary timer to feed recent text into the
    /// summarizer prompt. Returns the empty string for unknown
    /// task ids.
    async fn read_output(&self, task_id: &str) -> String;

    /// Whether a task is in a terminal state. Periodic loops
    /// observe this so they can stop cleanly without racing the
    /// final `mark_completed` / `mark_failed` call.
    async fn is_terminal(&self, task_id: &str) -> bool;
}

pub type AgentTaskRegistryRef = Arc<dyn AgentTaskRegistry>;

/// Sidecar metadata for a background AgentTool spawn. Mirrors TS
/// `AgentMetadata` (`utils/sessionStorage.ts:264-272`). Persisted
/// when the spawn registers; read on resume.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct AgentSpawnMetadata {
    /// Agent type used at original spawn. Resume routes against this
    /// when `subagent_type` is omitted.
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
///
/// TS reference: split across `utils/sessionStorage.ts`'s
/// `getAgentTranscriptPath` / `getAgentMetadataPath` /
/// `writeAgentMetadata` / `readAgentMetadata` / `getAgentTranscript`.
#[async_trait::async_trait]
pub trait AgentTranscriptStore: Send + Sync {
    /// Append `messages` (each a serialised `coco_messages::Message`)
    /// to the per-agent JSONL transcript. Conversation order is
    /// preserved by append order — coco-rs doesn't need TS's
    /// parent_uuid chain because `MessageHistory.messages` is
    /// already a Vec. Idempotent across multiple calls.
    async fn append_agent_messages(
        &self,
        session_id: &str,
        agent_id: &str,
        messages: Vec<serde_json::Value>,
    ) -> anyhow::Result<()>;

    /// Read every persisted message for an agent in conversation
    /// order. Returns `Ok(None)` when no transcript exists (no
    /// prior spawn). Resume passes the result as
    /// `AgentQueryConfig.fork_context_messages`.
    async fn load_agent_messages(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> anyhow::Result<Option<Vec<serde_json::Value>>>;

    /// Write the metadata sidecar for an agent.
    async fn write_agent_metadata(
        &self,
        session_id: &str,
        agent_id: &str,
        metadata: &AgentSpawnMetadata,
    ) -> anyhow::Result<()>;

    /// Read the metadata sidecar; `Ok(None)` when no sidecar exists.
    async fn read_agent_metadata(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> anyhow::Result<Option<AgentSpawnMetadata>>;
}

pub type AgentTranscriptStoreRef = Arc<dyn AgentTranscriptStore>;

/// No-op implementation for contexts without background tasks.
#[derive(Debug, Clone)]
pub struct NoOpTaskHandle;

#[async_trait::async_trait]
impl TaskHandle for NoOpTaskHandle {
    async fn spawn_shell_task(&self, _: BackgroundShellRequest) -> anyhow::Result<String> {
        anyhow::bail!("Background tasks not available in this context")
    }
    async fn get_task_status(&self, _: &str) -> anyhow::Result<BackgroundTaskInfo> {
        anyhow::bail!("Background tasks not available in this context")
    }
    async fn get_task_output_delta(&self, _: &str, _: i64) -> anyhow::Result<TaskOutputDelta> {
        anyhow::bail!("Background tasks not available in this context")
    }
    async fn kill_task(&self, _: &str) -> anyhow::Result<()> {
        anyhow::bail!("Background tasks not available in this context")
    }
    async fn list_tasks(&self) -> Vec<BackgroundTaskInfo> {
        vec![]
    }
    async fn poll_notifications(&self) -> Vec<BackgroundTaskInfo> {
        vec![]
    }
}
