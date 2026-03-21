//! TaskGet tool for retrieving a single structured task.

use super::prompts;
use super::structured_tasks::StructuredTaskStore;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

pub struct TaskGetTool {
    store: StructuredTaskStore,
}

impl TaskGetTool {
    pub fn new(store: StructuredTaskStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for TaskGetTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::TaskGet.as_str()
    }

    fn description(&self) -> &str {
        prompts::TASK_GET_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskId": {
                    "type": "string",
                    "description": "The ID of the structured task to retrieve"
                }
            },
            "required": ["taskId"]
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
        let id = super::input_helpers::require_str(&input, "taskId")?;

        let store = self.store.lock().await;
        let task = store.get(id).ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: format!("Task not found: {id}"),
            }
            .build()
        })?;

        let json = serde_json::to_string_pretty(task).unwrap_or_else(|e| {
            tracing::error!("Task serialization failed: {e}");
            String::new()
        });
        Ok(ToolOutput::text(json))
    }
}

#[cfg(test)]
#[path = "task_get.test.rs"]
mod tests;
