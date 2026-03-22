//! TaskList tool for listing all structured tasks.

use super::prompts;
use super::structured_tasks::StructuredTaskStore;
use super::structured_tasks::{self};
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

pub struct TaskListTool {
    store: StructuredTaskStore,
}

impl TaskListTool {
    pub fn new(store: StructuredTaskStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::TaskList.as_str()
    }

    fn description(&self) -> &str {
        prompts::TASK_LIST_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "deleted", "all"],
                    "description": "Filter by status (default: all non-deleted)",
                    "default": "all"
                }
            }
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::StructuredTasks)
    }

    async fn execute(&self, input: Value, _ctx: &mut ToolContext) -> Result<ToolOutput> {
        let status_filter = input["status"].as_str().unwrap_or("all");

        let store = self.store.lock().await;

        // Build set of completed task IDs for smart blocker filtering
        let completed_ids: std::collections::HashSet<String> = store
            .values()
            .filter(|t| matches!(t.status, structured_tasks::TaskStatus::Completed))
            .map(|t| t.id.clone())
            .collect();

        if status_filter == "all" {
            let summary =
                structured_tasks::format_task_summary_filtered(&store, Some(&completed_ids));
            return Ok(ToolOutput::text(summary));
        }

        // Filter by specific status using shared format_single_task
        let target_status = structured_tasks::TaskStatus::parse(status_filter);
        let mut summary = String::new();
        for task in store.values() {
            if target_status.is_some_and(|s| task.status != s) {
                continue;
            }
            if let Some(line) = structured_tasks::format_single_task(task, Some(&completed_ids)) {
                summary.push_str(&line);
            }
        }
        if summary.is_empty() {
            summary = "No tasks.".to_string();
        }
        Ok(ToolOutput::text(summary))
    }
}

#[cfg(test)]
#[path = "task_list.test.rs"]
mod tests;
