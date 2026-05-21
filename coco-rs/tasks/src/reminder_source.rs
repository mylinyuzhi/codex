//! `TaskStatusSource` impl on [`crate::running::TaskManager`].
//!
//! TS `getUnifiedTaskAttachments(ctx)` fires post-compaction to warn
//! the agent against duplicate background-task spawns. coco-rs
//! mirrors that semantics: when `just_compacted` is true, this impl
//! snapshots every currently-tracked LocalAgent **running** task and
//! maps its status into a `TaskStatusSnapshot` so the reminder
//! generator can render the "still running, don't spawn a duplicate"
//! warning text verbatim with TS.
//!
//! ## TS-aligned filter (mirrors `compact.ts:1572-1598`)
//!
//! Skip conditions, applied in order:
//!
//! 1. **Not a LocalAgent** — TS only generates `task_status`
//!    reminders for `local_agent` tasks. Shell / dream / teammate
//!    tasks don't carry the "don't spawn a duplicate" affordance.
//! 2. **Terminal status** (W6 / A2 fix) — Completed / Failed /
//!    Killed tasks have already delivered their result through the
//!    `<task-notification>` envelope via `CommandQueue`
//!    (`QueuedCommandGenerator`). Re-emitting them post-compact
//!    would double-inform the model. The post-compact reminder is
//!    explicitly the "still running, don't spawn a duplicate" hint;
//!    that's only meaningful for in-flight tasks.
//! 3. **`retrieved`** — `TaskOutputTool` already served this task's
//!    output (terminal or partial); model knows what happened. TS:
//!    `compact.ts:1578` `agent.retrieved`.
//! 4. **Same `agent_id` as the caller** — fork mode + nested
//!    spawns. The agent doesn't warn itself about itself. TS:
//!    `compact.ts:1580` `agent.agentId === context.agentId`.
//!    coco-rs threads `agent_id` through `collect`; `None` means
//!    the main thread.
//!
//! TS parity notes:
//! - When `just_compacted` is false we return empty — TS only emits
//!   this reminder right after the compaction boundary.
//! - Terminal-task reporting flows through the `QueuedCommand`
//!   reminder (the `<task-notification>` XML envelope wrapped in
//!   `<system-reminder>`), which is the single source of truth for
//!   "task X terminated with result Y".

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
        let mut skipped_terminal = 0usize;
        let mut skipped_retrieved = 0usize;
        let mut skipped_self = 0usize;
        let mut snapshots = Vec::with_capacity(states.len());
        for t in states {
            // Rule 1: LocalAgent only.
            if t.task_type != TaskType::LocalAgent {
                skipped_type += 1;
                continue;
            }
            // Rule 2 (W6 / A2): terminal tasks already delivered via
            // the `QueuedCommandGenerator` (`<task-notification>` XML
            // wrapped in `<system-reminder>`). The post-compact
            // reminder is the "still running, don't spawn a
            // duplicate" hint — it has no purpose for terminal
            // tasks.
            if t.status.is_terminal() {
                skipped_terminal += 1;
                continue;
            }
            let extra = self.local_agent_extra(&t.id).await;
            // Rule 3: skip if `TaskOutputTool` already retrieved
            // partial output and the model has shown awareness.
            if extra.retrieved {
                skipped_retrieved += 1;
                continue;
            }
            // Rule 4: skip when the caller is the task itself
            // (fork-mode recursion). `tool_use_id` is the closest
            // proxy we have for "agent that produced this task" on
            // the caller side; matches TS `agent.agentId ===
            // context.agentId`.
            if let Some(caller_id) = agent_id
                && t.tool_use_id.as_deref() == Some(caller_id)
            {
                skipped_self += 1;
                continue;
            }
            kept += 1;
            // After Rule 2, every snapshot here is in `Running` state
            // (Pending/Running both map to TaskRunStatus::Running).
            // `delta_summary` mirrors TS `agent.progress?.summary`
            // (`compact.ts:1591-1594`).
            // Build the `delta_summary` from the richer `TaskProgress`
            // struct. TS source: `compact.ts:1591-1594` reads
            // `agent.progress?.summary`. coco-rs falls back to a
            // "$tool_use_count tool uses, $token_count tokens" sentence
            // when the periodic summary text hasn't fired yet so the
            // model gets *some* delta signal instead of nothing.
            let delta_summary = extra.progress.as_ref().map(|p| {
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
            });
            snapshots.push(TaskStatusSnapshot {
                task_id: t.id,
                description: t.description,
                status: TaskRunStatus::Running,
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
            skipped_terminal,
            skipped_retrieved,
            skipped_self,
            caller_agent_id = ?agent_id,
            "task_status reminder snapshot built (post-compact, running-only)"
        );
        snapshots
    }
}

// W6 / A2: terminal tasks are now filtered out (Rule 2 above), so
// the status mapping is trivially `Running` for everything that
// survives the filter. Kept as a no-op helper for now in case future
// reminders re-introduce terminal-task variants; remove if it remains
// unused after the W6 settle.
#[allow(dead_code)]
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
