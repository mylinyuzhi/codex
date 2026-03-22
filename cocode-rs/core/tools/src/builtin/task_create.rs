//! TaskCreate tool for creating structured tasks.

use super::prompts;
use super::structured_tasks::StructuredTask;
use super::structured_tasks::StructuredTaskStore;
use super::structured_tasks::TaskStatus;
use super::structured_tasks::{self};
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

pub struct TaskCreateTool {
    store: StructuredTaskStore,
}

impl TaskCreateTool {
    pub fn new(store: StructuredTaskStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::TaskCreate.as_str()
    }

    fn description(&self) -> &str {
        prompts::TASK_CREATE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "subject": {
                    "type": "string",
                    "description": "Brief task title in imperative form (e.g., 'Fix authentication bug')"
                },
                "description": {
                    "type": "string",
                    "description": "Detailed description of what needs to be done"
                },
                "activeForm": {
                    "type": "string",
                    "description": "Present continuous form for progress display (e.g., 'Fixing authentication bug')"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress"],
                    "description": "Initial status (default: pending)",
                    "default": "pending"
                },
                "owner": {
                    "type": "string",
                    "description": "Optional owner (agent/team name) for task assignment"
                },
                "blocks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs that this task blocks"
                },
                "blockedBy": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs that block this task"
                },
                "metadata": {
                    "type": "object",
                    "description": "Arbitrary metadata to attach to the task"
                }
            },
            "required": ["subject"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        // Safe: uses Arc<Mutex<...>> internally for synchronization
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::StructuredTasks)
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let subject = super::input_helpers::require_str(&input, "subject")?;

        let status_str = input["status"].as_str().unwrap_or("pending");
        let status = TaskStatus::parse(status_str).unwrap_or(TaskStatus::Pending);

        // Only pending or in_progress allowed at creation
        if matches!(status, TaskStatus::Completed | TaskStatus::Deleted) {
            return Err(crate::error::tool_error::InvalidInputSnafu {
                message: "Initial status must be 'pending' or 'in_progress'",
            }
            .build());
        }

        let task_id = structured_tasks::generate_task_id();

        // Parse initial dependency arrays
        let initial_blocks = super::input_helpers::string_array(&input, "blocks");
        let initial_blocked_by = super::input_helpers::string_array(&input, "blockedBy");

        let task = StructuredTask {
            id: task_id.clone(),
            subject: subject.to_string(),
            description: input["description"].as_str().map(String::from),
            status,
            active_form: Some(
                input["activeForm"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| structured_tasks::derive_active_form(subject)),
            ),
            owner: input["owner"].as_str().map(String::from),
            blocks: initial_blocks.clone(),
            blocked_by: initial_blocked_by.clone(),
            metadata: input.get("metadata").cloned().unwrap_or(Value::Null),
        };

        let snapshot = {
            let mut store = self.store.lock().await;

            // Enforce max 1 in_progress
            if matches!(status, TaskStatus::InProgress) {
                let existing_in_progress = store
                    .values()
                    .any(|t| matches!(t.status, TaskStatus::InProgress));
                if existing_in_progress {
                    return Err(crate::error::tool_error::InvalidInputSnafu {
                        message: "At most 1 task can be in_progress at a time",
                    }
                    .build());
                }
            }

            store.insert(task_id.clone(), task);

            // Bidirectional dependency consistency: update related tasks
            for target_id in &initial_blocks {
                if let Some(target) = store.get_mut(target_id)
                    && !target.blocked_by.contains(&task_id)
                {
                    target.blocked_by.push(task_id.clone());
                }
            }
            for source_id in &initial_blocked_by {
                if let Some(source) = store.get_mut(source_id)
                    && !source.blocks.contains(&task_id)
                {
                    source.blocks.push(task_id.clone());
                }
            }

            structured_tasks::tasks_to_value(&store)
        };

        ctx.emit_progress(format!("Created task {task_id}: {subject}"))
            .await;

        Ok(ToolOutput::text(format!(
            "Task created successfully.\nID: {task_id}\nSubject: {subject}\nStatus: {status_str}"
        ))
        .with_modifier(cocode_protocol::ContextModifier::StructuredTasksUpdated {
            tasks: snapshot,
        }))
    }
}

#[cfg(test)]
#[path = "task_create.test.rs"]
mod tests;
