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
use coco_types::LocalAgentExtras;
use coco_types::ServerNotification;
use coco_types::TaskCompletedParams;
use coco_types::TaskCompletionStatus;
use coco_types::TaskExtras;
use coco_types::TaskProgress;
use coco_types::TaskProgressParams;
use coco_types::TaskStartedParams;
use coco_types::TaskStateBase;
use coco_types::TaskStatus;
use coco_types::TaskType;
use coco_types::TaskUsage;
use coco_types::generate_task_id;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tokio::sync::mpsc;

/// Grace period before the panel may evict a terminal LocalAgent
/// task. TS: `utils/task/framework.ts:28` `PANEL_GRACE_MS = 30_000`.
pub const PANEL_GRACE_MS: i64 = 30_000;

/// Backward-compat type alias for the (now-canonical)
/// [`coco_types::LocalAgentExtras`] struct. Older `MemoryRuntime`
/// / test code referenced `LocalAgentExtra` (singular); the canonical
/// type lives in `coco-types` with the grammatically-correct plural
/// name. The alias keeps existing imports working without churn.
pub type LocalAgentExtra = LocalAgentExtras;

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
    /// Per-emit gate for `TaskProgress` summary events. TS parity:
    /// `getSdkAgentProgressSummariesEnabled()` at `LocalAgentTask.tsx:390`
    /// — TS checks this BOTH at summary-timer-start AND at every
    /// emit, so toggling the flag mid-session immediately stops new
    /// progress events. CLI bootstrap wires this from
    /// `app_state.agent_progress_summaries_enabled`. `None` ⇒ emission
    /// follows the upstream timer gate only (no per-emit check).
    sdk_summaries_enabled: Option<Arc<AtomicBool>>,
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
            sdk_summaries_enabled: None,
        }
    }

    /// Attach an SDK-summary gate. After this call, `TaskProgress`
    /// summary emissions are gated on `flag.load(Relaxed)` at every
    /// emit, in addition to the upstream timer gate. Matches TS
    /// double-gating at `LocalAgentTask.tsx:390 getSdkAgentProgressSummariesEnabled()`.
    pub fn with_summary_emission_gate(mut self, flag: Arc<AtomicBool>) -> Self {
        self.sdk_summaries_enabled = Some(flag);
        self
    }

    // ── LocalAgent sidecar accessors ─────────────────────────────
    //
    // Fields live inside `TaskStateBase.extras::LocalAgent(...)`.
    // All setters route through [`Self::update_local_agent_extras`]
    // so there's exactly one lock-acquire + variant-match site —
    // mirrors TS `utils/task/framework.ts:48-72 updateTaskState<T>`,
    // the single higher-order helper every TS setter delegates to.

    /// Higher-order helper: run `f` against the task's LocalAgent
    /// extras under the write lock and return its output. Returns
    /// `None` when the task doesn't exist OR isn't a LocalAgent task.
    ///
    /// TS parity: `updateTaskState<T extends TaskState>` at
    /// `utils/task/framework.ts:48-72` — the single update primitive
    /// every per-field TS setter delegates to.
    async fn update_local_agent_extras<F, R>(&self, id: &str, f: F) -> Option<R>
    where
        F: FnOnce(&mut LocalAgentExtras) -> R,
    {
        self.tasks
            .write()
            .await
            .get_mut(id)
            .and_then(|t| t.local_agent_extras_mut())
            .map(f)
    }

    /// Change-detecting variant of [`Self::update_local_agent_extras`].
    /// The closure returns `true` when it observed a real mutation,
    /// `false` for no-op writes. Used by emit-on-change setters so
    /// idempotent writes don't fire SDK events.
    ///
    /// TS parity: `framework.ts:59-62` — `if (updated === task) return prev`
    /// ref-equality short-circuit. Rust mutates in place so there's
    /// no reference identity to test; the closure declares change
    /// explicitly.
    ///
    /// Returns `Some(true)` on real change, `Some(false)` on no-op,
    /// `None` when the task doesn't exist OR isn't a LocalAgent task.
    async fn update_local_agent_extras_if_changed<F>(&self, id: &str, f: F) -> Option<bool>
    where
        F: FnOnce(&mut LocalAgentExtras) -> bool,
    {
        self.tasks
            .write()
            .await
            .get_mut(id)
            .and_then(|t| t.local_agent_extras_mut())
            .map(f)
    }

    /// Snapshot the extras for a LocalAgent task; returns default
    /// when the task doesn't exist OR isn't a LocalAgent task.
    pub async fn local_agent_extra(&self, id: &str) -> LocalAgentExtras {
        self.tasks
            .read()
            .await
            .get(id)
            .and_then(|t| t.local_agent_extras())
            .cloned()
            .unwrap_or_default()
    }

    /// Update a LocalAgent task's progress summary text. Preserves
    /// existing token / activity counters and emits a `TaskProgress`
    /// SDK event so subscribed consumers (VS Code subagent panel,
    /// JSONL transcript) see the summary as it arrives.
    ///
    /// TS parity: `updateAgentSummary` (`LocalAgentTask.tsx:387-406`):
    /// writes the summary field AND calls `emitTaskProgress` when
    /// `getSdkAgentProgressSummariesEnabled()`. The upstream gate
    /// lives at `agent_tool.rs:480` (where the summary timer is
    /// only spawned when the flag is on), so reaching this method in
    /// production already implies the SDK consumer opted in;
    /// emission is unconditional here when an `event_tx` is wired.
    pub async fn set_progress_summary(&self, id: &str, summary: String) {
        // Capture-or-skip: write only when the incoming summary differs
        // from what's already stored. On a real change, we also pull
        // the counter snapshot needed for the SDK emission so the
        // post-write read doesn't have to reacquire the lock.
        //
        // `RefCell`-style interior mutability is overkill here; an
        // `Option<ProgressEmitSnapshot>` placed in an outer scope via
        // a fresh binding gets the closure's captured-by-mut effect.
        let mut captured: Option<ProgressEmitSnapshot> = None;
        let changed = self
            .update_local_agent_extras_if_changed(id, |e| {
                let mut p = e.progress.clone().unwrap_or_default();
                if p.summary.as_deref() == Some(summary.as_str()) {
                    // No-op write: don't perturb the cache or emit.
                    return false;
                }
                p.summary = Some(summary.clone());
                captured = Some(ProgressEmitSnapshot {
                    total_tokens: p.total_tokens,
                    tool_use_count: p.tool_use_count,
                    last_tool_name: p.last_tool_name.clone(),
                });
                e.progress = Some(p);
                true
            })
            .await;
        if matches!(changed, Some(true))
            && let Some(snap) = captured
        {
            self.emit_progress_summary(id, summary, snap).await;
        }
    }

    /// Replace a LocalAgent task's full progress snapshot. Used by the
    /// engine's stream-drain loop on every `ToolUseStarted` event to
    /// refresh tool count + `last_tool_name` (D2). Preserves any
    /// existing `summary` (the periodic AgentSummary timer is the only
    /// writer of that field) across overlapping writes.
    ///
    /// Emits a `TaskProgress` SDK event so consumers (VS Code subagent
    /// panel, JSONL transcript) observe per-tool progress in real time.
    /// TS parity: `AgentTool.tsx:947-948` calls
    /// `updateAsyncAgentProgress` then `emitTaskProgress` separately —
    /// Rust folds both into one seam (`set_progress`).
    pub async fn set_progress(&self, id: &str, mut progress: TaskProgress) {
        // Capture-or-skip: write + emit only when the incoming snapshot
        // differs from what's stored. Avoids fanning out SDK events
        // for no-op writes (e.g. unchanged tool count).
        let mut emit_payload: Option<TaskProgress> = None;
        self.update_local_agent_extras_if_changed(id, |e| {
            // Preserve any existing summary; otherwise an interleaved
            // write from the stream would clobber the periodic
            // AgentSummary text.
            if progress.summary.is_none()
                && let Some(existing) = e.progress.as_ref().and_then(|p| p.summary.clone())
            {
                progress.summary = Some(existing);
            }
            if e.progress.as_ref() == Some(&progress) {
                return false;
            }
            emit_payload = Some(progress.clone());
            e.progress = Some(progress);
            true
        })
        .await;
        if let Some(payload) = emit_payload {
            self.emit_progress(id, payload).await;
        }
    }

    /// Emit a `TaskProgress` SDK event with the full progress snapshot.
    /// Per-emit gate (D14) applies — when the SDK summary flag is off,
    /// drop without emitting. Per-tool progress events ride the same
    /// gate as periodic summaries; TS uses one flag for both
    /// (`getSdkAgentProgressSummariesEnabled`).
    async fn emit_progress(&self, task_id: &str, progress: TaskProgress) {
        let Some(tx) = &self.event_tx else { return };
        if let Some(gate) = &self.sdk_summaries_enabled
            && !gate.load(Ordering::Relaxed)
        {
            return;
        }
        let Some(state) = self.tasks.read().await.get(task_id).cloned() else {
            return;
        };
        let duration_ms = current_time_ms().saturating_sub(state.start_time);
        let params = TaskProgressParams {
            task_id: task_id.to_string(),
            tool_use_id: state.tool_use_id,
            description: state.description,
            usage: TaskUsage {
                total_tokens: progress.total_tokens,
                tool_uses: progress.tool_use_count,
                duration_ms,
            },
            last_tool_name: progress.last_tool_name,
            summary: progress.summary,
            workflow_progress: Vec::new(),
        };
        let _ = tx
            .send(CoreEvent::Protocol(ServerNotification::TaskProgress(
                params,
            )))
            .await;
    }

    /// Flip the `retrieved` flag — TS: `compact.ts:1578`.
    pub async fn mark_retrieved(&self, id: &str) {
        self.update_local_agent_extras(id, |e| e.retrieved = true)
            .await;
    }

    /// UI flips this when the user pins a task panel open.
    pub async fn set_retain(&self, id: &str, retain: bool) {
        self.update_local_agent_extras(id, |e| e.retain = retain)
            .await;
    }

    /// Stamp the panel grace-period deadline.
    pub async fn set_evict_after(&self, id: &str, evict_after_ms: Option<i64>) {
        self.update_local_agent_extras(id, |e| e.evict_after = evict_after_ms)
            .await;
    }

    /// Toggle the Ctrl+B session-backgrounded flag.
    pub async fn set_backgrounded(&self, id: &str, backgrounded: bool) {
        self.update_local_agent_extras(id, |e| e.is_backgrounded = backgrounded)
            .await;
    }

    /// Record the error text from a `Failed` terminal transition.
    /// Picked up by `coco_tasks::reminder_source::collect` to populate
    /// the `delta_summary` field of the post-compact `task_status`
    /// reminder. TS parity: `compact.ts:1591-1594` `agent.error`.
    pub async fn set_error(&self, id: &str, error: String) {
        self.update_local_agent_extras(id, |e| e.error = Some(error))
            .await;
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
        self.insert_and_emit(
            id,
            task_type,
            description,
            output_file,
            TaskStatus::Pending,
            false,
        )
        .await
    }

    /// Create + insert a task already in [`TaskStatus::Running`].
    /// Used by every production background-task spawn — agent
    /// registration (`TaskRuntime::register_agent_task`) and shell
    /// dispatch (`TaskRuntime::spawn_shell_task`) both call this so
    /// only ONE lifecycle event fires: a single
    /// `TaskStarted(Running)` instead of TS-noisy
    /// `TaskStarted(Pending)` → `TaskProgress(Running)` pair.
    ///
    /// `is_backgrounded` defaults to `false` (foreground init); the
    /// id-providing variant ([`Self::create_running_with_id`]) lets
    /// agent-task callers pass `true` for fire-and-forget spawns to
    /// match TS `registerAsyncAgent`'s immediate-background init.
    pub async fn create_running(
        &self,
        task_type: TaskType,
        description: &str,
        output_file: &str,
    ) -> String {
        let id = generate_task_id(task_type);
        self.insert_and_emit(
            id,
            task_type,
            description,
            output_file,
            TaskStatus::Running,
            false,
        )
        .await
    }

    /// Insert a task with a caller-provided id (minted upstream so
    /// the caller can resolve per-id state like the disk-output
    /// path *before* the lifecycle event fires) already in
    /// [`TaskStatus::Running`]. Returns the id unchanged.
    ///
    /// `is_backgrounded` selects between TS `registerAgentForeground`
    /// (`false`, default; flips to `true` later if the auto-background
    /// timer fires) and `registerAsyncAgent` (`true`, fire-and-forget).
    /// TS parity: `LocalAgentTask.tsx:499` (async) vs `:564` (fg).
    pub async fn create_running_with_id(
        &self,
        id: String,
        task_type: TaskType,
        description: &str,
        output_file: &str,
        is_backgrounded: bool,
    ) -> String {
        self.insert_and_emit(
            id,
            task_type,
            description,
            output_file,
            TaskStatus::Running,
            is_backgrounded,
        )
        .await
    }

    async fn insert_and_emit(
        &self,
        id: String,
        task_type: TaskType,
        description: &str,
        output_file: &str,
        status: TaskStatus,
        is_backgrounded: bool,
    ) -> String {
        // LocalAgent + Dream tasks carry the LocalAgent sidecar
        // (TS uses the same shape for both task types). Other task
        // types use the `None` variant — no dead Option fields on
        // the wire.
        let extras = match task_type {
            TaskType::LocalAgent | TaskType::Dream => TaskExtras::local_agent(is_backgrounded),
            _ => TaskExtras::None,
        };
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
            extras,
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
        // W6 (A5): status flip + evict_after stamp happen under a
        // single write lock. Previously the evict_after stamp went
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
                    if task.task_type == TaskType::LocalAgent
                        && let Some(extras) = task.local_agent_extras_mut()
                        && !extras.retain
                    {
                        extras.evict_after = Some(current_time_ms() + PANEL_GRACE_MS);
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
                if t.task_type == TaskType::LocalAgent
                    && let Some(extras) = t.local_agent_extras()
                {
                    if extras.retain {
                        return false;
                    }
                    if let Some(deadline) = extras.evict_after
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

    /// Emit a `TaskProgress` SDK event with a summary update.
    /// Mirrors TS `emitTaskProgress` (`utils/task/sdkProgress.ts:10-36`)
    /// called from `updateAgentSummary` at `LocalAgentTask.tsx:397-405`.
    async fn emit_progress_summary(
        &self,
        task_id: &str,
        summary: String,
        snap: ProgressEmitSnapshot,
    ) {
        let Some(tx) = &self.event_tx else { return };
        // D14: per-emit gate. When the SDK consumer toggles
        // `agent_progress_summaries_enabled` off mid-session, stop
        // emitting immediately — don't rely solely on the upstream
        // timer-start gate, which checks once and never re-reads.
        if let Some(gate) = &self.sdk_summaries_enabled
            && !gate.load(Ordering::Relaxed)
        {
            return;
        }
        // Pull description + tool_use_id + start_time from canonical
        // state without holding any extras lock — the snapshot above
        // already captured the counter fields atomically.
        let Some(state) = self.tasks.read().await.get(task_id).cloned() else {
            return;
        };
        let duration_ms = current_time_ms().saturating_sub(state.start_time);
        let params = TaskProgressParams {
            task_id: task_id.to_string(),
            tool_use_id: state.tool_use_id,
            description: state.description,
            usage: TaskUsage {
                total_tokens: snap.total_tokens,
                tool_uses: snap.tool_use_count,
                duration_ms,
            },
            last_tool_name: snap.last_tool_name,
            summary: Some(summary),
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

/// Counter fields captured under the `update_local_agent_extras`
/// write lock so the post-write `TaskProgress` emission doesn't have
/// to re-acquire the lock to read them. The progress event uses
/// these alongside `summary`, `task_id`, and `description`.
#[derive(Debug, Clone)]
struct ProgressEmitSnapshot {
    total_tokens: i64,
    tool_use_count: i32,
    last_tool_name: Option<String>,
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
