//! `TaskStatusSource` impl on [`crate::running::TaskManager`].
//!
//! TS `createAsyncAgentAttachmentsIfNeeded` (`services/compact/compact.ts:1568-1598`)
//! fires post-compaction so the model rediscovers background agents
//! whose `<task-notification>` envelope was wiped from the
//! `CommandQueue` by compaction. Both **still-running** agents (so the
//! model doesn't spawn a duplicate) AND **terminal** agents whose
//! results haven't been retrieved must be re-injected — compaction
//! cleared the queue that delivered them inline.
//!
//! ## TS-aligned filter (mirrors `compact.ts:1576-1583`)
//!
//! Skip conditions, applied in order:
//!
//! 1. **Not a LocalAgent** — TS only generates `task_status`
//!    reminders for `local_agent` tasks. Shell / dream / teammate
//!    tasks don't carry the "don't spawn a duplicate" affordance.
//! 2. **`status == Pending`** — never started; nothing to report.
//!    TS: `compact.ts:1579` `agent.status === 'pending'`.
//! 3. **`retrieved`** — `TaskOutputTool` already served this task's
//!    output (terminal or partial); model knows what happened. TS:
//!    `compact.ts:1578` `agent.retrieved`.
//! 4. **Same `agent_id` as the caller** — fork mode + nested
//!    spawns. The agent doesn't warn itself about itself. TS:
//!    `compact.ts:1580` `agent.agentId === context.agentId`.
//!    coco-rs threads `agent_id` through `collect`; `None` means
//!    the main thread.
//!
//! Terminal tasks (Completed / Failed / Killed) flow through the
//! generator's status-dispatched render at
//! `coco_system_reminder::generators::task_status::render_one` —
//! TS counterpart `messages.ts:3954-4024`.

use async_trait::async_trait;
use coco_system_reminder::TaskRunStatus;
use coco_system_reminder::TaskStatusSnapshot;
use coco_system_reminder::TaskStatusSource;
use coco_types::TaskStatus;
use coco_types::TaskType;
use tracing::debug;

use crate::running::TaskManager;
use crate::running::task_type_wire_name;

#[async_trait]
impl TaskStatusSource for TaskManager {
    async fn collect(
        &self,
        agent_id: Option<&str>,
        just_compacted: bool,
    ) -> Vec<TaskStatusSnapshot> {
        // TS only emits `task_status` reminders post-compaction.
        if !just_compacted {
            return Vec::new();
        }
        let states = self.list().await;
        // Track filter outcomes for ops debugging — when an agent
        // wonders "why no task_status reminder?", the log answers
        // exactly which rule fired.
        let total = states.len();
        let mut kept = 0usize;
        let mut skipped_type = 0usize;
        let mut skipped_pending = 0usize;
        let mut skipped_retrieved = 0usize;
        let mut skipped_self = 0usize;
        let mut snapshots = Vec::with_capacity(states.len());
        for t in states {
            // Rule 1: LocalAgent only.
            if t.task_type != TaskType::LocalAgent {
                skipped_type += 1;
                continue;
            }
            // Rule 2: skip Pending — never started, nothing to report.
            // TS: `compact.ts:1579` `agent.status === 'pending'`.
            if t.status == TaskStatus::Pending {
                skipped_pending += 1;
                continue;
            }
            let extra = self.local_agent_extra(&t.id).await;
            // Rule 3: skip if `TaskOutputTool` already retrieved
            // partial output and the model has shown awareness.
            if extra.retrieved {
                skipped_retrieved += 1;
                continue;
            }
            // Rule 4: skip when the caller IS the task (fork-mode
            // recursion guard — an agent shouldn't be reminded about
            // its own running state). For LocalAgent tasks the
            // `task_id` IS the agent's identifier: `register_agent_task_inner`
            // mints both from `coco_types::generate_task_id(LocalAgent)`
            // and threads the same id everywhere downstream.
            //
            // TS parity: `compact.ts:1580` `agent.agentId === context.agentId`.
            //
            // (The previous comparison used `tool_use_id` as a "proxy" —
            // it isn't one. `tool_use_id` is the Anthropic-style
            // `toolu_...` ID of the spawning tool call, in a different
            // namespace from agent_id; the proxy never matched and the
            // self-filter never fired.)
            if let Some(caller_id) = agent_id
                && t.id == caller_id
            {
                skipped_self += 1;
                continue;
            }
            kept += 1;
            // Build the `delta_summary`. TS source:
            // `compact.ts:1591-1594` reads `agent.progress?.summary`
            // for running tasks and `agent.error` for terminal tasks.
            //
            // For terminal-error tasks, prefer the recorded `error`
            // text (set by `mark_failed` on the failure path) so the
            // model sees `"Delta: <error>"` in the post-compact
            // reminder — TS-parity. Fall through to the progress
            // summary / synthetic counter sentence when no error is
            // recorded (Completed / Killed without recorded error).
            let delta_summary = if t.status.is_terminal()
                && let Some(err) = extra.error.as_deref()
                && !err.is_empty()
            {
                Some(err.to_string())
            } else {
                extra.progress.as_ref().map(|p| {
                    p.summary.clone().unwrap_or_else(|| {
                        let mut parts = Vec::new();
                        if let Some(last) = p.last_tool_name.as_deref() {
                            parts.push(format!("last action: {last}"));
                        }
                        if p.tool_use_count > 0 {
                            parts.push(format!("tool uses: {}", p.tool_use_count));
                        }
                        if p.total_tokens > 0 {
                            parts.push(format!("tokens: {}", p.total_tokens));
                        }
                        parts.join(", ")
                    })
                })
            };
            snapshots.push(TaskStatusSnapshot {
                task_id: t.id,
                description: t.description,
                status: map_status(t.status),
                task_type: task_type_wire_name(t.task_type).to_string(),
                delta_summary,
                output_file_path: Some(t.output_file).filter(|s| !s.is_empty()),
            });
        }
        debug!(
            target: "coco::task_reminder",
            total,
            kept,
            skipped_type,
            skipped_pending,
            skipped_retrieved,
            skipped_self,
            caller_agent_id = ?agent_id,
            "task_status reminder snapshot built (post-compact, TS-aligned filter)"
        );
        snapshots
    }
}

/// Map the 5-variant `TaskStatus` to the 4-variant `TaskRunStatus`
/// the reminder generator dispatches on (`Pending` collapses into
/// `Running` because the filter above already rejects `Pending`).
fn map_status(s: TaskStatus) -> TaskRunStatus {
    match s {
        TaskStatus::Completed => TaskRunStatus::Completed,
        TaskStatus::Failed => TaskRunStatus::Failed,
        TaskStatus::Killed => TaskRunStatus::Killed,
        TaskStatus::Pending | TaskStatus::Running => TaskRunStatus::Running,
    }
}

#[cfg(test)]
#[path = "reminder_source.test.rs"]
mod tests;
