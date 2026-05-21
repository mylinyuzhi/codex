//! `TaskReader` trait implementation — read-only inspection of task
//! state and on-disk output.
//!
//! TS source: `utils/task/framework.ts` (state-snapshot read side) +
//! `utils/task/diskOutput.ts` (`getTaskOutputDelta`).

use std::sync::Arc;

use async_trait::async_trait;
use coco_tool_runtime::{TaskOutputDelta, TaskReader, TerminalOutputs, TerminalSignal};
use coco_types::{TaskStateBase, TaskStatus, TaskType};
use tokio::sync::Notify;
use tracing::trace;

use super::{TaskRuntime, boxed_msg};
use crate::disk_task_output::DEFAULT_MAX_READ_BYTES;

#[async_trait]
impl TaskReader for TaskRuntime {
    async fn get_task_status(
        &self,
        task_id: &str,
    ) -> Result<TaskStateBase, coco_error::BoxedError> {
        self.manager.get(task_id).await.ok_or_else(|| {
            boxed_msg(
                format!("No running task found with ID: {task_id}"),
                coco_error::StatusCode::FileNotFound,
            )
        })
    }

    async fn get_task_output_delta(
        &self,
        task_id: &str,
        from_offset: i64,
    ) -> Result<TaskOutputDelta, coco_error::BoxedError> {
        let Some(state) = self.manager.get(task_id).await else {
            return Err(boxed_msg(
                format!("No running task found with ID: {task_id}"),
                coco_error::StatusCode::FileNotFound,
            ));
        };
        let Some(dto) = self.disk.get(task_id).await else {
            return Ok(TaskOutputDelta {
                content: String::new(),
                new_offset: from_offset,
                is_complete: state.status.is_terminal(),
            });
        };
        let _ = dto.flush().await;
        let (content, new_offset) = match dto.read_delta(from_offset, DEFAULT_MAX_READ_BYTES).await
        {
            Ok(pair) => pair,
            Err(_) => (String::new(), from_offset),
        };
        let is_complete = state.status.is_terminal();
        if is_complete && state.task_type == TaskType::LocalAgent {
            self.manager.mark_retrieved(task_id).await;
            trace!(
                target: "coco::task_runtime",
                task_id,
                "marked LocalAgent task as retrieved"
            );
        }
        trace!(
            target: "coco::task_runtime",
            task_id,
            from_offset,
            new_offset,
            delta_bytes = content.len(),
            is_complete,
            "served task output delta"
        );
        Ok(TaskOutputDelta {
            content,
            new_offset,
            is_complete,
        })
    }

    async fn list_tasks(&self) -> Vec<TaskStateBase> {
        self.manager.list().await
    }

    async fn subscribe_terminal(&self, task_id: &str) -> Option<TerminalSignal> {
        let entries = self.entries.read().await;
        entries
            .get(task_id)
            .map(|e| TerminalSignal::new(e.status_tx.subscribe()))
    }

    async fn detach_handle(&self, task_id: &str) -> Option<Arc<Notify>> {
        let entries = self.entries.read().await;
        entries.get(task_id).map(|e| e.detach.clone())
    }

    async fn read_terminal_outputs(
        &self,
        task_id: &str,
    ) -> Result<TerminalOutputs, coco_error::BoxedError> {
        let Some(state) = self.manager.get(task_id).await else {
            return Err(boxed_msg(
                format!("No task found with ID: {task_id}"),
                coco_error::StatusCode::FileNotFound,
            ));
        };
        let stdout = if let Some(dto) = self.disk.get(task_id).await {
            let _ = dto.flush().await;
            dto.read_tail(DEFAULT_MAX_READ_BYTES)
                .await
                .unwrap_or_default()
        } else {
            String::new()
        };
        let interrupted = matches!(state.status, TaskStatus::Killed);
        // W3: shell driver writes exit_code into the per-task
        // `OnceLock` from `apply_shell_terminal_state`. Agent tasks
        // and shell `Cancelled` / `TimedOut` / `SpawnFailed` outcomes
        // leave it unset, yielding `None`.
        let exit_code = self
            .entries
            .read()
            .await
            .get(task_id)
            .and_then(|e| e.exit_code.get().copied());
        Ok(TerminalOutputs {
            stdout,
            stderr: String::new(),
            exit_code,
            interrupted,
        })
    }
}
