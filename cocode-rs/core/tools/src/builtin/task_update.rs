//! TaskUpdate tool for updating structured tasks.

use super::prompts;
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

pub struct TaskUpdateTool {
    store: StructuredTaskStore,
}

impl TaskUpdateTool {
    pub fn new(store: StructuredTaskStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::TaskUpdate.as_str()
    }

    fn description(&self) -> &str {
        prompts::TASK_UPDATE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskId": {
                    "type": "string",
                    "description": "The ID of the task to update"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "deleted"],
                    "description": "New status for the task"
                },
                "subject": {
                    "type": "string",
                    "description": "Updated task title"
                },
                "description": {
                    "type": "string",
                    "description": "Updated task description"
                },
                "activeForm": {
                    "type": "string",
                    "description": "Updated progress display text"
                },
                "addBlocks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs to add to 'blocks' list"
                },
                "addBlockedBy": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs to add to 'blockedBy' list"
                },
                "removeBlocks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs to remove from 'blocks' list"
                },
                "removeBlockedBy": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs to remove from 'blockedBy' list"
                },
                "owner": {
                    "type": "string",
                    "description": "Updated owner (agent/team name) for task assignment"
                },
                "metadata": {
                    "type": "object",
                    "description": "Metadata to merge into the task"
                }
            },
            "required": ["taskId"]
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
        let id = input["taskId"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "taskId must be a string",
            }
            .build()
        })?;

        let snapshot = {
            let mut store = self.store.lock().await;

            // Validate status transition before mutating
            if let Some(status_str) = input["status"].as_str() {
                let new_status = TaskStatus::parse(status_str).ok_or_else(|| {
                    crate::error::tool_error::InvalidInputSnafu {
                        message: format!("Invalid status: {status_str}"),
                    }
                    .build()
                })?;

                // Enforce max 1 in_progress
                if matches!(new_status, TaskStatus::InProgress) {
                    let already_in_progress = store
                        .get(id)
                        .is_some_and(|t| matches!(t.status, TaskStatus::InProgress));
                    if !already_in_progress
                        && store
                            .values()
                            .any(|t| t.id != id && matches!(t.status, TaskStatus::InProgress))
                    {
                        return Err(crate::error::tool_error::InvalidInputSnafu {
                            message: "At most 1 task can be in_progress at a time",
                        }
                        .build());
                    }
                }
            }

            // Get mutable ref to the task (single borrow for all mutations)
            let task = store.get_mut(id).ok_or_else(|| {
                crate::error::tool_error::InvalidInputSnafu {
                    message: format!("Task not found: {id}"),
                }
                .build()
            })?;

            // Apply status update
            if let Some(status_str) = input["status"].as_str() {
                if let Some(new_status) = TaskStatus::parse(status_str) {
                    task.status = new_status;
                }
            }

            // Update other fields
            if let Some(subject) = input["subject"].as_str() {
                task.subject = subject.to_string();
            }
            if let Some(desc) = input["description"].as_str() {
                task.description = Some(desc.to_string());
            }
            if let Some(active_form) = input["activeForm"].as_str() {
                task.active_form = Some(active_form.to_string());
            }
            if let Some(owner) = input["owner"].as_str() {
                task.owner = Some(owner.to_string());
            }

            // Update dependencies
            if let Some(add_blocks) = input["addBlocks"].as_array() {
                for v in add_blocks {
                    if let Some(s) = v.as_str() {
                        let s = s.to_string();
                        if !task.blocks.contains(&s) {
                            task.blocks.push(s);
                        }
                    }
                }
            }
            if let Some(add_blocked_by) = input["addBlockedBy"].as_array() {
                for v in add_blocked_by {
                    if let Some(s) = v.as_str() {
                        let s = s.to_string();
                        if !task.blocked_by.contains(&s) {
                            task.blocked_by.push(s);
                        }
                    }
                }
            }
            if let Some(remove_blocks) = input["removeBlocks"].as_array() {
                let remove: Vec<String> = remove_blocks
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                task.blocks.retain(|b| !remove.contains(b));
            }
            if let Some(remove_blocked_by) = input["removeBlockedBy"].as_array() {
                let remove: Vec<String> = remove_blocked_by
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                task.blocked_by.retain(|b| !remove.contains(b));
            }

            // Update metadata (merge)
            if let Some(meta) = input.get("metadata") {
                if let (Value::Object(existing), Value::Object(new)) = (&mut task.metadata, meta) {
                    for (k, v) in new {
                        existing.insert(k.clone(), v.clone());
                    }
                } else if !meta.is_null() {
                    task.metadata = meta.clone();
                }
            }

            structured_tasks::tasks_to_value(&store)
        };

        ctx.emit_progress(format!("Updated task {id}")).await;

        Ok(
            ToolOutput::text(format!("Task {id} updated successfully.")).with_modifier(
                cocode_protocol::ContextModifier::StructuredTasksUpdated { tasks: snapshot },
            ),
        )
    }
}

#[cfg(test)]
#[path = "task_update.test.rs"]
mod tests;
