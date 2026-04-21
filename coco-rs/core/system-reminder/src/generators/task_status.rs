//! TS `task_status` generator.
//!
//! Mirrors `normalizeAttachmentForAPI` `case 'task_status':`
//! (`messages.ts:3954`). Rendering varies by status:
//!
//! - `Killed`: brief "stopped by the user" note.
//! - `Running`: anti-duplicate warning; optional delta summary and
//!   output-file pointer.
//! - `Completed` / `Failed`: outcome summary with optional delta.
//!
//! TS emits one reminder per `task_status` attachment; coco-rs joins
//! multiple statuses emitted this turn with `\n\n`.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generator::TaskRunStatus;
use crate::generator::TaskStatusSnapshot;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

#[derive(Debug, Default)]
pub struct TaskStatusGenerator;

#[async_trait]
impl AttachmentGenerator for TaskStatusGenerator {
    fn name(&self) -> &str {
        "TaskStatusGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::TaskStatus
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.task_status
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.task_statuses.is_empty() {
            return Ok(None);
        }
        let parts: Vec<String> = ctx.task_statuses.iter().map(render_one).collect();
        Ok(Some(SystemReminder::new(
            AttachmentType::TaskStatus,
            parts.join("\n\n"),
        )))
    }
}

fn render_one(t: &TaskStatusSnapshot) -> String {
    match t.status {
        TaskRunStatus::Killed => format!(
            "Task \"{desc}\" ({id}) was stopped by the user.",
            desc = t.description,
            id = t.task_id
        ),
        TaskRunStatus::Running => {
            let mut parts = vec![format!(
                "Background agent \"{desc}\" ({id}) is still running.",
                desc = t.description,
                id = t.task_id
            )];
            if let Some(s) = t.delta_summary.as_deref() {
                parts.push(format!("Progress: {s}"));
            }
            if let Some(p) = t.output_file_path.as_deref() {
                parts.push(format!(
                    "Do NOT spawn a duplicate. You will be notified when it completes. You can read partial output at {p}."
                ));
            } else {
                parts.push(
                    "Do NOT spawn a duplicate. You will be notified when it completes.".to_string(),
                );
            }
            parts.join("\n")
        }
        TaskRunStatus::Completed => {
            let header = format!(
                "Task \"{desc}\" ({id}) completed.",
                desc = t.description,
                id = t.task_id
            );
            match t.delta_summary.as_deref() {
                Some(s) => format!("{header}\nResult: {s}"),
                None => header,
            }
        }
        TaskRunStatus::Failed => {
            let header = format!(
                "Task \"{desc}\" ({id}) failed.",
                desc = t.description,
                id = t.task_id
            );
            match t.delta_summary.as_deref() {
                Some(s) => format!("{header}\nError: {s}"),
                None => header,
            }
        }
    }
}

#[cfg(test)]
#[path = "task_status.test.rs"]
mod tests;
