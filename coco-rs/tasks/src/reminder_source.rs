//! `TaskStatusSource` impl on [`crate::running::TaskManager`].
//!
//! TS `getUnifiedTaskAttachments(ctx)` fires post-compaction to warn
//! the agent against duplicate background-task spawns. coco-rs
//! mirrors that semantics: when `just_compacted` is true, this impl
//! snapshots every currently-tracked running task and maps its status
//! into a `TaskStatusSnapshot` so the reminder generator can render
//! the per-task warning text verbatim with TS.
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

use crate::running::TaskManager;

#[async_trait]
impl TaskStatusSource for TaskManager {
    async fn collect(
        &self,
        _agent_id: Option<&str>,
        just_compacted: bool,
    ) -> Vec<TaskStatusSnapshot> {
        // TS only emits `task_status` reminders post-compaction.
        if !just_compacted {
            return Vec::new();
        }
        self.list()
            .await
            .into_iter()
            .map(|t| TaskStatusSnapshot {
                task_id: t.id,
                description: t.description,
                status: map_status(t.status),
                // Delta summary + output-file path come from deeper
                // integration (LocalAgentTask progress summaries); for
                // now the reminder omits them and the generator
                // emits the terse text variant.
                delta_summary: None,
                output_file_path: Some(t.output_file).filter(|s| !s.is_empty()),
            })
            .collect()
    }
}

fn map_status(s: TaskStatus) -> TaskRunStatus {
    match s {
        TaskStatus::Completed => TaskRunStatus::Completed,
        TaskStatus::Failed => TaskRunStatus::Failed,
        TaskStatus::Killed | TaskStatus::Cancelled => TaskRunStatus::Killed,
        TaskStatus::Pending | TaskStatus::Running => TaskRunStatus::Running,
    }
}

#[cfg(test)]
#[path = "reminder_source.test.rs"]
mod tests;
