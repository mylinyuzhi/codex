//! `TaskStatusSource` impl on [`crate::running::TaskManager`].
//!
//! TS `getUnifiedTaskAttachments(ctx)` fires post-compaction to warn
//! the agent against duplicate background-task spawns. coco-rs
//! mirrors that semantics: when `just_compacted` is true, this impl
//! snapshots every currently-tracked LocalAgent running task and
//! maps its status into a `TaskStatusSnapshot` so the reminder
//! generator can render the per-task warning text verbatim with TS.
//!
//! ## TS-aligned filter (mirrors `compact.ts:1572-1598`)
//!
//! Three skip conditions, applied in order:
//!
//! 1. **Not a LocalAgent** — TS only generates `task_status`
//!    reminders for `local_agent` tasks. Shell / dream / teammate
//!    tasks don't carry the "don't spawn a duplicate" affordance.
//! 2. **`retrieved`** — `TaskOutputTool` already served this task's
//!    terminal output; the model knows what happened, no need to
//!    re-announce. Source: TS `compact.ts:1578` `agent.retrieved`.
//! 3. **Same `agent_id` as the caller** — fork mode + nested
//!    spawns. The agent doesn't warn itself about itself. Source:
//!    TS `compact.ts:1580` `agent.agentId === context.agentId`.
//!    coco-rs threads `agent_id` through the `collect` parameter;
//!    `None` means the main thread.
//!
//! TS parity notes:
//! - Only `Running` tasks need the anti-duplicate warning; coco-rs
//!   surfaces `Completed` / `Failed` / `Killed` too so the post-compact
//!   reminder text matches TS branching (`messages.ts:3954-3988`).
//! - When `just_compacted` is false we return empty — TS only emits
//!   this reminder right after the compaction boundary.

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
        let mut skipped_retrieved = 0usize;
        let mut skipped_self = 0usize;
        let mut snapshots = Vec::with_capacity(states.len());
        for t in states {
            // Rule 1: LocalAgent only.
            if t.task_type != TaskType::LocalAgent {
                skipped_type += 1;
                continue;
            }
            let extra = self.local_agent_extra(&t.id).await;
            // Rule 2: skip if `TaskOutputTool` already retrieved.
            if extra.retrieved {
                skipped_retrieved += 1;
                continue;
            }
            // Rule 3: skip when the caller is the task itself
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
            snapshots.push(TaskStatusSnapshot {
                task_id: t.id,
                description: t.description,
                status: map_status(t.status),
                task_type: task_type_wire_name(t.task_type).to_string(),
                // `delta_summary` mirrors TS `agent.progress?.summary`
                // for running tasks and `agent.error` for failed.
                // `compact.ts:1591-1594`.
                delta_summary: match map_status(t.status) {
                    TaskRunStatus::Running => extra.progress_summary,
                    _ => None,
                },
                output_file_path: Some(t.output_file).filter(|s| !s.is_empty()),
            });
        }
        debug!(
            target: "coco::task_reminder",
            total,
            kept,
            skipped_type,
            skipped_retrieved,
            skipped_self,
            caller_agent_id = ?agent_id,
            "task_status reminder snapshot built (post-compact)"
        );
        snapshots
    }
}

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
